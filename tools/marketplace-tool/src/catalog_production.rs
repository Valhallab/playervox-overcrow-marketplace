use crate::{admission, package, private_fs};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use ring::signature::{ED25519, UnparsedPublicKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

const KEY_ID: &str = "overcrow-production-2026-01";
const PUBLIC_KEY: &str = include_str!("../../../keys/overcrow-production-2026-01.pub");
const BASE_URL: &str = "https://overcrow.playervox.com/marketplace/v1/";
const MAX_PAYLOAD: u64 = 700 * 1024;
const MAX_ENVELOPE: u64 = 1024 * 1024;
const MAX_TARGETS: usize = 500;
const MAX_REQUEST: u64 = 128 * 1024;
const MAX_TREE_ENTRIES: usize = MAX_TARGETS * 4 + 4;

pub struct PrepareOptions<'a> {
    pub store: &'a Path,
    pub review_tree: &'a str,
    pub state: &'a Path,
    pub request: &'a Path,
    pub output: &'a Path,
    pub previous_output: Option<&'a Path>,
}

pub struct FinalizeOptions<'a> {
    pub prepared: &'a Path,
    pub state: &'a Path,
    pub signature: &'a Path,
    pub output: &'a Path,
}

#[derive(Debug)]
pub struct ProductionError;

