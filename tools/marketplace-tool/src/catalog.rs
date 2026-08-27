use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read as _, Write as _},
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _, fs::PermissionsExt as _},
    path::Path,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ED25519, Ed25519KeyPair, UnparsedPublicKey};
use rustix::fs::{
    AtFlags, CWD, FlockOperation, Mode, OFlags, ResolveFlags, flock, fsync, openat, openat2,
    renameat, unlinkat,
};
use serde::{Deserialize, Serialize};

use crate::{
    metadata::{CatalogStatus, Localization, Manifest, PackageKind},
    package::PackageArtifact,
};

pub(crate) const DEVELOPMENT_KEY_ID: &str = "overcrow-development-2026";
pub(crate) const DEV_SEED: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];
const MAX_PAYLOAD_BYTES: usize = 700 * 1024;
const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;

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
    targets: Vec<CatalogTarget<'a>>,
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
    targets: Vec<VerifiedTarget>,
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
    if sequence == 0 || inputs.len() > 500 {
        return Err(CatalogCode::Target);
    }
    let generated = parse_time(generated_at)?;
    let expires = parse_time(expires_at)?;
    if expires <= generated {
        return Err(CatalogCode::Time);
    }
    let mut inputs: Vec<_> = inputs.iter().collect();
    inputs.sort_by(|left, right| {
        let left = left.package.metadata().manifest();
        let right = right.package.metadata().manifest();
        (left.id(), left.version()).cmp(&(right.id(), right.version()))
    });
    if inputs.windows(2).any(|pair| {
        let left = pair[0].package.metadata().manifest();
        let right = pair[1].package.metadata().manifest();
        left.id() == right.id() && left.version() == right.version()
    }) {
        return Err(CatalogCode::Target);
    }
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
        targets,
    })
    .map_err(|_| CatalogCode::Payload)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CatalogCode::Payload);
    }
    let envelope = sign_payload(key_id, &payload, seed)?;
    Ok(CatalogBuild { payload, envelope })
}

fn validate_dependencies(inputs: &[&PreparedTarget<'_>]) -> Result<(), CatalogCode> {
    let mut targets = BTreeMap::new();
    let mut identifiers = BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        let manifest = input.package.metadata().manifest();
        if targets
            .insert((manifest.id(), manifest.version()), index)
            .is_some()
            || identifiers.insert(manifest.id(), index).is_some()
        {
            return Err(CatalogCode::Target);
        }
    }
    let mut edges = vec![Vec::new(); inputs.len()];
    for (index, input) in inputs.iter().enumerate() {
        let manifest = input.package.metadata().manifest();
        for dependency in manifest.dependencies() {
            let dependency_index = *targets
                .get(&(dependency.id(), dependency.version()))
                .ok_or(CatalogCode::Target)?;
            if dependency_index == index {
                return Err(CatalogCode::Target);
            }
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
        if color[start] != 0 { continue; }
        let mut stack = vec![(start, 0usize)];
        color[start] = 1;
        while let Some((node, edge)) = stack.pop() {
            if edge == edges[node].len() { color[node] = 2; continue; }
            stack.push((node, edge + 1));
            let next = edges[node][edge];
            if color[next] == 1 { return Err(CatalogCode::Target); }
            if color[next] == 0 { color[next] = 1; stack.push((next, 0)); }
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
        || generated
            > now
                .checked_add_signed(chrono::TimeDelta::minutes(5))
                .unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC)
    {
        return Err(CatalogCode::Time);
    }
    if payload.schema_version != 1 || payload.sequence == 0 || payload.targets.len() > 500 {
        return Err(CatalogCode::Payload);
    }
    let mut previous = None;
    for target in payload.targets {
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
        if let Some(preview) = target.preview
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
    Ok(())
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
        verify_catalog, verify_envelope,
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
        "localizations":[{"locale":"en","name":"Hello","description":"Safe"}],
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
            "2036-08-25T00:00:00Z",
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

    #[test]
    fn catalog_rejects_dependencies_that_do_not_bind_a_verified_provider_archive() {
        let provider = validate_metadata(
            include_bytes!("../../../providers/warframe-worldstate/manifest.json"),
            include_bytes!("../../../providers/warframe-worldstate/listing.json"),
        )
        .expect("provider metadata");
        let provider = PackageArtifact::fixture(provider, b"provider archive".to_vec(), None);
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../widgets/warframe-status/manifest.json"
        ))
        .expect("status manifest");
        manifest["dependencies"][0]["sha256"] = json!(provider.archive_sha256());
        let manifest = serde_json::to_vec(&manifest).expect("status manifest JSON");
        let widget = validate_metadata(
            &manifest,
            include_bytes!("../../../widgets/warframe-status/listing.json"),
        )
        .expect("widget metadata");
        let widget = PackageArtifact::fixture(widget, b"widget archive".to_vec(), None);
        let inputs = [
            PreparedTarget {
                package: &provider,
                status: CatalogStatus::Verified,
            },
            PreparedTarget {
                package: &widget,
                status: CatalogStatus::Verified,
            },
        ];
        assert!(
            build_catalog(
                &inputs,
                2,
                NOW,
                "2036-08-26T00:00:00Z",
                CatalogOrigin::Development,
                DEVELOPMENT_KEY_ID,
                &DEV_SEED
            )
            .is_ok()
        );

        let mut wrong_manifest: Value = serde_json::from_slice(&manifest).expect("status manifest");
        wrong_manifest["dependencies"][0]["sha256"] = json!("0".repeat(64));
        let wrong_manifest = serde_json::to_vec(&wrong_manifest).expect("wrong manifest JSON");
        let wrong = validate_metadata(
            &wrong_manifest,
            include_bytes!("../../../widgets/warframe-status/listing.json"),
        )
        .expect("wrong metadata remains structurally valid");
        let wrong = PackageArtifact::fixture(wrong, b"widget archive".to_vec(), None);
        let inputs = [
            PreparedTarget {
                package: &provider,
                status: CatalogStatus::Verified,
            },
            PreparedTarget {
                package: &wrong,
                status: CatalogStatus::Verified,
            },
        ];
        assert!(
            build_catalog(
                &inputs,
                2,
                NOW,
                "2036-08-26T00:00:00Z",
                CatalogOrigin::Development,
                DEVELOPMENT_KEY_ID,
                &DEV_SEED
            )
            .is_err()
        );
    }
}
