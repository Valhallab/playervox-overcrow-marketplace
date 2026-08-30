mod cargo_manifest;
mod catalog;
mod metadata;
mod package;

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::File,
    io::{Read as _, Write as _},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

use crate::{
    catalog::{
        CatalogOrigin, DEV_SEED, DEVELOPMENT_KEY_ID, PreparedTarget, PublisherState, build_catalog,
        parse_counter, read_private_input, verify_catalog,
    },
    metadata::{
        TargetSpec, bind_manifest_digests, inspect_component, validate_build_bindings,
        validate_metadata,
    },
    package::{
        PublisherOutput, build_package, read_private_file, read_source_file, replace_source_file,
    },
};
use ring::signature::{Ed25519KeyPair, KeyPair as _};

const MAX_SNAPSHOT_PLAN_BYTES: usize = 1024 * 1024;
const MAX_SNAPSHOT_ENTRIES: usize = 1_000;
const MAX_SNAPSHOT_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SNAPSHOT_AGGREGATE_BYTES: u64 = 16 * 1024 * 1024;
const SNAPSHOT_PLAN_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum AppError {
    Arguments,
    Input,
    Policy,
    Package,
    Catalog,
    State,
    Output,
    Verification,
}

impl AppError {
    const fn message(self) -> &'static str {
        match self {
            Self::Arguments => "invalid arguments",
            Self::Input => "invalid input",
            Self::Policy => "metadata policy rejected",
            Self::Package => "package build failed",
            Self::Catalog => "catalog build failed",
            Self::State => "publisher state rejected",
            Self::Output => "publication failed",
            Self::Verification => "catalog verification failed",
        }
    }
}

enum BuildMode {
    Development,
    Production {
        sequence_file: PathBuf,
        sequence_state: PathBuf,
        signing_key: PathBuf,
        key_id: String,
    },
}

struct BuildOptions {
    repository: PathBuf,
    generated_at: String,
    expires_at: String,
    mode: BuildMode,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut output = [0; 32];
    output.copy_from_slice(digest.as_ref());
    output
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("marketplace-tool: {}", error.message());
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<OsString>) -> Result<(), AppError> {
    let arguments: Vec<String> = arguments
        .into_iter()
        .map(|argument| argument.into_string().map_err(|_| AppError::Arguments))
        .collect::<Result<_, _>>()?;
    match arguments.first().map(String::as_str) {
        Some("build") => build(parse_build(&arguments)?),
        Some("verify") => verify(&arguments),
        Some("inspect-component") => inspect(&arguments),
        Some("policy") => policy(&arguments),
        Some("build-plan") => build_plan(&arguments),
        Some("bind-build") => bind_build(&arguments),
        Some("snapshot-plan") => snapshot_plan(&arguments),
        _ => Err(AppError::Arguments),
    }
}

fn parse_build(arguments: &[String]) -> Result<BuildOptions, AppError> {
    if arguments.len() < 8
        || arguments[1] != "--repository"
        || arguments[3] != "--generated-at"
        || arguments[5] != "--expires-at"
    {
        return Err(AppError::Arguments);
    }
    let mode = if arguments.len() == 8 && arguments[7] == "--development-key" {
        BuildMode::Development
    } else if arguments.len() == 16
        && arguments[7] == "--production"
        && arguments[8] == "--sequence-file"
        && arguments[10] == "--sequence-state"
        && arguments[12] == "--signing-key"
        && arguments[14] == "--key-id"
    {
        BuildMode::Production {
            sequence_file: PathBuf::from(&arguments[9]),
            sequence_state: PathBuf::from(&arguments[11]),
            signing_key: PathBuf::from(&arguments[13]),
            key_id: arguments[15].clone(),
        }
    } else {
        return Err(AppError::Arguments);
    };
    Ok(BuildOptions {
        repository: PathBuf::from(&arguments[2]),
        generated_at: arguments[4].clone(),
        expires_at: arguments[6].clone(),
        mode,
    })
}