impl fmt::Display for ProductionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("production catalog operation rejected")
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Verified,
    SecuritySuspended,
    Revoked,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    sequence: u64,
    previous_sequence: u64,
    generated_at: String,
    statuses: Vec<StatusChange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusChange {
    id: String,
    version: String,
    status: Status,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    schema_version: u32,
    sequence: u64,
    generated_at: String,
    expires_at: String,
    targets: Vec<Target>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Target {
    manifest: serde_json::Value,
    listing: package::Listing,
    package_url: String,
    package_size: u64,
    package_sha256: String,
    status: Status,
    preview: Option<()>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    key_id: String,
    payload: String,
    signature: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reservation {
    schema_version: u32,
    sequence: u64,
    previous_sequence: u64,
    payload_sha256: String,
    generated_at: String,
    expires_at: String,
    previous_catalog_sha256: Option<String>,
    completed_catalog_sha256: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preparation {
    schema_version: u32,
    sequence: u64,
    payload_sha256: String,
}

pub fn prepare(options: &PrepareOptions<'_>) -> Result<usize, ProductionError> {
    prepare_inner(
        options,
        &compiled_key()?,
        DateTime::<Utc>::from(SystemTime::now()),
    )
}

pub fn finalize(options: &FinalizeOptions<'_>) -> Result<usize, ProductionError> {
    finalize_inner(
        options,
        &compiled_key()?,
        DateTime::<Utc>::from(SystemTime::now()),
    )
}

pub fn command(command: &str, args: &[String]) -> Result<(usize, PathBuf), &'static str> {
    const INVALID: &str = "invalid production catalog arguments";
    const REJECTED: &str = "production catalog operation rejected";
    if command == "prepare-production-catalog" {
        if !matches!(args.len(), 10 | 12)
            || args[0] != "--store"
            || args[2] != "--review-tree"
            || args[4] != "--state"
            || args[6] != "--request"
            || args[8] != "--output"
            || (args.len() == 12 && args[10] != "--previous-output")
        {
            return Err(INVALID);
        }
        let options = PrepareOptions {
            store: Path::new(&args[1]),
            review_tree: &args[3],
            state: Path::new(&args[5]),
            request: Path::new(&args[7]),
            output: Path::new(&args[9]),
            previous_output: args.get(11).map(Path::new),
        };
        Ok((
            prepare(&options).map_err(|_| REJECTED)?,
            options.output.join("payload.json"),
        ))
    } else {
        if args.len() != 8
            || args[0] != "--prepared"
            || args[2] != "--state"
            || args[4] != "--signature"
            || args[6] != "--output"
        {
            return Err(INVALID);
        }
        let options = FinalizeOptions {
            prepared: Path::new(&args[1]),
            state: Path::new(&args[3]),
            signature: Path::new(&args[5]),
            output: Path::new(&args[7]),
        };
        Ok((
            finalize(&options).map_err(|_| REJECTED)?,
            options.output.join("catalog.json"),
        ))
    }
}

fn prepare_inner(
    options: &PrepareOptions<'_>,
    public_key: &[u8],
    now: DateTime<Utc>,
) -> Result<usize, ProductionError> {
    validate_external_directory(options.state)?;
    validate_external_directory(options.output)?;
    let _lock = private_fs::lock_private_directory(options.state)?;
    let _output_lock = private_fs::lock_private_directory(options.output)?;
    distinct_roots(&[
        Some(options.store),
        Some(options.state),
        Some(options.output),
        options.previous_output,
    ])?;
    let request: Request = read_json(options.request, MAX_REQUEST)?;
    if request.schema_version != 1
        || request.previous_sequence.checked_add(1) != Some(request.sequence)
        || request.statuses.len() > MAX_TARGETS
    {
        return Err(ProductionError);
    }
    let generated = timestamp(&request.generated_at)?;
    let expires = generated
        .checked_add_signed(TimeDelta::days(90))
        .ok_or(ProductionError)?;
    if generated > now || expires <= now {
        return Err(ProductionError);
    }
    let current = read_reservation(options.state)?;
    let previous = load_previous(options.previous_output, public_key)?;
    let previous_digest = previous.as_ref().map(|(_, digest)| digest.clone());
    if let Some(current) = &current {
        let expected_previous = if request.sequence == current.sequence {
            current.previous_catalog_sha256.as_ref()
        } else {
            if request.previous_sequence != current.sequence
                || (current.completed_catalog_sha256.is_none()
                    && timestamp(&current.expires_at)? > now)
                || generated < timestamp(&current.generated_at)?
            {
                return Err(ProductionError);
            }
            current
                .completed_catalog_sha256
                .as_ref()
                .or(current.previous_catalog_sha256.as_ref())
        };
        if previous_digest.as_ref() != expected_previous {
            return Err(ProductionError);
        }
    }
    if let Some((previous, _)) = &previous
        && (previous.sequence > request.previous_sequence
            || generated < timestamp(&previous.generated_at)?)
    {
        return Err(ProductionError);
    }
    let mut targets = BTreeMap::new();
    let mut sources = BTreeMap::new();
    if let Some((payload, _)) = previous {
        let root = options.previous_output.ok_or(ProductionError)?;
        verify_inventory(root, &inventory(&payload, &["catalog.json"])?, false)?;
        for target in payload.targets {
            let identity = identity(&target)?;
            sources.insert(identity.clone(), root.join(relative_package(&target)?));
            targets.insert(identity, target);
        }
    }
    let admitted = admission::load_verified(options.store, options.review_tree)
        .map_err(|_| ProductionError)?;
    for artifact in admitted.artifacts {
        let identity = (artifact.id.clone(), artifact.version.clone());
        let candidate = Target {
            manifest: artifact.manifest,
            listing: artifact.listing,
            package_url: format!(
                "{BASE_URL}packages/{}/{}/{}.ocpkg",
                artifact.id, artifact.version, artifact.package_sha256
            ),
            package_size: artifact.package_size,
            package_sha256: artifact.package_sha256,
            status: Status::Verified,
            preview: None,
        };
        if let Some(existing) = targets.get(&identity) {
            if !same_artifact(existing, &candidate)? {
                return Err(ProductionError);
            }
        } else {
            let version = Version::parse(&identity.1).map_err(|_| ProductionError)?;
            if targets.keys().any(|(id, old_version)| {
                id == &identity.0 && Version::parse(old_version).is_ok_and(|old| old > version)
            }) {
                return Err(ProductionError);
            }
            targets.insert(identity.clone(), candidate);
        }
        sources.insert(identity, artifact.package_path);
    }
    let mut changed = BTreeSet::new();
    for change in request.statuses {
        let identity = (change.id, change.version);
        if !changed.insert(identity.clone()) {
            return Err(ProductionError);
        }
        let target = targets.get_mut(&identity).ok_or(ProductionError)?;
        if target.status == Status::Revoked && change.status != Status::Revoked {
            return Err(ProductionError);
        }
        target.status = change.status;
    }
    let payload = Payload {
        schema_version: 1,
        sequence: request.sequence,
        generated_at: request.generated_at,
        expires_at: expires.to_rfc3339_opts(SecondsFormat::Secs, true),
        targets: targets.into_values().collect(),
    };
    validate_payload(&payload)?;
    let bytes = serde_json::to_vec(&payload).map_err(|_| ProductionError)?;
    if bytes.len() as u64 > MAX_PAYLOAD {
        return Err(ProductionError);
    }
    let digest = hash(&bytes);
    let reservation = Reservation {
        schema_version: 1,
        sequence: payload.sequence,
        previous_sequence: request.previous_sequence,
        payload_sha256: digest.clone(),
        generated_at: payload.generated_at.clone(),
        expires_at: payload.expires_at.clone(),
        previous_catalog_sha256: previous_digest,
        completed_catalog_sha256: None,
    };
    let expected_inventory = inventory(&payload, &["payload.json", "preparation.json"])?;
    verify_inventory(options.output, &expected_inventory, true)?;
    if let Some(current) = current.filter(|current| current.sequence == payload.sequence) {
        if current.payload_sha256 != digest
            || current.previous_sequence != request.previous_sequence
        {
            return Err(ProductionError);
        }
    } else {
        write_reservation(options.state, &reservation)?;
    }
    // Only one archive is resident at a time, including historical versions.
    for target in &payload.targets {
        let source = sources.get(&identity(target)?).ok_or(ProductionError)?;
        copy_target(source, options.output, target)?;
    }
    private_fs::commit_file(options.output, &options.output.join("payload.json"), &bytes)?;
    let marker = serde_json::to_vec(&Preparation {
        schema_version: 1,
        sequence: payload.sequence,
        payload_sha256: digest,
    })
    .map_err(|_| ProductionError)?;
    private_fs::commit_file(
        options.output,
        &options.output.join("preparation.json"),
        &marker,
    )?;
    verify_inventory(options.output, &expected_inventory, false)?;
    Ok(payload.targets.len())
}

fn finalize_inner(
    options: &FinalizeOptions<'_>,
    public_key: &[u8],
    now: DateTime<Utc>,
) -> Result<usize, ProductionError> {
    validate_external_directory(options.state)?;
    validate_external_directory(options.prepared)?;
    validate_external_directory(options.output)?;
    distinct_roots(&[
        Some(options.state),
        Some(options.prepared),
        Some(options.output),
    ])?;
    let _lock = private_fs::lock_private_directory(options.state)?;
    let _output_lock = private_fs::lock_private_directory(options.output)?;
    let mut reservation = read_reservation(options.state)?.ok_or(ProductionError)?;
    let marker: Preparation = read_json(&options.prepared.join("preparation.json"), 1024)?;
    let bytes = private_fs::read_regular_file(&options.prepared.join("payload.json"), MAX_PAYLOAD)?;
    let payload: Payload = serde_json::from_slice(&bytes).map_err(|_| ProductionError)?;
    validate_payload(&payload)?;
    if marker.schema_version != 1
        || marker.sequence != payload.sequence
        || marker.payload_sha256 != hash(&bytes)
        || reservation.sequence != payload.sequence
        || reservation.payload_sha256 != marker.payload_sha256
        || serde_json::to_vec(&payload).map_err(|_| ProductionError)? != bytes
        || timestamp(&payload.generated_at)? > now
        || timestamp(&payload.expires_at)? <= now
    {
        return Err(ProductionError);
    }
    let signature = private_fs::read_regular_file(options.signature, 64)?;
    if signature.len() != 64 {
        return Err(ProductionError);
    }
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&bytes, &signature)
        .map_err(|_| ProductionError)?;
    let envelope = serde_json::to_vec(&Envelope {
        schema_version: 1,
        key_id: KEY_ID.to_owned(),
        payload: URL_SAFE_NO_PAD.encode(&bytes),
        signature: URL_SAFE_NO_PAD.encode(&signature),
    })
    .map_err(|_| ProductionError)?;
    if envelope.len() as u64 > MAX_ENVELOPE {
        return Err(ProductionError);
    }
    let envelope_digest = hash(&envelope);
    if reservation
        .completed_catalog_sha256
        .as_ref()
        .is_some_and(|digest| digest != &envelope_digest)
    {
        return Err(ProductionError);
    }
    verify_inventory(
        options.prepared,
        &inventory(&payload, &["payload.json", "preparation.json"])?,
        false,
    )?;
    let expected_inventory = inventory(&payload, &["catalog.json"])?;
    verify_inventory(options.output, &expected_inventory, true)?;
    for target in &payload.targets {
        copy_target(
            &options.prepared.join(relative_package(target)?),
            options.output,
            target,
        )?;
    }
    // Persist the verified signature's envelope digest before making the catalog
    // visible. A crash cannot forget a possibly published security transition.
    reservation.completed_catalog_sha256 = Some(envelope_digest);
    write_reservation(options.state, &reservation)?;
    // No usable catalog exists until every retained archive is revalidated and committed.
    private_fs::commit_file(
        options.output,
        &options.output.join("catalog.json"),
        &envelope,
    )?;
    verify_inventory(options.output, &expected_inventory, false)?;
    Ok(payload.targets.len())
}

fn compiled_key() -> Result<[u8; 32], ProductionError> {
    let text = PUBLIC_KEY.trim_end_matches('\n');
    if !valid_hash(text) {
        return Err(ProductionError);
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| ProductionError)?;
    }
    Ok(key)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, limit: u64) -> Result<T, ProductionError> {
    serde_json::from_slice(&private_fs::read_regular_file(path, limit)?)
        .map_err(|_| ProductionError)
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, ProductionError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ProductionError)?
        .with_timezone(&Utc);
    if value.len() != 20 || parsed.to_rfc3339_opts(SecondsFormat::Secs, true) != value {
        return Err(ProductionError);
    }
    Ok(parsed)
}

