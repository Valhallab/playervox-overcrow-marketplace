use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read as _, Write as _},
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _, fs::PermissionsExt as _},
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use rustix::fs::{
    AtFlags, CWD, FlockOperation, Mode, OFlags, ResolveFlags, fchmod, flock, fsync, openat,
    openat2, renameat, unlinkat,
};
use serde::{Deserialize, Serialize};

use crate::{
    metadata::{CatalogStatus, Localization, Manifest, PackageKind},
    package::PackageArtifact,
};

pub(crate) const DEVELOPMENT_KEY_ID: &str = "overcrow-development-2026";
pub(crate) const PRODUCTION_KEY_ID: &str = "overcrow-production-2026-01";
pub(crate) const PRODUCTION_PUBLIC_KEY_PATH: &str = "keys/overcrow-production-2026-01.pub";
const PRODUCTION_CATALOG_LIFETIME_DAYS: i64 = 90;
pub(crate) const DEV_SEED: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];
const MAX_PAYLOAD_BYTES: usize = 700 * 1024;
const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
const MAX_BROWSER_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogCode {
    Envelope,
    Signature,
    Counter,
    State,
    Rollback,
    SequenceConflict,
    Time,
    Target,
    Payload,
}

#[derive(Clone, Copy)]
pub(crate) enum CatalogOrigin {
    Development,
    Production,
}

impl CatalogOrigin {
    const fn base_url(self) -> &'static str {
        match self {
            Self::Development => "http://127.0.0.1:8787/marketplace/v1/",
            Self::Production => "https://overcrow.playervox.com/marketplace/v1/",
        }
    }
}

pub(crate) struct PreparedTarget<'a> {
    pub(crate) package: &'a PackageArtifact,
    pub(crate) status: CatalogStatus,
}

pub(crate) struct CatalogBuild {
    pub(crate) payload: Vec<u8>,
    pub(crate) envelope: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPayload<'a> {
    schema_version: u32,
    sequence: u64,
    generated_at: &'a str,
    expires_at: &'a str,
    #[serde(skip_serializing_if = "static_tree_is_empty")]
    static_tree: &'a [StaticTreeEntry],
    targets: Vec<CatalogTarget<'a>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StaticTreeEntry {
    path: String,
    kind: StaticTreeKind,
    mode: u32,
    size: u64,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum StaticTreeKind {
    Directory,
    File,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogTarget<'a> {
    manifest: &'a Manifest,
    listing: CatalogListing<'a>,
    package_url: String,
    package_size: u64,
    package_sha256: &'a str,
    min_host_api: u32,
    max_host_api: u32,
    status: CatalogStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<CatalogPreview>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogListing<'a> {
    author: &'a str,
    spdx_license: &'a str,
    source_url: &'a str,
    localizations: Vec<CatalogLocalization<'a>>,
}

#[derive(Serialize)]
struct CatalogLocalization<'a> {
    locale: &'a str,
    name: &'a str,
    description: &'a str,
}

impl<'a> CatalogListing<'a> {
    fn new(listing: &'a crate::metadata::Listing) -> Self {
        Self {
            author: listing.author(),
            spdx_license: listing.spdx_license(),
            source_url: listing.source_url(),
            localizations: listing
                .localizations()
                .iter()
                .map(CatalogLocalization::from)
                .collect(),
        }
    }
}