fn build(options: BuildOptions) -> Result<(), AppError> {
    let targets = load_build_plan(&options.repository)?;
    let packages = targets
        .iter()
        .map(|target| build_package(&options.repository, target).map_err(|_| AppError::Package))
        .collect::<Result<Vec<_>, _>>()?;

    let (mut state, sequence, key_id, seed, origin) = match options.mode {
        BuildMode::Development => {
            let state_path = options
                .repository
                .join("marketplace/development-catalog-state.json");
            let state = PublisherState::development(&state_path).map_err(|_| AppError::State)?;
            let counter = read_source_file(
                &options.repository,
                "marketplace/development-sequence.txt",
                32,
            )
            .map_err(|_| AppError::Input)?;
            let seed = read_source_file(
                &options.repository,
                "fixtures/keys/development-ed25519.key",
                65,
            )
            .map_err(|_| AppError::Input)
            .and_then(|bytes| decode_hex_32(&bytes).ok_or(AppError::Input))?;
            if seed != DEV_SEED {
                return Err(AppError::Input);
            }
            (
                state,
                parse_counter(&counter).map_err(|_| AppError::State)?,
                DEVELOPMENT_KEY_ID.to_owned(),
                seed,
                CatalogOrigin::Development,
            )
        }
        BuildMode::Production {
            sequence_file,
            sequence_state,
            signing_key,
            key_id,
        } => {
            if key_id == DEVELOPMENT_KEY_ID
                || sequence_file == sequence_state
                || sequence_file == signing_key
                || sequence_state == signing_key
            {
                return Err(AppError::Arguments);
            }
            let state = PublisherState::production(&options.repository, &sequence_state)
                .map_err(|_| AppError::State)?;
            let counter = read_private_input(&options.repository, &sequence_file, 32)
                .map_err(|_| AppError::Input)?;
            let seed = read_private_input(&options.repository, &signing_key, 65)
                .map_err(|_| AppError::Input)
                .and_then(|bytes| decode_hex_32(&bytes).ok_or(AppError::Input))?;
            if seed == DEV_SEED {
                return Err(AppError::Input);
            }
            (
                state,
                parse_counter(&counter).map_err(|_| AppError::State)?,
                key_id,
                seed,
                CatalogOrigin::Production,
            )
        }
    };
    let prepared: Vec<_> = packages
        .iter()
        .zip(&targets)
        .map(|(package, target)| PreparedTarget {
            package,
            status: target.status(),
        })
        .collect();
    let catalog = build_catalog(
        &prepared,
        sequence,
        &options.generated_at,
        &options.expires_at,
        origin,
        &key_id,
        &seed,
    )
    .map_err(|_| AppError::Catalog)?;
    state
        .accept(sequence, sha256(&catalog.payload))
        .map_err(|_| AppError::State)?;
    let output = PublisherOutput::open(&options.repository).map_err(|_| AppError::Output)?;
    output
        .publish_objects(&packages)
        .map_err(|_| AppError::Output)?;
    output
        .publish_catalog(&catalog.envelope)
        .map_err(|_| AppError::Output)
}

fn verify(arguments: &[String]) -> Result<(), AppError> {
    if !matches!(arguments.len(), 2 | 6) {
        return Err(AppError::Arguments);
    }
    let catalog_path = Path::new(&arguments[1]);
    let bytes = read_cli_file(catalog_path, 1024 * 1024).map_err(|_| AppError::Input)?;
    let (key_id, public_key, origin) = if arguments.len() == 2 {
        (
            DEVELOPMENT_KEY_ID,
            public_key(&DEV_SEED)?,
            CatalogOrigin::Development,
        )
    } else if arguments[2] == "--public-key" && arguments[4] == "--key-id" {
        let bytes = read_cli_file(Path::new(&arguments[3]), 65).map_err(|_| AppError::Input)?;
        let public_key = decode_hex_32(&bytes).ok_or(AppError::Input)?;
        (arguments[5].as_str(), public_key, CatalogOrigin::Production)
    } else {
        return Err(AppError::Arguments);
    };
    verify_catalog(&bytes, key_id, &public_key, origin, chrono::Utc::now())
        .map_err(|_| AppError::Verification)
}

fn inspect(arguments: &[String]) -> Result<(), AppError> {
    if arguments.len() != 2 {
        return Err(AppError::Arguments);
    }
    let bytes =
        read_cli_file(Path::new(&arguments[1]), 4 * 1024 * 1024).map_err(|_| AppError::Input)?;
    inspect_component(&bytes).map_err(|_| AppError::Policy)
}