fn read_reservation(state: &Path) -> Result<Option<Reservation>, ProductionError> {
    let path = state.join("state.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ProductionError),
        Ok(_) => {
            let record: Reservation = read_json(&path, 2048)?;
            if record.schema_version != 1
                || record.previous_sequence.checked_add(1) != Some(record.sequence)
                || !valid_hash(&record.payload_sha256)
                || record
                    .previous_catalog_sha256
                    .as_ref()
                    .is_some_and(|digest| !valid_hash(digest))
                || record
                    .completed_catalog_sha256
                    .as_ref()
                    .is_some_and(|digest| !valid_hash(digest))
                || timestamp(&record.expires_at)?
                    .signed_duration_since(timestamp(&record.generated_at)?)
                    != TimeDelta::days(90)
            {
                return Err(ProductionError);
            }
            Ok(Some(record))
        }
    }
}

fn write_reservation(state: &Path, record: &Reservation) -> Result<(), ProductionError> {
    let bytes = serde_json::to_vec(record).map_err(|_| ProductionError)?;
    private_fs::replace_file(state, &state.join("state.json"), &bytes)?;
    Ok(())
}

fn load_previous(
    root: Option<&Path>,
    public_key: &[u8],
) -> Result<Option<(Payload, String)>, ProductionError> {
    let Some(root) = root else {
        return Ok(None);
    };
    validate_external_directory(root)?;
    let bytes = private_fs::read_regular_file(&root.join("catalog.json"), MAX_ENVELOPE)?;
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| ProductionError)?;
    if envelope.schema_version != 1 || envelope.key_id != KEY_ID {
        return Err(ProductionError);
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(envelope.payload.as_bytes())
        .map_err(|_| ProductionError)?;
    let signature = URL_SAFE_NO_PAD
        .decode(envelope.signature.as_bytes())
        .map_err(|_| ProductionError)?;
    if payload_bytes.len() as u64 > MAX_PAYLOAD || signature.len() != 64 {
        return Err(ProductionError);
    }
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload_bytes, &signature)
        .map_err(|_| ProductionError)?;
    let payload: Payload = serde_json::from_slice(&payload_bytes).map_err(|_| ProductionError)?;
    validate_payload(&payload)?;
    Ok(Some((payload, hash(&bytes))))
}