impl<'a> From<&'a Localization> for CatalogLocalization<'a> {
    fn from(value: &'a Localization) -> Self {
        Self {
            locale: value.locale(),
            name: value.name(),
            description: value.description(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPreview {
    url: String,
    media_type: &'static str,
    size: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignedEnvelope {
    schema_version: u32,
    key_id: String,
    payload: String,
    signature: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifiedPayload {
    schema_version: u32,
    sequence: u64,
    generated_at: String,
    expires_at: String,
    #[serde(default)]
    static_tree: Vec<StaticTreeEntry>,
    targets: Vec<VerifiedTarget>,
}

fn static_tree_is_empty(entries: &&[StaticTreeEntry]) -> bool {
    entries.is_empty()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifiedTarget {
    manifest: Manifest,
    listing: VerifiedListing,
    package_url: String,
    package_size: u64,
    package_sha256: String,
    min_host_api: u32,
    max_host_api: u32,
    status: CatalogStatus,
    preview: Option<VerifiedPreview>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifiedListing {
    author: String,
    spdx_license: String,
    source_url: String,
    localizations: Vec<VerifiedLocalization>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerifiedLocalization {
    locale: String,
    name: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifiedPreview {
    url: String,
    media_type: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SequenceState {
    schema_version: u32,
    sequence: u64,
    payload_sha256: String,
}

impl SequenceState {
    pub(crate) fn new(sequence: u64, digest: [u8; 32]) -> Result<Self, CatalogCode> {
        if sequence == 0 {
            return Err(CatalogCode::State);
        }
        Ok(Self {
            schema_version: 1,
            sequence,
            payload_sha256: crate::lower_hex(&digest),
        })
    }

    pub(crate) fn accept(&mut self, sequence: u64, digest: [u8; 32]) -> Result<(), CatalogCode> {
        self.validate()?;
        let digest = crate::lower_hex(&digest);
        if sequence < self.sequence {
            return Err(CatalogCode::Rollback);
        }
        if sequence == self.sequence {
            return if digest == self.payload_sha256 {
                Ok(())
            } else {
                Err(CatalogCode::SequenceConflict)
            };
        }
        self.sequence = sequence;
        self.payload_sha256 = digest;
        Ok(())
    }

    fn validate(&self) -> Result<(), CatalogCode> {
        if self.schema_version == 1 && self.sequence != 0 && valid_digest(&self.payload_sha256) {
            Ok(())
        } else {
            Err(CatalogCode::State)
        }
    }
}

pub(crate) struct PublisherState {
    directory: File,
    directory_path: PathBuf,
    file_name: OsString,
    private: bool,
}

impl PublisherState {
    pub(crate) fn development(path: &Path) -> Result<Self, CatalogCode> {
        Self::open(path, false)
    }

    pub(crate) fn production(repository: &Path, path: &Path) -> Result<Self, CatalogCode> {
        if !normalized_absolute(repository)
            || !normalized_absolute(path)
            || path.starts_with(repository)
        {
            return Err(CatalogCode::State);
        }
        Self::open(path, true)
    }

    fn open(path: &Path, private: bool) -> Result<Self, CatalogCode> {
        if !normalized_absolute(path) {
            return Err(CatalogCode::State);
        }
        let parent = path.parent().ok_or(CatalogCode::State)?;
        let file_name = path
            .file_name()
            .filter(|name| {
                !name.is_empty() && *name != OsStr::new(".") && *name != OsStr::new("..")
            })
            .ok_or(CatalogCode::State)?
            .to_owned();
        let directory = File::from(
            openat2(
                CWD,
                parent,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
            )
            .map_err(|_| CatalogCode::State)?,
        );
        if !crate::owned_directory_is_safe(&directory, private) {
            return Err(CatalogCode::State);
        }
        flock(&directory, FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| CatalogCode::State)?;
        let state = Self {
            directory,
            directory_path: parent.to_owned(),
            file_name,
            private,
        };
        state.read()?;
        Ok(state)
    }

    pub(crate) fn accept(&mut self, sequence: u64, digest: [u8; 32]) -> Result<(), CatalogCode> {
        let next = match self.read()? {
            Some(mut state) => {
                state.accept(sequence, digest)?;
                state
            }
            None => SequenceState::new(sequence, digest)?,
        };
        self.write(&next)
    }

    pub(crate) fn advance_counter(
        &self,
        sequence_file: &Path,
        sequence: u64,
        digest: [u8; 32],
    ) -> Result<(), CatalogCode> {
        if !self.private || !normalized_absolute(sequence_file) {
            return Err(CatalogCode::State);
        }
        let accepted = self.read()?.ok_or(CatalogCode::State)?;
        accepted.validate()?;
        if accepted.sequence != sequence || accepted.payload_sha256 != crate::lower_hex(&digest) {
            return Err(CatalogCode::SequenceConflict);
        }
        let parent = sequence_file.parent().ok_or(CatalogCode::State)?;
        let name = sequence_file
            .file_name()
            .filter(|name| {
                !name.is_empty() && *name != OsStr::new(".") && *name != OsStr::new("..")
            })
            .ok_or(CatalogCode::State)?;
        let separate_directory;
        let directory = if parent == self.directory_path {
            &self.directory
        } else {
            separate_directory = File::from(
                openat2(
                    CWD,
                    parent,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
                )
                .map_err(|_| CatalogCode::State)?,
            );
            if !crate::owned_directory_is_safe(&separate_directory, true) {
                return Err(CatalogCode::State);
            }
            flock(
                &separate_directory,
                FlockOperation::NonBlockingLockExclusive,
            )
            .map_err(|_| CatalogCode::State)?;
            &separate_directory
        };
        let counter = read_private_at(directory, name, 32)?;
        if parse_counter(&counter)? != sequence {
            return Err(CatalogCode::SequenceConflict);
        }
        let next = sequence
            .checked_add(1)
            .filter(|next| *next <= MAX_BROWSER_SEQUENCE)
            .ok_or(CatalogCode::Counter)?;
        write_private_at(directory, name, format!("{next}\n").as_bytes())
    }

    fn read(&self) -> Result<Option<SequenceState>, CatalogCode> {
        let descriptor = match openat(
            &self.directory,
            &self.file_name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
            Err(_) => return Err(CatalogCode::State),
        };
        let file = File::from(descriptor);
        validate_state_file(&file, self.private)?;
        let mut bytes = Vec::new();
        file.take(513)
            .read_to_end(&mut bytes)
            .map_err(|_| CatalogCode::State)?;
        if bytes.len() > 512 {
            return Err(CatalogCode::State);
        }
        let state: SequenceState =
            serde_json::from_slice(&bytes).map_err(|_| CatalogCode::State)?;
        state.validate()?;
        Ok(Some(state))
    }

    fn write(&self, state: &SequenceState) -> Result<(), CatalogCode> {
        let mut bytes = serde_json::to_vec(state).map_err(|_| CatalogCode::State)?;
        bytes.push(b'\n');
        let temporary_name = OsString::from(format!(".catalog-state.{}.tmp", std::process::id()));
        let mode = if self.private {
            Mode::RUSR | Mode::WUSR
        } else {
            Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH
        };
        let descriptor = openat(
            &self.directory,
            &temporary_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            mode,
        )
        .map_err(|_| CatalogCode::State)?;
        let mut temporary = File::from(descriptor);
        let result = (|| {
            validate_state_file(&temporary, self.private)?;
            temporary
                .write_all(&bytes)
                .map_err(|_| CatalogCode::State)?;
            temporary.sync_all().map_err(|_| CatalogCode::State)?;
            renameat(
                &self.directory,
                &temporary_name,
                &self.directory,
                &self.file_name,
            )
            .map_err(|_| CatalogCode::State)?;
            fsync(&self.directory).map_err(|_| CatalogCode::State)
        })();
        if result.is_err() {
            let _ = unlinkat(&self.directory, &temporary_name, AtFlags::empty());
        }
        result
    }
}

fn read_private_at(directory: &File, name: &OsStr, maximum: usize) -> Result<Vec<u8>, CatalogCode> {
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| CatalogCode::State)?;
    let file = File::from(descriptor);
    validate_state_file(&file, true)?;
    let expected = usize::try_from(file.metadata().map_err(|_| CatalogCode::State)?.len())
        .ok()
        .filter(|length| *length <= maximum)
        .ok_or(CatalogCode::State)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| CatalogCode::State)?;
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| CatalogCode::State)?;
    if bytes.len() == expected {
        Ok(bytes)
    } else {
        Err(CatalogCode::State)
    }
}

fn write_private_at(directory: &File, name: &OsStr, bytes: &[u8]) -> Result<(), CatalogCode> {
    if bytes.is_empty() || bytes.len() > 32 {
        return Err(CatalogCode::Counter);
    }
    let temporary_name = OsString::from(format!(".sequence-counter.{}.tmp", std::process::id()));
    let descriptor = openat(
        directory,
        &temporary_name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| CatalogCode::State)?;
    let mut temporary = File::from(descriptor);
    let result = (|| {
        validate_state_file(&temporary, true)?;
        temporary
            .write_all(bytes)
            .and_then(|()| temporary.sync_all())
            .map_err(|_| CatalogCode::State)?;
        renameat(directory, &temporary_name, directory, name).map_err(|_| CatalogCode::State)?;
        fsync(directory).map_err(|_| CatalogCode::State)
    })();
    if result.is_err() {
        let _ = unlinkat(directory, &temporary_name, AtFlags::empty());
    }
    result
}

fn validate_state_file(file: &File, private: bool) -> Result<(), CatalogCode> {
    let metadata = file.metadata().map_err(|_| CatalogCode::State)?;
    let mode = metadata.permissions().mode() & 0o7777;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || mode & 0o022 != 0
        || (private && mode != 0o600)
    {
        return Err(CatalogCode::State);
    }
    Ok(())
}

fn normalized_absolute(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    bytes.starts_with(b"/")
        && bytes.len() > 1
        && !bytes[1..]
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || matches!(part, b"." | b".."))
}

pub(crate) fn parse_counter(bytes: &[u8]) -> Result<u64, CatalogCode> {
    let digits = bytes.strip_suffix(b"\n").ok_or(CatalogCode::Counter)?;
    if digits.is_empty()
        || digits[0] == b'0'
        || !digits.iter().all(u8::is_ascii_digit)
        || digits.len() > 20
    {
        return Err(CatalogCode::Counter);
    }
    let text = std::str::from_utf8(digits).map_err(|_| CatalogCode::Counter)?;
    let value = text.parse::<u64>().map_err(|_| CatalogCode::Counter)?;
    if value == 0 || value.to_string() != text {
        return Err(CatalogCode::Counter);
    }
    Ok(value)
}

pub(crate) fn read_private_input(
    repository: &Path,
    path: &Path,
    maximum: usize,
) -> Result<Vec<u8>, CatalogCode> {
    if !normalized_absolute(repository)
        || !normalized_absolute(path)
        || path.starts_with(repository)
    {
        return Err(CatalogCode::State);
    }
    let parent = path.parent().ok_or(CatalogCode::State)?;
    let name = path.file_name().ok_or(CatalogCode::State)?;
    let directory = File::from(
        openat2(
            CWD,
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| CatalogCode::State)?,
    );
    if !crate::owned_directory_is_safe(&directory, true) {
        return Err(CatalogCode::State);
    }
    let descriptor = openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| CatalogCode::State)?;
    let file = File::from(descriptor);
    validate_state_file(&file, true)?;
    let expected = usize::try_from(file.metadata().map_err(|_| CatalogCode::State)?.len())
        .ok()
        .filter(|length| *length <= maximum)
        .ok_or(CatalogCode::State)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| CatalogCode::State)?;
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| CatalogCode::State)?;
    if bytes.len() == expected {
        Ok(bytes)
    } else {
        Err(CatalogCode::State)
    }
}

pub(crate) fn build_catalog(
    inputs: &[PreparedTarget<'_>],
    sequence: u64,
    generated_at: &str,
    expires_at: &str,
    origin: CatalogOrigin,
    key_id: &str,
    seed: &[u8; 32],
) -> Result<CatalogBuild, CatalogCode> {
    build_catalog_with_static_tree(
        inputs,
        sequence,
        generated_at,
        expires_at,
        origin,
        key_id,
        seed,
        &[],
    )
}

// Keeping one shared signature prevents production-only catalog construction
// from drifting from the development path when the signed static tree is added.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_catalog_with_static_tree(
    inputs: &[PreparedTarget<'_>],
    sequence: u64,
    generated_at: &str,
    expires_at: &str,
    origin: CatalogOrigin,
    key_id: &str,
    seed: &[u8; 32],
    static_tree: &[StaticTreeEntry],
) -> Result<CatalogBuild, CatalogCode> {
    if sequence == 0
        || (matches!(origin, CatalogOrigin::Production) && sequence > MAX_BROWSER_SEQUENCE)
        || inputs.len() > 500
        || static_tree.len() > 1_000
    {
        return Err(CatalogCode::Target);
    }
    validate_static_tree_manifest(static_tree)?;
    let generated = parse_time(generated_at)?;
    let expires = parse_time(expires_at)?;
    if expires <= generated {
        return Err(CatalogCode::Time);
    }
    if matches!(origin, CatalogOrigin::Production)
        && expires.signed_duration_since(generated)
            != chrono::TimeDelta::days(PRODUCTION_CATALOG_LIFETIME_DAYS)
    {
        return Err(CatalogCode::Time);
    }
    let mut inputs: Vec<_> = inputs.iter().collect();
    inputs.sort_by(|left, right| {
        let left = left.package.metadata().manifest();
        let right = right.package.metadata().manifest();
        (left.id(), left.version()).cmp(&(right.id(), right.version()))
    });
    validate_dependencies(&inputs)?;
    let mut targets = Vec::with_capacity(inputs.len());
    for input in inputs {
        let metadata = input.package.metadata();
        let manifest = metadata.manifest();
        if input.package.archive().is_empty() || input.package.archive().len() > 16 * 1024 * 1024 {
            return Err(CatalogCode::Target);
        }
        let preview = input.package.preview().map(|bytes| {
            let sha256 = crate::lower_hex(&crate::sha256(bytes));
            CatalogPreview {
                url: format!(
                    "{}previews/{}/{}/{}.png",
                    origin.base_url(),
                    manifest.id(),
                    manifest.version(),
                    sha256
                ),
                media_type: "image/png",
                size: bytes.len() as u64,
                sha256,
            }
        });
        targets.push(CatalogTarget {
            manifest,
            listing: CatalogListing::new(metadata.listing()),
            package_url: format!(
                "{}packages/{}/{}/{}.ocpkg",
                origin.base_url(),
                manifest.id(),
                manifest.version(),
                input.package.archive_sha256()
            ),
            package_size: input.package.archive().len() as u64,
            package_sha256: input.package.archive_sha256(),
            min_host_api: 1,
            max_host_api: 1,
            status: input.status,
            preview,
        });
    }
    let payload = serde_json::to_vec(&CatalogPayload {
        schema_version: 1,
        sequence,
        generated_at,
        expires_at,
        static_tree,
        targets,
    })
    .map_err(|_| CatalogCode::Payload)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CatalogCode::Payload);
    }
    let envelope = sign_payload(key_id, &payload, seed)?;
    Ok(CatalogBuild { payload, envelope })
}

pub(crate) fn collect_static_tree(repository: &Path) -> Result<Vec<StaticTreeEntry>, CatalogCode> {
    let root = repository.join("public");
    if !root.is_dir() || root.is_symlink() {
        return Ok(Vec::new());
    }
    let mut entries = BTreeMap::new();
    collect_static_entries(&root, &root, &mut entries, 0, &mut 0_u64)?;
    let entries: Vec<_> = entries.into_values().collect();
    validate_static_tree_manifest(&entries)?;
    Ok(entries)
}

fn collect_static_entries(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, StaticTreeEntry>,
    depth: usize,
    aggregate: &mut u64,
) -> Result<(), CatalogCode> {
    if depth > 16 || entries.len() > 1_000 {
        return Err(CatalogCode::Target);
    }
    for entry in std::fs::read_dir(directory).map_err(|_| CatalogCode::Target)? {
        let entry = entry.map_err(|_| CatalogCode::Target)?;
        let path = entry.path();
        let relative = normalized_tree_path(root, &path)?;
        if relative == "marketplace/v1" {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| CatalogCode::Target)?;
        let mode = metadata.permissions().mode() & 0o7777;
        let manifest_entry = if metadata.is_dir() {
            if metadata.uid() != rustix::process::geteuid().as_raw() || mode != 0o755 {
                return Err(CatalogCode::Target);
            }
            collect_static_entries(root, &path, entries, depth + 1, aggregate)?;
            StaticTreeEntry {
                path: relative.clone(),
                kind: StaticTreeKind::Directory,
                mode,
                size: 0,
                sha256: crate::lower_hex(&crate::sha256(&[])),
            }
        } else if metadata.is_file() {
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.nlink() != 1
                || mode != 0o644
                || metadata.len() > 16 * 1024 * 1024
            {
                return Err(CatalogCode::Target);
            }
            let bytes = crate::package::read_source_file(root, &relative, 16 * 1024 * 1024)
                .map_err(|_| CatalogCode::Target)?;
            if bytes.len() as u64 != metadata.len() {
                return Err(CatalogCode::Target);
            }
            *aggregate = aggregate
                .checked_add(metadata.len())
                .filter(|total| *total <= 128 * 1024 * 1024)
                .ok_or(CatalogCode::Target)?;
            StaticTreeEntry {
                path: relative.clone(),
                kind: StaticTreeKind::File,
                mode,
                size: metadata.len(),
                sha256: crate::lower_hex(&crate::sha256(&bytes)),
            }
        } else {
            return Err(CatalogCode::Target);
        };
        if entries.insert(relative, manifest_entry).is_some() || entries.len() > 1_000 {
            return Err(CatalogCode::Target);
        }
    }
    Ok(())
}

fn validate_static_tree_manifest(entries: &[StaticTreeEntry]) -> Result<(), CatalogCode> {
    let empty_digest = crate::lower_hex(&crate::sha256(&[]));
    let mut previous: Option<&str> = None;
    let mut aggregate = 0_u64;
    for entry in entries {
        validate_tree_path(&entry.path)?;
        if entry.path == "marketplace/v1" || entry.path.starts_with("marketplace/v1/") {
            return Err(CatalogCode::Target);
        }
        if previous.is_some_and(|value| value >= entry.path.as_str()) {
            return Err(CatalogCode::Target);
        }
        previous = Some(&entry.path);
        match entry.kind {
            StaticTreeKind::Directory => {
                if entry.mode != 0o755 || entry.size != 0 || entry.sha256 != empty_digest {
                    return Err(CatalogCode::Target);
                }
            }
            StaticTreeKind::File => {
                if entry.mode != 0o644
                    || entry.size > 16 * 1024 * 1024
                    || !valid_digest(&entry.sha256)
                {
                    return Err(CatalogCode::Target);
                }
                aggregate = aggregate
                    .checked_add(entry.size)
                    .filter(|total| *total <= 128 * 1024 * 1024)
                    .ok_or(CatalogCode::Target)?;
            }
        }
    }
    let directories: BTreeSet<_> = entries
        .iter()
        .filter(|entry| entry.kind == StaticTreeKind::Directory)
        .map(|entry| entry.path.as_str())
        .collect();
    for entry in entries {
        if let Some(parent) = Path::new(&entry.path).parent()
            && !parent.as_os_str().is_empty()
            && !directories.contains(parent.to_str().ok_or(CatalogCode::Target)?)
        {
            return Err(CatalogCode::Target);
        }
    }
    Ok(())
}

fn validate_dependencies(inputs: &[&PreparedTarget<'_>]) -> Result<(), CatalogCode> {
    let mut targets = BTreeMap::new();
    let mut identifiers = BTreeSet::new();
    for (index, input) in inputs.iter().enumerate() {
        let manifest = input.package.metadata().manifest();
        if !identifiers.insert(manifest.id()) {
            return Err(CatalogCode::Target);
        }
        targets.insert((manifest.id(), manifest.version()), index);
    }
    let mut edges = vec![Vec::new(); inputs.len()];
    for (index, input) in inputs.iter().enumerate() {
        let manifest = input.package.metadata().manifest();
        for dependency in manifest.dependencies() {
            let dependency_index = *targets
                .get(&(dependency.id(), dependency.version()))
                .ok_or(CatalogCode::Target)?;
            let target = inputs[dependency_index];
            let dependency_manifest = target.package.metadata().manifest();
            if dependency_manifest.kind() != PackageKind::Provider
                || target.status != CatalogStatus::Verified
                || target.package.archive_sha256() != dependency.sha256()
            {
                return Err(CatalogCode::Target);
            }
            edges[index].push(dependency_index);
        }
    }
    let mut color = vec![0u8; inputs.len()];
    for start in 0..inputs.len() {
        if color[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        color[start] = 1;
        while let Some((node, edge)) = stack.pop() {
            if edge == edges[node].len() {
                color[node] = 2;
                continue;
            }
            stack.push((node, edge + 1));
            let next = edges[node][edge];
            if color[next] == 1 {
                return Err(CatalogCode::Target);
            }
            if color[next] == 0 {
                color[next] = 1;
                stack.push((next, 0));
            }
        }
    }
    Ok(())
}

fn parse_time(value: &str) -> Result<chrono::DateTime<chrono::Utc>, CatalogCode> {
    if value.len() != 20 {
        return Err(CatalogCode::Time);
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| CatalogCode::Time)?
        .with_timezone(&chrono::Utc);
    if parsed.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) != value {
        return Err(CatalogCode::Time);
    }
    Ok(parsed)
}

pub(crate) fn sign_payload(
    key_id: &str,
    payload: &[u8],
    seed: &[u8; 32],
) -> Result<Vec<u8>, CatalogCode> {
    if !valid_key_id(key_id) || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CatalogCode::Envelope);
    }
    let key = Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| CatalogCode::Signature)?;
    let envelope = SignedEnvelope {
        schema_version: 1,
        key_id: key_id.to_owned(),
        payload: URL_SAFE_NO_PAD.encode(payload),
        signature: URL_SAFE_NO_PAD.encode(key.sign(payload).as_ref()),
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| CatalogCode::Envelope)?;
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(CatalogCode::Envelope);
    }
    Ok(bytes)
}