fn policy(arguments: &[String]) -> Result<(), AppError> {
    if arguments.len() != 3 || arguments[1] != "--repository" {
        return Err(AppError::Arguments);
    }
    let repository = Path::new(&arguments[2]);
    let targets = load_build_plan(repository)?;
    for target in &targets {
        build_package(repository, target).map_err(|_| AppError::Package)?;
    }
    Ok(())
}

fn build_plan(arguments: &[String]) -> Result<(), AppError> {
    if arguments.len() != 3 || arguments[1] != "--repository" {
        return Err(AppError::Arguments);
    }
    let targets = load_build_plan(Path::new(&arguments[2]))?;
    let mut output = String::new();
    for target in &targets {
        let entry = target.build_plan_entry();
        use std::fmt::Write as _;
        writeln!(
            output,
            "{}\t{}\t{}",
            entry.cargo_package, entry.component_artifact, entry.source_directory
        )
        .map_err(|_| AppError::Output)?;
    }
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|_| AppError::Output)
}

fn snapshot_plan(arguments: &[String]) -> Result<(), AppError> {
    if arguments.len() != 5 || arguments[1] != "--repository" || arguments[3] != "--revision" {
        return Err(AppError::Arguments);
    }
    let repository = Path::new(&arguments[2]);
    if !safe_owned_root(repository) || !valid_object_id(&arguments[4]) {
        return Err(AppError::Policy);
    }
    let mut child = Command::new("/usr/bin/git")
        .args([
            "--no-replace-objects",
            "-C",
            repository.to_str().ok_or(AppError::Policy)?,
            "ls-tree",
            "-r",
            "-z",
            "-l",
            "--full-tree",
            &arguments[4],
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| AppError::Policy)?;
    let stdout = child.stdout.take().ok_or(AppError::Policy)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_SNAPSHOT_PLAN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + SNAPSHOT_PLAN_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| AppError::Policy)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Policy);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let bytes = reader
        .join()
        .map_err(|_| AppError::Policy)?
        .map_err(|_| AppError::Policy)?;
    if !status.success() || bytes.len() > MAX_SNAPSHOT_PLAN_BYTES {
        return Err(AppError::Policy);
    }
    let entries = validate_snapshot_plan(&bytes)?;
    let mut output = String::new();
    for entry in entries {
        use std::fmt::Write as _;
        writeln!(
            output,
            "{}\t{}\t{}\t{}",
            entry.mode, entry.size, entry.object, entry.path
        )
        .map_err(|_| AppError::Output)?;
    }
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|_| AppError::Output)
}

struct SnapshotEntry {
    mode: String,
    size: u64,
    object: String,
    path: String,
}

fn validate_snapshot_plan(bytes: &[u8]) -> Result<Vec<SnapshotEntry>, AppError> {
    let mut paths = BTreeSet::new();
    let mut aggregate = 0_u64;
    let mut entries = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(AppError::Policy)?;
        let header = std::str::from_utf8(&record[..tab]).map_err(|_| AppError::Policy)?;
        let mut fields = header.split_ascii_whitespace();
        let mode = fields.next().ok_or(AppError::Policy)?;
        let kind = fields.next().ok_or(AppError::Policy)?;
        let object = fields.next().ok_or(AppError::Policy)?;
        let size = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(AppError::Policy)?;
        if fields.next().is_some()
            || !matches!(mode, "100644" | "100755")
            || kind != "blob"
            || !valid_object_id(object)
            || size > MAX_SNAPSHOT_FILE_BYTES
        {
            return Err(AppError::Policy);
        }
        let path = std::str::from_utf8(&record[tab + 1..]).map_err(|_| AppError::Policy)?;
        if !valid_snapshot_path(path) || !paths.insert(path) {
            return Err(AppError::Policy);
        }
        if paths.len() > MAX_SNAPSHOT_ENTRIES {
            return Err(AppError::Policy);
        }
        aggregate = aggregate.checked_add(size).ok_or(AppError::Policy)?;
        if aggregate > MAX_SNAPSHOT_AGGREGATE_BYTES {
            return Err(AppError::Policy);
        }
        entries.push(SnapshotEntry {
            mode: mode.to_owned(),
            size,
            object: object.to_owned(),
            path: path.to_owned(),
        });
    }
    if entries.is_empty() {
        return Err(AppError::Policy);
    }
    Ok(entries)
}

fn valid_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_snapshot_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && value
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

fn safe_owned_root(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return false;
    };
    if canonical != path {
        return false;
    }
    File::open(path).is_ok_and(|directory| owned_directory_is_safe(&directory, false))
}