fn validate_payload(payload: &Payload) -> Result<(), ProductionError> {
    if payload.schema_version != 1
        || payload.sequence == 0
        || payload.targets.is_empty()
        || payload.targets.len() > MAX_TARGETS
        || timestamp(&payload.expires_at)?.signed_duration_since(timestamp(&payload.generated_at)?)
            != TimeDelta::days(90)
    {
        return Err(ProductionError);
    }
    let mut identities = BTreeSet::new();
    for target in &payload.targets {
        if !identities.insert(identity(target)?)
            || target.preview.is_some()
            || target.package_size == 0
            || target.package_size > package::MAX_PACKAGE_BYTES as u64
            || !valid_hash(&target.package_sha256)
        {
            return Err(ProductionError);
        }
        relative_package(target)?;
        let listing = serde_json::to_vec(&target.listing).map_err(|_| ProductionError)?;
        package::parse_listing_bytes(&listing).map_err(|_| ProductionError)?;
    }
    Ok(())
}

fn identity(target: &Target) -> Result<(String, String), ProductionError> {
    let id = target
        .manifest
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or(ProductionError)?;
    let version = target
        .manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or(ProductionError)?;
    if !package::valid_extension_id(id) || !package::canonical_semver(version) {
        return Err(ProductionError);
    }
    Ok((id.to_owned(), version.to_owned()))
}