pub(crate) fn verify_envelope(
    bytes: &[u8],
    expected_key_id: &str,
    public_key: &[u8; 32],
) -> Result<Vec<u8>, CatalogCode> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(CatalogCode::Envelope);
    }
    let envelope: SignedEnvelope =
        serde_json::from_slice(bytes).map_err(|_| CatalogCode::Envelope)?;
    if envelope.schema_version != 1 || envelope.key_id != expected_key_id {
        return Err(CatalogCode::Envelope);
    }
    let payload = decode_canonical(&envelope.payload, MAX_PAYLOAD_BYTES)?;
    let signature = decode_canonical(&envelope.signature, 64)?;
    if signature.len() != 64 {
        return Err(CatalogCode::Signature);
    }
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload, &signature)
        .map_err(|_| CatalogCode::Signature)?;
    Ok(payload)
}

pub(crate) fn verify_catalog(
    bytes: &[u8],
    expected_key_id: &str,
    public_key: &[u8; 32],
    origin: CatalogOrigin,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), CatalogCode> {
    verify_catalog_payload(bytes, expected_key_id, public_key, origin, now).map(|_| ())
}

fn verify_catalog_payload(
    bytes: &[u8],
    expected_key_id: &str,
    public_key: &[u8; 32],
    origin: CatalogOrigin,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<VerifiedPayload, CatalogCode> {
    if matches!(origin, CatalogOrigin::Production)
        && (expected_key_id == DEVELOPMENT_KEY_ID || public_key == &development_public_key()?)
    {
        return Err(CatalogCode::Signature);
    }
    let payload = verify_envelope(bytes, expected_key_id, public_key)?;
    let payload: VerifiedPayload =
        serde_json::from_slice(&payload).map_err(|_| CatalogCode::Payload)?;
    let generated = parse_time(&payload.generated_at)?;
    let expires = parse_time(&payload.expires_at)?;
    if expires <= generated
        || expires <= now
        || (matches!(origin, CatalogOrigin::Production)
            && expires.signed_duration_since(generated)
                != chrono::TimeDelta::days(PRODUCTION_CATALOG_LIFETIME_DAYS))
        || generated
            > now
                .checked_add_signed(chrono::TimeDelta::minutes(5))
                .unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC)
    {
        return Err(CatalogCode::Time);
    }
    if payload.schema_version != 1
        || payload.sequence == 0
        || (matches!(origin, CatalogOrigin::Production) && payload.sequence > MAX_BROWSER_SEQUENCE)
        || payload.targets.len() > 500
    {
        return Err(CatalogCode::Payload);
    }
    let mut previous = None;
    for target in &payload.targets {
        let manifest_bytes =
            serde_json::to_vec(&target.manifest).map_err(|_| CatalogCode::Payload)?;
        let listing_bytes =
            serde_json::to_vec(&target.listing).map_err(|_| CatalogCode::Payload)?;
        let metadata = crate::metadata::validate_metadata(&manifest_bytes, &listing_bytes)
            .map_err(|_| CatalogCode::Payload)?;
        let manifest = metadata.manifest();
        let identity = (manifest.id().to_owned(), manifest.version().to_owned());
        if previous
            .as_ref()
            .is_some_and(|previous| previous >= &identity)
        {
            return Err(CatalogCode::Target);
        }
        previous = Some(identity);
        if target.package_size == 0
            || target.package_size > 16 * 1024 * 1024
            || !valid_digest(&target.package_sha256)
            || target.min_host_api != 1
            || target.max_host_api != 1
            || target.package_url
                != format!(
                    "{}packages/{}/{}/{}.ocpkg",
                    origin.base_url(),
                    manifest.id(),
                    manifest.version(),
                    target.package_sha256
                )
        {
            return Err(CatalogCode::Target);
        }
        let _status = target.status;
        if let Some(preview) = &target.preview
            && (manifest.kind() == PackageKind::Provider
                || preview.size == 0
                || preview.size > 256 * 1024
                || preview.media_type != "image/png"
                || !valid_digest(&preview.sha256)
                || preview.url
                    != format!(
                        "{}previews/{}/{}/{}.png",
                        origin.base_url(),
                        manifest.id(),
                        manifest.version(),
                        preview.sha256
                    ))
        {
            return Err(CatalogCode::Target);
        }
    }
    Ok(payload)
}