fn bind_build(arguments: &[String]) -> Result<(), AppError> {
    if arguments.len() != 5 || arguments[1] != "--repository" || arguments[3] != "--bindings" {
        return Err(AppError::Arguments);
    }
    let repository = Path::new(&arguments[2]);
    let binding_path = Path::new(&arguments[4]);
    if !binding_path.is_absolute() || !binding_path.starts_with(repository) {
        return Err(AppError::Arguments);
    }
    let targets = load_build_plan(repository)?;
    let binding_bytes = read_private_file(binding_path, 128 * 1024).map_err(|_| AppError::Input)?;
    let bindings = validate_build_bindings(&binding_bytes).map_err(|_| AppError::Policy)?;
    let components: BTreeMap<_, _> = bindings
        .components()
        .iter()
        .map(|binding| (binding.source_directory(), binding.sha256()))
        .collect();
    if components.len() != targets.len()
        || targets
            .iter()
            .any(|target| !components.contains_key(target.source_directory()))
    {
        return Err(AppError::Policy);
    }
    let providers: BTreeMap<_, _> = bindings
        .providers()
        .iter()
        .map(|binding| {
            (
                (binding.id().to_owned(), binding.version().to_owned()),
                binding.sha256().to_owned(),
            )
        })
        .collect();
    let mut required_providers = BTreeSet::new();
    let mut rewrites = Vec::with_capacity(targets.len());
    for target in &targets {
        let manifest_relative = format!("{}/manifest.json", target.source_directory());
        let listing_relative = format!("{}/listing.json", target.source_directory());
        let manifest = read_source_file(repository, &manifest_relative, 64 * 1024)
            .map_err(|_| AppError::Input)?;
        let listing = read_source_file(repository, &listing_relative, 64 * 1024)
            .map_err(|_| AppError::Input)?;
        let metadata = validate_metadata(&manifest, &listing).map_err(|_| AppError::Policy)?;
        required_providers.extend(
            metadata
                .manifest()
                .dependencies()
                .iter()
                .map(|dependency| (dependency.id().to_owned(), dependency.version().to_owned())),
        );
        let component = components
            .get(target.source_directory())
            .ok_or(AppError::Policy)?;
        let encoded = bind_manifest_digests(&manifest, &listing, component, &providers)
            .map_err(|_| AppError::Policy)?;
        rewrites.push((target.source_directory().to_owned(), encoded));
    }
    if required_providers.len() != providers.len()
        || required_providers
            .iter()
            .any(|provider| !providers.contains_key(provider))
    {
        return Err(AppError::Policy);
    }
    for (source_directory, bytes) in rewrites {
        replace_source_file(repository, &source_directory, "manifest.json", &bytes)
            .map_err(|_| AppError::Output)?;
    }
    Ok(())
}

fn load_build_plan(repository: &Path) -> Result<Vec<TargetSpec>, AppError> {
    cargo_manifest::load(repository).map_err(|_| AppError::Policy)
}

fn read_cli_file(path: &Path, maximum: usize) -> Result<Vec<u8>, ()> {
    let resolved = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(|_| ())?.join(path)
    };
    let path = resolved.as_path();
    let parent = path.parent().ok_or(())?;
    let name = path.file_name().and_then(|name| name.to_str()).ok_or(())?;
    read_source_file(parent, name, maximum).map_err(|_| ())
}

fn public_key(seed: &[u8; 32]) -> Result<[u8; 32], AppError> {
    let pair = Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| AppError::Verification)?;
    let mut public = [0; 32];
    public.copy_from_slice(pair.public_key().as_ref());
    Ok(public)
}

fn decode_hex_32(bytes: &[u8]) -> Option<[u8; 32]> {
    let value = bytes.strip_suffix(b"\n")?;
    if value.len() != 64 {
        return None;
    }
    let mut decoded = [0; 32];
    for (index, pair) in value.as_chunks::<2>().0.iter().enumerate() {
        decoded[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(decoded)
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn owned_directory_is_safe(directory: &File, private: bool) -> bool {
    directory.metadata().is_ok_and(|metadata| {
        metadata.is_dir()
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && if private {
                metadata.permissions().mode() & 0o7777 == 0o700
            } else {
                metadata.permissions().mode() & 0o022 == 0
            }
    })
}