fn relative_package(target: &Target) -> Result<String, ProductionError> {
    let (id, version) = identity(target)?;
    if !valid_hash(&target.package_sha256) {
        return Err(ProductionError);
    }
    let relative = format!("packages/{id}/{version}/{}.ocpkg", target.package_sha256);
    if target.package_url != format!("{BASE_URL}{relative}") {
        return Err(ProductionError);
    }
    Ok(relative)
}

fn same_artifact(left: &Target, right: &Target) -> Result<bool, ProductionError> {
    Ok(left.package_size == right.package_size
        && left.package_sha256 == right.package_sha256
        && left.manifest == right.manifest
        && serde_json::to_vec(&left.listing).map_err(|_| ProductionError)?
            == serde_json::to_vec(&right.listing).map_err(|_| ProductionError)?)
}

fn copy_target(source: &Path, output: &Path, target: &Target) -> Result<(), ProductionError> {
    let bytes = private_fs::read_regular_file(source, target.package_size)?;
    if bytes.len() as u64 != target.package_size || hash(&bytes) != target.package_sha256 {
        return Err(ProductionError);
    }
    let manifest = package::inspect_bytes(&bytes).map_err(|_| ProductionError)?;
    if manifest.catalog_value != target.manifest {
        return Err(ProductionError);
    }
    let (id, version) = identity(target)?;
    let packages = private_fs::ensure_private_directory(&output.join("packages"))?;
    let identity = private_fs::ensure_private_directory(&packages.join(id))?;
    private_fs::ensure_private_directory(&identity.join(version))?;
    private_fs::commit_file(output, &output.join(relative_package(target)?), &bytes)?;
    Ok(())
}

fn inventory(payload: &Payload, files: &[&str]) -> Result<BTreeSet<String>, ProductionError> {
    let mut expected = files
        .iter()
        .map(|file| (*file).to_owned())
        .collect::<BTreeSet<_>>();
    for target in &payload.targets {
        expected.insert(relative_package(target)?);
    }
    Ok(expected)
}

fn verify_inventory(
    root: &Path,
    expected: &BTreeSet<String>,
    incomplete: bool,
) -> Result<(), ProductionError> {
    let mut actual = BTreeSet::new();
    let mut pending = vec![(root.to_path_buf(), String::new(), 0)];
    let mut count = 0_usize;
    while let Some((directory, prefix, depth)) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|_| ProductionError)? {
            let entry = entry.map_err(|_| ProductionError)?;
            count += 1;
            if count > MAX_TREE_ENTRIES {
                return Err(ProductionError);
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ProductionError)?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type().map_err(|_| ProductionError)?;
            if kind.is_dir() {
                if depth >= 3
                    || !expected
                        .iter()
                        .any(|path| path.starts_with(&format!("{relative}/")))
                {
                    return Err(ProductionError);
                }
                private_fs::validate_private_directory(&entry.path())?;
                pending.push((entry.path(), relative, depth + 1));
            } else if kind.is_file() && expected.contains(&relative) {
                actual.insert(relative);
            } else {
                return Err(ProductionError);
            }
        }
    }
    if !incomplete && &actual != expected {
        return Err(ProductionError);
    }
    Ok(())
}

fn validate_external_directory(path: &Path) -> Result<(), ProductionError> {
    private_fs::validate_private_directory(path)?;
    // Operator state and output must stay outside repository working trees.
    if path
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
    {
        return Err(ProductionError);
    }
    Ok(())
}

fn distinct_roots(paths: &[Option<&Path>]) -> Result<(), ProductionError> {
    let paths = paths.iter().flatten().collect::<Vec<_>>();
    for (index, path) in paths.iter().enumerate() {
        for other in paths.iter().skip(index + 1) {
            if path.starts_with(other) || other.starts_with(path) {
                return Err(ProductionError);
            }
        }
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    package::sha256_hex(&Sha256::digest(bytes).into())
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
impl From<private_fs::PrivateFsError> for ProductionError {
    fn from(_: private_fs::PrivateFsError) -> Self {
        Self
    }
}

#[cfg(test)]
#[path = "catalog_production_tests.rs"]
mod tests;