pub(crate) fn accepted_catalog_identity(
    bytes: &[u8],
    public_key: &[u8; 32],
) -> Result<(u64, [u8; 32]), CatalogCode> {
    let verified = verify_catalog_payload(
        bytes,
        PRODUCTION_KEY_ID,
        public_key,
        CatalogOrigin::Production,
        chrono::Utc::now(),
    )?;
    let payload = verify_envelope(bytes, PRODUCTION_KEY_ID, public_key)?;
    Ok((verified.sequence, crate::sha256(&payload)))
}

pub(crate) fn derive_public_key(
    repository: &Path,
    signing_key: &Path,
    key_id: &str,
    output: &Path,
) -> Result<(), CatalogCode> {
    if key_id != PRODUCTION_KEY_ID || !normalized_absolute(output) {
        return Err(CatalogCode::State);
    }
    let seed = read_private_input(repository, signing_key, 65)?;
    let seed = decode_hex_seed(&seed)?;
    if seed == DEV_SEED {
        return Err(CatalogCode::Signature);
    }
    let pair = Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| CatalogCode::Signature)?;
    let bytes = format!("{}\n", crate::lower_hex(pair.public_key().as_ref()));
    write_public_output(output, bytes.as_bytes())
}

pub(crate) fn production_signing_key(
    repository: &Path,
    signing_key: &Path,
) -> Result<[u8; 32], CatalogCode> {
    let seed = read_private_input(repository, signing_key, 65)
        .and_then(|bytes| decode_hex_seed(&bytes))?;
    if seed == DEV_SEED {
        return Err(CatalogCode::Signature);
    }
    let pinned = production_public_key(repository)?;
    let pair = Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| CatalogCode::Signature)?;
    if !bytes_match(pair.public_key().as_ref(), &pinned) {
        return Err(CatalogCode::Signature);
    }
    Ok(seed)
}

fn bytes_match(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

pub(crate) fn production_public_key(repository: &Path) -> Result<[u8; 32], CatalogCode> {
    let pinned = crate::package::read_source_file(repository, PRODUCTION_PUBLIC_KEY_PATH, 65)
        .map_err(|_| CatalogCode::Signature)?;
    decode_hex_seed(&pinned)
}

fn decode_hex_seed(bytes: &[u8]) -> Result<[u8; 32], CatalogCode> {
    let value = bytes.strip_suffix(b"\n").ok_or(CatalogCode::Signature)?;
    if value.len() != 64 {
        return Err(CatalogCode::Signature);
    }
    let mut decoded = [0; 32];
    for (index, pair) in value.as_chunks::<2>().0.iter().enumerate() {
        let digit = |byte| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        decoded[index] = (digit(pair[0]).ok_or(CatalogCode::Signature)? << 4)
            | digit(pair[1]).ok_or(CatalogCode::Signature)?;
    }
    Ok(decoded)
}

fn write_public_output(path: &Path, bytes: &[u8]) -> Result<(), CatalogCode> {
    write_output(path, bytes, 0o644)
}

fn write_private_output(path: &Path, bytes: &[u8]) -> Result<(), CatalogCode> {
    write_output(path, bytes, 0o600)
}

fn write_output(path: &Path, bytes: &[u8], expected_mode: u32) -> Result<(), CatalogCode> {
    let create_mode = match expected_mode {
        0o600 => Mode::RUSR | Mode::WUSR,
        0o644 => Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
        _ => return Err(CatalogCode::State),
    };
    let parent = path.parent().ok_or(CatalogCode::State)?;
    let name = path.file_name().ok_or(CatalogCode::State)?;
    let directory = File::from(
        openat2(
            CWD,
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| CatalogCode::State)?,
    );
    if !crate::owned_directory_is_safe(&directory, false) {
        return Err(CatalogCode::State);
    }
    let descriptor = openat(
        &directory,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        create_mode,
    )
    .map_err(|_| CatalogCode::State)?;
    let mut file = File::from(descriptor);
    let result = (|| {
        fchmod(&file, create_mode).map_err(|_| CatalogCode::State)?;
        let metadata = file.metadata().map_err(|_| CatalogCode::State)?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.nlink() != 1
            || metadata.permissions().mode() & 0o7777 != expected_mode
        {
            return Err(CatalogCode::State);
        }
        file.write_all(bytes).map_err(|_| CatalogCode::State)?;
        file.sync_all().map_err(|_| CatalogCode::State)?;
        fsync(&directory).map_err(|_| CatalogCode::State)
    })();
    if result.is_err() {
        let _ = unlinkat(&directory, name, AtFlags::empty());
    }
    result
}

pub(crate) fn write_verified_tree_ledger(
    repository: &Path,
    tree: &Path,
    catalog_bytes: &[u8],
    public_key: &[u8; 32],
    output: &Path,
) -> Result<(), CatalogCode> {
    verify_published_tree(
        repository,
        tree,
        catalog_bytes,
        PRODUCTION_KEY_ID,
        public_key,
    )?;
    let first = encode_tree_ledger(tree)?;
    verify_published_tree(
        repository,
        tree,
        catalog_bytes,
        PRODUCTION_KEY_ID,
        public_key,
    )?;
    let second = encode_tree_ledger(tree)?;
    if first != second {
        return Err(CatalogCode::Target);
    }
    write_private_output(output, &first)
}

pub(crate) fn verify_tree_ledger(
    tree: &Path,
    ledger: &[u8],
    expected_sha256: &str,
) -> Result<(), CatalogCode> {
    if !valid_digest(expected_sha256)
        || crate::lower_hex(&crate::sha256(ledger)) != expected_sha256
        || encode_tree_ledger(tree)? != ledger
    {
        return Err(CatalogCode::Target);
    }
    Ok(())
}

fn encode_tree_ledger(tree: &Path) -> Result<Vec<u8>, CatalogCode> {
    let root = std::fs::symlink_metadata(tree).map_err(|_| CatalogCode::Target)?;
    if !root.is_dir()
        || root.uid() != rustix::process::geteuid().as_raw()
        || root.permissions().mode() & 0o7777 != 0o755
    {
        return Err(CatalogCode::Target);
    }
    let mut entries = BTreeMap::new();
    walk_public_tree(
        tree,
        tree,
        &mut entries,
        0,
        &mut 0_u64,
        TreePermissions::Public,
    )?;
    let mut bytes = Vec::new();
    append_ledger_entry(
        &mut bytes,
        ".",
        StaticTreeKind::Directory,
        &root,
        &crate::sha256(&[]),
    )?;
    for (relative, entry) in entries {
        let metadata =
            std::fs::symlink_metadata(tree.join(&relative)).map_err(|_| CatalogCode::Target)?;
        let digest = decode_hex_digest(&entry.sha256)?;
        append_ledger_entry(&mut bytes, &relative, entry.kind, &metadata, &digest)?;
    }
    if bytes.len() > 1024 * 1024 {
        return Err(CatalogCode::Target);
    }
    Ok(bytes)
}

fn append_ledger_entry(
    output: &mut Vec<u8>,
    path: &str,
    kind: StaticTreeKind,
    metadata: &std::fs::Metadata,
    digest: &[u8; 32],
) -> Result<(), CatalogCode> {
    use std::fmt::Write as _;
    let kind = match kind {
        StaticTreeKind::Directory => "directory",
        StaticTreeKind::File => "file",
    };
    let size = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    let mut line = String::new();
    writeln!(
        line,
        "{kind}\t{path}\t{}\t{}\t{:o}\t{}\t{size}\t{}",
        metadata.uid(),
        metadata.gid(),
        metadata.permissions().mode() & 0o7777,
        metadata.nlink(),
        crate::lower_hex(digest),
    )
    .map_err(|_| CatalogCode::Target)?;
    output
        .try_reserve(line.len())
        .map_err(|_| CatalogCode::Target)?;
    output.extend_from_slice(line.as_bytes());
    Ok(())
}

fn decode_hex_digest(value: &str) -> Result<[u8; 32], CatalogCode> {
    if !valid_digest(value) {
        return Err(CatalogCode::Target);
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |byte| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        output[index] = (digit(pair[0]).ok_or(CatalogCode::Target)? << 4)
            | digit(pair[1]).ok_or(CatalogCode::Target)?;
    }
    Ok(output)
}

pub(crate) fn verify_published_tree(
    repository: &Path,
    tree: &Path,
    catalog_bytes: &[u8],
    key_id: &str,
    public_key: &[u8; 32],
) -> Result<(), CatalogCode> {
    verify_published_tree_with_permissions(
        repository,
        tree,
        catalog_bytes,
        key_id,
        public_key,
        TreePermissions::Public,
    )
}

pub(crate) fn verify_release_snapshot_tree(
    repository: &Path,
    tree: &Path,
    catalog_bytes: &[u8],
    key_id: &str,
    public_key: &[u8; 32],
) -> Result<(), CatalogCode> {
    let metadata = std::fs::symlink_metadata(tree).map_err(|_| CatalogCode::Target)?;
    let permissions = match metadata.permissions().mode() & 0o7777 {
        0o755 => TreePermissions::Public,
        0o700 => TreePermissions::PrivateSnapshot,
        _ => return Err(CatalogCode::Target),
    };
    verify_published_tree_with_permissions(
        repository,
        tree,
        catalog_bytes,
        key_id,
        public_key,
        permissions,
    )
}

#[derive(Clone, Copy)]
enum TreePermissions {
    Public,
    PrivateSnapshot,
}

impl TreePermissions {
    const fn directory_mode(self) -> u32 {
        match self {
            Self::Public => 0o755,
            Self::PrivateSnapshot => 0o700,
        }
    }

    const fn file_mode(self) -> u32 {
        match self {
            Self::Public => 0o644,
            Self::PrivateSnapshot => 0o600,
        }
    }
}

fn verify_published_tree_with_permissions(
    repository: &Path,
    tree: &Path,
    catalog_bytes: &[u8],
    key_id: &str,
    public_key: &[u8; 32],
    permissions: TreePermissions,
) -> Result<(), CatalogCode> {
    if key_id != PRODUCTION_KEY_ID || !safe_tree_location(repository, tree) {
        return Err(CatalogCode::Target);
    }
    let payload = verify_catalog_payload(
        catalog_bytes,
        key_id,
        public_key,
        CatalogOrigin::Production,
        chrono::Utc::now(),
    )?;
    validate_static_tree_manifest(&payload.static_tree)?;
    let mut expected: BTreeMap<String, StaticTreeEntry> = payload
        .static_tree
        .iter()
        .cloned()
        .map(|entry| (entry.path.clone(), entry))
        .collect();
    add_expected_file(
        &mut expected,
        "marketplace/v1/catalog.json",
        catalog_bytes.len() as u64,
        crate::lower_hex(&crate::sha256(catalog_bytes)),
    )?;
    add_expected_directory(&mut expected, "marketplace/v1/packages")?;
    add_expected_directory(&mut expected, "marketplace/v1/previews")?;
    for target in &payload.targets {
        let package = format!(
            "marketplace/v1/packages/{}/{}/{}.ocpkg",
            target.manifest.id(),
            target.manifest.version(),
            target.package_sha256
        );
        add_expected_file(
            &mut expected,
            &package,
            target.package_size,
            target.package_sha256.clone(),
        )?;
        if let Some(preview) = &target.preview {
            let path = format!(
                "marketplace/v1/previews/{}/{}/{}.png",
                target.manifest.id(),
                target.manifest.version(),
                preview.sha256
            );
            add_expected_file(&mut expected, &path, preview.size, preview.sha256.clone())?;
        }
    }
    if expected.len() > 1_000 {
        return Err(CatalogCode::Target);
    }
    let root_metadata = std::fs::symlink_metadata(tree).map_err(|_| CatalogCode::Target)?;
    if !root_metadata.is_dir()
        || root_metadata.uid() != rustix::process::geteuid().as_raw()
        || root_metadata.permissions().mode() & 0o7777 != permissions.directory_mode()
    {
        return Err(CatalogCode::Target);
    }
    let mut actual = BTreeMap::new();
    walk_public_tree(tree, tree, &mut actual, 0, &mut 0_u64, permissions)?;
    if actual != expected {
        return Err(CatalogCode::Target);
    }
    Ok(())
}

fn add_expected_file(
    expected: &mut BTreeMap<String, StaticTreeEntry>,
    path: &str,
    size: u64,
    sha256: String,
) -> Result<(), CatalogCode> {
    validate_tree_path(path)?;
    if size > 16 * 1024 * 1024 || !valid_digest(&sha256) {
        return Err(CatalogCode::Target);
    }
    let file_path = path.to_owned();
    let mut parent = Path::new(&file_path).parent().map(Path::to_path_buf);
    while let Some(directory_path) = parent {
        let directory = directory_path.as_path();
        if directory.as_os_str().is_empty() {
            break;
        }
        let directory = directory.to_str().ok_or(CatalogCode::Target)?.to_owned();
        add_expected_directory(expected, &directory)?;
        parent = directory_path.parent().map(Path::to_path_buf);
    }
    let entry = StaticTreeEntry {
        path: file_path.clone(),
        kind: StaticTreeKind::File,
        mode: 0o644,
        size,
        sha256,
    };
    if expected.insert(file_path, entry).is_some() {
        return Err(CatalogCode::Target);
    }
    Ok(())
}

fn add_expected_directory(
    expected: &mut BTreeMap<String, StaticTreeEntry>,
    path: &str,
) -> Result<(), CatalogCode> {
    validate_tree_path(path)?;
    let entry = StaticTreeEntry {
        path: path.to_owned(),
        kind: StaticTreeKind::Directory,
        mode: 0o755,
        size: 0,
        sha256: crate::lower_hex(&crate::sha256(&[])),
    };
    if let Some(existing) = expected.get(path)
        && existing != &entry
    {
        return Err(CatalogCode::Target);
    }
    expected.entry(path.to_owned()).or_insert(entry);
    Ok(())
}

fn safe_tree_location(repository: &Path, tree: &Path) -> bool {
    if !normalized_absolute(repository) || !normalized_absolute(tree) {
        return false;
    }
    let Ok(repository_canonical) = std::fs::canonicalize(repository) else {
        return false;
    };
    let Ok(tree_canonical) = std::fs::canonicalize(tree) else {
        return false;
    };
    if repository_canonical != repository || tree_canonical != tree {
        return false;
    }
    if File::from(
        match openat2(
            CWD,
            tree,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        ) {
            Ok(descriptor) => descriptor,
            Err(_) => return false,
        },
    )
    .metadata()
    .is_err()
    {
        return false;
    }
    if tree == repository.join("published") || tree == repository.join("public") {
        return true;
    }
    let Ok(relative) = tree.strip_prefix(repository) else {
        return false;
    };
    let components: Vec<_> = relative.iter().collect();
    components.len() == 3
        && components[0]
            .to_str()
            .is_some_and(|value| value.starts_with(".build-production.") && value.len() <= 64)
        && components[1] == OsStr::new("repository")
        && components[2] == OsStr::new("public")
}

fn walk_public_tree(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, StaticTreeEntry>,
    depth: usize,
    aggregate: &mut u64,
    permissions: TreePermissions,
) -> Result<(), CatalogCode> {
    if depth > 16 || entries.len() > 1_000 {
        return Err(CatalogCode::Target);
    }
    for entry in std::fs::read_dir(directory).map_err(|_| CatalogCode::Target)? {
        let entry = entry.map_err(|_| CatalogCode::Target)?;
        let path = entry.path();
        let relative = normalized_tree_path(root, &path)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| CatalogCode::Target)?;
        let mode = metadata.permissions().mode() & 0o7777;
        let observed = if metadata.is_dir() {
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || mode != permissions.directory_mode()
            {
                return Err(CatalogCode::Target);
            }
            walk_public_tree(root, &path, entries, depth + 1, aggregate, permissions)?;
            StaticTreeEntry {
                path: relative.clone(),
                kind: StaticTreeKind::Directory,
                mode: 0o755,
                size: 0,
                sha256: crate::lower_hex(&crate::sha256(&[])),
            }
        } else if metadata.is_file() {
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.nlink() != 1
                || mode != permissions.file_mode()
                || metadata.len() > 16 * 1024 * 1024
            {
                return Err(CatalogCode::Target);
            }
            *aggregate = aggregate
                .checked_add(metadata.len())
                .filter(|total| *total <= 128 * 1024 * 1024)
                .ok_or(CatalogCode::Target)?;
            let bytes = crate::package::read_source_file(root, &relative, 16 * 1024 * 1024)
                .map_err(|_| CatalogCode::Target)?;
            if bytes.len() as u64 != metadata.len() {
                return Err(CatalogCode::Target);
            }
            StaticTreeEntry {
                path: relative.clone(),
                kind: StaticTreeKind::File,
                mode: 0o644,
                size: metadata.len(),
                sha256: crate::lower_hex(&crate::sha256(&bytes)),
            }
        } else {
            return Err(CatalogCode::Target);
        };
        if entries.insert(relative, observed).is_some() || entries.len() > 1_000 {
            return Err(CatalogCode::Target);
        }
    }
    Ok(())
}

fn normalized_tree_path(root: &Path, path: &Path) -> Result<String, CatalogCode> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CatalogCode::Target)?
        .to_str()
        .ok_or(CatalogCode::Target)?
        .to_owned();
    validate_tree_path(&relative)?;
    Ok(relative)
}

fn validate_tree_path(relative: &str) -> Result<(), CatalogCode> {
    if relative.is_empty()
        || relative.len() > 240
        || !relative
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        || relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        Err(CatalogCode::Target)
    } else {
        Ok(())
    }
}

fn development_public_key() -> Result<[u8; 32], CatalogCode> {
    let pair =
        Ed25519KeyPair::from_seed_unchecked(&DEV_SEED).map_err(|_| CatalogCode::Signature)?;
    let mut public = [0; 32];
    public.copy_from_slice(ring::signature::KeyPair::public_key(&pair).as_ref());
    Ok(public)
}

fn decode_canonical(value: &str, maximum: usize) -> Result<Vec<u8>, CatalogCode> {
    if value.len() > maximum.saturating_mul(4).saturating_add(2) / 3 {
        return Err(CatalogCode::Envelope);
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CatalogCode::Envelope)?;
    if decoded.len() > maximum || URL_SAFE_NO_PAD.encode(&decoded) != value {
        return Err(CatalogCode::Envelope);
    }
    Ok(decoded)
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::{fs::Permissions, os::unix::fs::PermissionsExt as _};

    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::{Value, json};

    use super::{
        CatalogCode, CatalogOrigin, DEV_SEED, DEVELOPMENT_KEY_ID, PreparedTarget, PublisherState,
        SequenceState, build_catalog, parse_counter, read_private_input, sign_payload,
        validate_dependencies, verify_catalog, verify_envelope,
    };
    use crate::{
        metadata::{CatalogStatus, validate_metadata},
        package::PackageArtifact,
    };

    const MANIFEST: &[u8] = include_bytes!("../../../examples/hello-widget/manifest.json");
    const LISTING: &[u8] = br#"{
        "author":"PlayerVox",
        "spdxLicense":"AGPL-3.0-only",
        "sourceUrl":"https://github.com/PlayerVox/playervox-overcrow-marketplace",
        "localizations":[
            {"locale":"en","name":"Hello","description":"Safe"},
            {"locale":"fr","name":"Bonjour","description":"Safe French"}
        ],
        "previewFile":"preview.png"
    }"#;
    const NOW: &str = "2026-08-26T00:00:00Z";

    fn development_public_key() -> [u8; 32] {
        let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&DEV_SEED)
            .expect("development key");
        let mut public = [0; 32];
        public.copy_from_slice(ring::signature::KeyPair::public_key(&pair).as_ref());
        public
    }

    fn verify_at(envelope: &[u8], key_id: &str, origin: CatalogOrigin) -> Result<(), CatalogCode> {
        verify_catalog(
            envelope,
            key_id,
            &development_public_key(),
            origin,
            NOW.parse().expect("test time"),
        )
    }

    #[test]
    fn publisher_state_is_atomic_and_enforces_sequence_before_replacement() {
        let temporary = tempfile::tempdir().expect("state directory");
        let state_path = temporary.path().join("catalog-state.json");
        let digest = [7; 32];
        {
            let mut state = PublisherState::development(&state_path).expect("state lock");
            state.accept(3, digest).expect("initial state");
        }
        let initial = std::fs::read(&state_path).expect("state bytes");
        assert_eq!(
            initial,
            b"{\"schemaVersion\":1,\"sequence\":3,\"payloadSha256\":\"0707070707070707070707070707070707070707070707070707070707070707\"}\n"
        );
        std::fs::set_permissions(&state_path, Permissions::from_mode(0o666))
            .expect("unsafe development state mode");
        assert!(PublisherState::development(&state_path).is_err());
        std::fs::set_permissions(&state_path, Permissions::from_mode(0o644))
            .expect("restore development state mode");
        {
            let mut state = PublisherState::development(&state_path).expect("state lock");
            state.accept(3, digest).expect("deterministic retry");
            assert_eq!(state.accept(3, [8; 32]), Err(CatalogCode::SequenceConflict));
        }
        assert_eq!(
            std::fs::read(&state_path).expect("unchanged state"),
            initial
        );
        let mut state = PublisherState::development(&state_path).expect("state lock");
        assert_eq!(state.accept(2, digest), Err(CatalogCode::Rollback));
        state.accept(4, [8; 32]).expect("strict advance");
    }

    #[test]
    fn production_state_requires_private_files_outside_repository() {
        const BROWSER_SAFE_MAXIMUM: u64 = 9_007_199_254_740_991;
        let repository = tempfile::tempdir().expect("repository");
        let secrets = tempfile::tempdir().expect("secrets");
        std::fs::set_permissions(secrets.path(), Permissions::from_mode(0o700))
            .expect("private secrets directory");
        let state_path = secrets.path().join("state.json");
        let input_path = secrets.path().join("counter.txt");
        std::fs::write(
            &state_path,
            b"{\"schemaVersion\":1,\"sequence\":1,\"payloadSha256\":\"0000000000000000000000000000000000000000000000000000000000000000\"}\n",
        )
        .expect("state fixture");
        std::fs::set_permissions(&state_path, Permissions::from_mode(0o600))
            .expect("private state");
        std::fs::write(&input_path, b"1\n").expect("private input fixture");
        std::fs::set_permissions(&input_path, Permissions::from_mode(0o600))
            .expect("private input");
        PublisherState::production(repository.path(), &state_path).expect("private state");
        assert_eq!(
            read_private_input(repository.path(), &input_path, 32).expect("private input"),
            b"1\n"
        );
        let digest = [7; 32];
        let mut state = PublisherState::production(repository.path(), &state_path)
            .expect("private state at browser-safe sequence boundary");
        state
            .accept(BROWSER_SAFE_MAXIMUM, digest)
            .expect("accept browser-safe maximum");
        std::fs::write(&input_path, format!("{BROWSER_SAFE_MAXIMUM}\n"))
            .expect("maximum browser-safe input");
        assert_eq!(
            state.advance_counter(&input_path, BROWSER_SAFE_MAXIMUM, digest),
            Err(CatalogCode::Counter)
        );
        drop(state);

        std::fs::set_permissions(secrets.path(), Permissions::from_mode(0o770))
            .expect("unsafe secrets directory");
        assert!(PublisherState::production(repository.path(), &state_path).is_err());
        assert!(read_private_input(repository.path(), &input_path, 32).is_err());
        std::fs::set_permissions(secrets.path(), Permissions::from_mode(0o700))
            .expect("restore secrets directory");

        std::fs::set_permissions(&state_path, Permissions::from_mode(0o644)).expect("public state");
        assert!(PublisherState::production(repository.path(), &state_path).is_err());

        let in_repository = repository.path().join("state.json");
        std::fs::write(&in_repository, b"state").expect("repository state fixture");
        std::fs::set_permissions(&in_repository, Permissions::from_mode(0o600))
            .expect("repository state mode");
        assert!(PublisherState::production(repository.path(), &in_repository).is_err());
    }

    #[test]
    fn signed_envelope_is_exact_deterministic_and_canonical() {
        let payload = br#"{"schemaVersion":1,"sequence":7}"#;
        let first = sign_payload(DEVELOPMENT_KEY_ID, payload, &DEV_SEED).expect("signed catalog");
        let second = sign_payload(DEVELOPMENT_KEY_ID, payload, &DEV_SEED).expect("signed catalog");
        assert_eq!(first, second);

        let envelope: Value = serde_json::from_slice(&first).expect("envelope JSON");
        assert_eq!(
            envelope
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>(),
            ["keyId", "payload", "schemaVersion", "signature"]
        );
        assert_eq!(envelope["schemaVersion"], 1);
        assert_eq!(
            URL_SAFE_NO_PAD.decode(envelope["payload"].as_str().expect("payload")),
            Ok(payload.to_vec())
        );
        verify_envelope(&first, DEVELOPMENT_KEY_ID, &development_public_key())
            .expect("valid signature");

        let mut tampered = first;
        let index = tampered
            .iter()
            .position(|byte| *byte == b'7')
            .expect("payload digit");
        tampered[index] = b'8';
        assert_eq!(
            verify_envelope(&tampered, DEVELOPMENT_KEY_ID, &development_public_key()),
            Err(CatalogCode::Signature)
        );

        let wrong_shape = sign_payload(DEVELOPMENT_KEY_ID, b"{}", &DEV_SEED).expect("signed JSON");
        assert_eq!(
            verify_catalog(
                &wrong_shape,
                DEVELOPMENT_KEY_ID,
                &development_public_key(),
                CatalogOrigin::Development,
                NOW.parse().expect("test time"),
            ),
            Err(CatalogCode::Payload)
        );
    }

    #[test]
    fn strict_counter_rejects_ambiguous_values() {
        assert_eq!(parse_counter(b"1\n"), Ok(1));
        assert_eq!(parse_counter(b"18446744073709551615\n"), Ok(u64::MAX));
        for value in [b"0\n".as_slice(), b"01\n", b"1", b" 1\n", b"1\n\n"] {
            assert_eq!(parse_counter(value), Err(CatalogCode::Counter));
        }
    }

    #[test]
    fn state_json_is_narrow_and_strict() {
        let state = SequenceState::new(3, [0xab; 32]).expect("state");
        let value = serde_json::to_value(&state).expect("state JSON");
        assert_eq!(
            value,
            json!({
                "schemaVersion": 1,
                "sequence": 3,
                "payloadSha256": "abababababababababababababababababababababababababababababababab"
            })
        );
        let mut unknown = value;
        unknown["path"] = json!("secret");
        assert!(serde_json::from_value::<SequenceState>(unknown).is_err());
    }

    #[test]
    fn catalog_target_is_exact_and_listing_never_leaks_build_fields() {
        let metadata = validate_metadata(MANIFEST, LISTING).expect("metadata");
        let artifact =
            PackageArtifact::fixture(metadata, b"archive".to_vec(), Some(b"png".to_vec()));
        let targets = [PreparedTarget {
            package: &artifact,
            status: CatalogStatus::Verified,
        }];
        let built = build_catalog(
            &targets,
            7,
            "2026-08-25T00:00:00Z",
            "2036-08-25T00:00:00Z",
            CatalogOrigin::Development,
            DEVELOPMENT_KEY_ID,
            &DEV_SEED,
        )
        .expect("catalog");
        let payload: Value = serde_json::from_slice(&built.payload).expect("payload JSON");
        let target = &payload["targets"][0];
        assert_eq!(
            target
                .as_object()
                .expect("target")
                .keys()
                .collect::<Vec<_>>(),
            [
                "listing",
                "manifest",
                "maxHostApi",
                "minHostApi",
                "packageSha256",
                "packageSize",
                "packageUrl",
                "preview",
                "status"
            ]
        );
        assert_eq!(
            target["listing"]
                .as_object()
                .expect("listing")
                .keys()
                .collect::<Vec<_>>(),
            ["author", "localizations", "sourceUrl", "spdxLicense"]
        );
        assert!(target["manifest"].get("capabilities").is_some());
        assert!(target["listing"].get("previewFile").is_none());
        assert_eq!(
            target["packageUrl"],
            format!(
                "http://127.0.0.1:8787/marketplace/v1/packages/com.playervox.overcrow.example.hello/0.1.0/{}.ocpkg",
                artifact.archive_sha256()
            )
        );
        assert_eq!(
            built.envelope,
            sign_payload(DEVELOPMENT_KEY_ID, &built.payload, &DEV_SEED).expect("signature")
        );
        verify_at(
            &built.envelope,
            DEVELOPMENT_KEY_ID,
            CatalogOrigin::Development,
        )
        .expect("strict catalog verification");

        let payload: Value = serde_json::from_slice(&built.payload).expect("catalog payload");
        for (generated, expires) in [
            ("2021-08-25T00:00:00Z", "2022-08-25T00:00:00Z"),
            ("2026-08-26T00:05:01Z", "2036-08-25T00:00:00Z"),
        ] {
            let mut invalid = payload.clone();
            invalid["generatedAt"] = generated.into();
            invalid["expiresAt"] = expires.into();
            let payload = serde_json::to_vec(&invalid).expect("invalid time payload");
            let envelope = sign_payload(DEVELOPMENT_KEY_ID, &payload, &DEV_SEED).expect("resign");
            assert_eq!(
                verify_at(&envelope, DEVELOPMENT_KEY_ID, CatalogOrigin::Development),
                Err(CatalogCode::Time)
            );
        }

        let development_key_in_production = build_catalog(
            &targets,
            10,
            "2026-08-25T00:00:00Z",
            "2026-11-23T00:00:00Z",
            CatalogOrigin::Production,
            "production-alias",
            &DEV_SEED,
        )
        .expect("production URL fixture signed by development key");
        for key_id in ["production-alias", DEVELOPMENT_KEY_ID] {
            assert_eq!(
                verify_at(
                    &development_key_in_production.envelope,
                    key_id,
                    CatalogOrigin::Production,
                ),
                Err(CatalogCode::Signature)
            );
        }
    }

    fn artifact(manifest: Value, listing: &[u8], archive: &[u8]) -> PackageArtifact {
        PackageArtifact::fixture(
            validate_metadata(
                &serde_json::to_vec(&manifest).expect("manifest JSON"),
                listing,
            )
            .expect("valid fixture metadata"),
            archive.to_vec(),
            None,
        )
    }

    fn provider() -> PackageArtifact {
        artifact(
            serde_json::from_slice(include_bytes!(
                "../../../providers/warframe-worldstate/manifest.json"
            ))
            .expect("provider manifest"),
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"provider archive",
        )
    }

    fn dependency(id: &str, version: &str, sha256: &str) -> Value {
        json!({"id": id, "version": version, "sha256": sha256})
    }

    fn status_widget(dependencies: Value) -> PackageArtifact {
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../widgets/warframe-status/manifest.json"
        ))
        .expect("status manifest");
        manifest["dependencies"] = dependencies;
        artifact(
            manifest,
            include_bytes!("../../../widgets/warframe-status/listing.json"),
            b"widget archive",
        )
    }

    fn validate_widget_dependency(
        target: &PackageArtifact,
        status: CatalogStatus,
        dependency: Value,
    ) -> Result<(), CatalogCode> {
        let widget = status_widget(json!([dependency]));
        validate_dependencies(&[
            &PreparedTarget {
                package: target,
                status,
            },
            &PreparedTarget {
                package: &widget,
                status: CatalogStatus::Verified,
            },
        ])
    }

    #[test]
    fn dependency_graph_accepts_a_verified_provider_archive_binding() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::Verified,
                dependency(
                    provider.metadata().manifest().id(),
                    provider.metadata().manifest().version(),
                    provider.archive_sha256(),
                ),
            ),
            Ok(())
        );
    }

    #[test]
    fn dependency_graph_rejects_a_dangling_provider_id() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::Verified,
                dependency(
                    "com.playervox.overcrow.missing",
                    provider.metadata().manifest().version(),
                    provider.archive_sha256(),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_wrong_provider_version() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::Verified,
                dependency(
                    provider.metadata().manifest().id(),
                    "9.9.9",
                    provider.archive_sha256(),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_wrong_provider_archive_hash() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::Verified,
                dependency(
                    provider.metadata().manifest().id(),
                    provider.metadata().manifest().version(),
                    &"0".repeat(64),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_revoked_provider() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::Revoked,
                dependency(
                    provider.metadata().manifest().id(),
                    provider.metadata().manifest().version(),
                    provider.archive_sha256(),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_security_suspended_provider() {
        let provider = provider();
        assert_eq!(
            validate_widget_dependency(
                &provider,
                CatalogStatus::SecuritySuspended,
                dependency(
                    provider.metadata().manifest().id(),
                    provider.metadata().manifest().version(),
                    provider.archive_sha256(),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_visible_non_provider_target() {
        let target = artifact(
            serde_json::from_slice(include_bytes!(
                "../../../widgets/warframe-market/manifest.json"
            ))
            .expect("market manifest"),
            include_bytes!("../../../widgets/warframe-market/listing.json"),
            b"market archive",
        );
        assert_eq!(
            validate_widget_dependency(
                &target,
                CatalogStatus::Verified,
                dependency(
                    target.metadata().manifest().id(),
                    target.metadata().manifest().version(),
                    target.archive_sha256(),
                ),
            ),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_self_dependency() {
        let original = provider();
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../providers/warframe-worldstate/manifest.json"
        ))
        .expect("provider manifest");
        manifest["dependencies"] = json!([dependency(
            original.metadata().manifest().id(),
            original.metadata().manifest().version(),
            original.archive_sha256(),
        )]);
        let provider = artifact(
            manifest,
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"provider archive",
        );
        assert_eq!(
            validate_dependencies(&[&PreparedTarget {
                package: &provider,
                status: CatalogStatus::Verified,
            }]),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_a_two_provider_cycle() {
        let provider_a = provider();
        let mut provider_b_manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../providers/warframe-worldstate/manifest.json"
        ))
        .expect("provider manifest");
        provider_b_manifest["id"] = json!("com.playervox.overcrow.warframe.worldstate.backup");
        let provider_b = artifact(
            provider_b_manifest.clone(),
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"backup provider archive",
        );

        let mut provider_a_manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../providers/warframe-worldstate/manifest.json"
        ))
        .expect("provider manifest");
        provider_a_manifest["dependencies"] = json!([dependency(
            provider_b.metadata().manifest().id(),
            provider_b.metadata().manifest().version(),
            provider_b.archive_sha256(),
        )]);
        provider_b_manifest["dependencies"] = json!([dependency(
            provider_a.metadata().manifest().id(),
            provider_a.metadata().manifest().version(),
            provider_a.archive_sha256(),
        )]);
        let provider_a = artifact(
            provider_a_manifest,
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"provider archive",
        );
        let provider_b = artifact(
            provider_b_manifest,
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"backup provider archive",
        );
        assert_eq!(
            validate_dependencies(&[
                &PreparedTarget {
                    package: &provider_a,
                    status: CatalogStatus::Verified,
                },
                &PreparedTarget {
                    package: &provider_b,
                    status: CatalogStatus::Verified,
                },
            ]),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_an_exact_duplicate_identity() {
        let provider = provider();
        let inputs = [
            PreparedTarget {
                package: &provider,
                status: CatalogStatus::Verified,
            },
            PreparedTarget {
                package: &provider,
                status: CatalogStatus::Verified,
            },
        ];
        assert_eq!(
            build_catalog(
                &inputs,
                2,
                NOW,
                "2036-08-26T00:00:00Z",
                CatalogOrigin::Development,
                DEVELOPMENT_KEY_ID,
                &DEV_SEED,
            )
            .map(|_| ()),
            Err(CatalogCode::Target)
        );
    }

    #[test]
    fn dependency_graph_rejects_an_ambiguous_multi_version_identity() {
        let provider_a = provider();
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../providers/warframe-worldstate/manifest.json"
        ))
        .expect("provider manifest");
        manifest["version"] = json!("2.0.0");
        let provider_b = artifact(
            manifest,
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
            b"provider version two archive",
        );
        let inputs = [
            PreparedTarget {
                package: &provider_a,
                status: CatalogStatus::Verified,
            },
            PreparedTarget {
                package: &provider_b,
                status: CatalogStatus::Verified,
            },
        ];
        assert_eq!(
            build_catalog(
                &inputs,
                2,
                NOW,
                "2036-08-26T00:00:00Z",
                CatalogOrigin::Development,
                DEVELOPMENT_KEY_ID,
                &DEV_SEED,
            )
            .map(|_| ()),
            Err(CatalogCode::Target)
        );
    }
}
