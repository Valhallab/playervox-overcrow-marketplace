use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};

const UTF8_FLAG: u16 = 1 << 11;
const DOS_DATE_1980_01_01: u16 = 33;
const REGULAR_MODE: u32 = 0o100644;
const MAX_FILES: usize = 4096;
pub(crate) const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_LISTING_BYTES: usize = 64 * 1024;
const MAX_FILE_PATH_BYTES: usize = 192;
const NATIVE_SUFFIXES: &[&str] = &[".so", ".dll", ".dylib", ".exe", ".node"];

#[derive(Clone, Debug)]
pub struct WrittenPackage {
    pub path: PathBuf,
    pub digest: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct InspectedManifest {
    pub id: String,
    pub version: String,
    pub(crate) catalog_value: serde_json::Value,
}

#[derive(Debug)]
pub struct PackageError {
    message: &'static str,
}

impl fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for PackageError {}

const fn error(message: &'static str) -> PackageError {
    PackageError { message }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireManifest {
    schema_version: u32,
    id: String,
    version: String,
    api_version: String,
    entrypoints: WireEntrypoints,
    permissions: WirePermissions,
    #[serde(deserialize_with = "deserialize_unique_file_map")]
    files: BTreeMap<String, WireFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireEntrypoints {
    view: String,
    controller: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePermissions {
    #[serde(default)]
    network: Vec<WireNetworkPermission>,
    #[serde(default)]
    game_events: Vec<String>,
    #[serde(default)]
    storage: bool,
    #[serde(default)]
    clipboard_write: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireNetworkPermission {
    origin: String,
    method: NetworkMethod,
    path_prefix: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
enum NetworkMethod {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "POST")]
    Post,
    #[serde(rename = "PUT")]
    Put,
    #[serde(rename = "PATCH")]
    Patch,
    #[serde(rename = "DELETE")]
    Delete,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFile {
    sha256: String,
    bytes: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Listing {
    pub(crate) author: String,
    pub(crate) spdx_license: String,
    pub(crate) source_url: String,
    pub(crate) default_locale: String,
    pub(crate) localizations: Vec<ListingLocalization>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListingLocalization {
    pub(crate) locale: String,
    pub(crate) name: String,
    pub(crate) description: String,
}

pub fn write_package(source: &Path, destination: &Path) -> Result<WrittenPackage, PackageError> {
    let entries = collect_entries(source)?;
    let archive = build_stored_archive(&entries)?;
    fs::write(destination, &archive).map_err(|_| error("unable to write package"))?;
    inspect_bytes(&archive)?;
    Ok(WrittenPackage {
        path: destination.to_path_buf(),
        digest: sha256(&archive),
    })
}

pub fn inspect(path: &Path) -> Result<InspectedManifest, PackageError> {
    let bytes = fs::read(path).map_err(|_| error("unable to read package"))?;
    inspect_bytes(&bytes)
}

pub fn inspect_bytes(archive: &[u8]) -> Result<InspectedManifest, PackageError> {
    if archive.len() > MAX_PACKAGE_BYTES {
        return Err(error("package too large"));
    }
    let files = parse_stored_zip(archive)?;
    let manifest_bytes = files
        .get("manifest.json")
        .ok_or_else(|| error("missing manifest.json"))?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(error("invalid manifest"));
    }
    let manifest: WireManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| error("invalid manifest"))?;
    validate_manifest(&manifest, &files)?;
    Ok(InspectedManifest {
        id: manifest.id,
        version: manifest.version,
        catalog_value: serde_json::from_slice(manifest_bytes)
            .map_err(|_| error("invalid manifest"))?,
    })
}

fn collect_entries(source: &Path) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    let manifest_bytes = read_bounded_source(
        &source.join("manifest.json"),
        MAX_MANIFEST_BYTES,
        "missing manifest.json",
    )?;
    let manifest: WireManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| error("invalid manifest"))?;
    validate_listing_source(source)?;
    let mut actual = BTreeSet::new();
    collect_paths(source, "", &mut actual)?;
    let mut expected = BTreeSet::from(["manifest.json".to_owned()]);
    expected.extend(manifest.files.keys().cloned());
    if actual != expected {
        return Err(error("file inventory mismatch"));
    }
    let mut entries = BTreeMap::new();
    for (path, declared) in &manifest.files {
        let maximum = usize::try_from(declared.bytes)
            .ok()
            .filter(|size| *size <= MAX_PACKAGE_BYTES)
            .ok_or_else(|| error("package too large"))?;
        let bytes = read_bounded_source(&source.join(path), maximum, "missing declared file")?;
        if u64::try_from(bytes.len()).ok() != Some(declared.bytes)
            || sha256_hex(&sha256(&bytes)) != declared.sha256
        {
            return Err(error("file inventory mismatch"));
        }
        if native_file(path, &bytes) {
            return Err(error("native files are unsupported"));
        }
        entries.insert(path.clone(), bytes);
    }
    entries.insert("manifest.json".to_owned(), manifest_bytes);
    validate_manifest(&manifest, &entries)?;
    Ok(entries)
}

fn read_bounded_source(
    path: &Path,
    maximum: usize,
    message: &'static str,
) -> Result<Vec<u8>, PackageError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error(message))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || usize::try_from(metadata.len()).map_or(true, |size| size > maximum)
    {
        return Err(error(message));
    }
    let bytes = fs::read(path).map_err(|_| error(message))?;
    if bytes.len() > maximum {
        return Err(error(message));
    }
    Ok(bytes)
}

fn validate_listing_source(source: &Path) -> Result<(), PackageError> {
    let bytes = read_bounded_source(
        &source.join("listing.json"),
        MAX_LISTING_BYTES,
        "invalid listing",
    )?;
    parse_listing_bytes(&bytes).map(|_| ())
}

pub(crate) fn parse_listing_bytes(bytes: &[u8]) -> Result<Listing, PackageError> {
    if bytes.is_empty() || bytes.len() > MAX_LISTING_BYTES {
        return Err(error("invalid listing"));
    }
    let listing: Listing = serde_json::from_slice(bytes).map_err(|_| error("invalid listing"))?;
    if !valid_plain_text(&listing.author, 128)
        || !valid_spdx_license(&listing.spdx_license)
        || !valid_source_url(&listing.source_url)
        || !valid_locale(&listing.default_locale)
        || listing.localizations.is_empty()
        || listing.localizations.len() > 16
    {
        return Err(error("invalid listing"));
    }
    let mut locales = BTreeSet::new();
    for localization in &listing.localizations {
        if !valid_locale(&localization.locale)
            || !locales.insert(localization.locale.as_str())
            || !valid_plain_text(&localization.name, 128)
            || !valid_plain_text(&localization.description, 512)
        {
            return Err(error("invalid listing"));
        }
    }
    if !locales.contains(listing.default_locale.as_str()) {
        return Err(error("invalid listing"));
    }
    Ok(listing)
}

fn collect_paths(
    root: &Path,
    prefix: &str,
    output: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    let current = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let mut names = fs::read_dir(&current)
        .map_err(|_| error("unsafe source"))?
        .map(|entry| entry.map_err(|_| error("unsafe source")))
        .collect::<Result<Vec<_>, _>>()?;
    names.sort_by_key(|entry| entry.file_name());
    for entry in names {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| error("unsafe source"))?;
        if name == "." || name == ".." || name == "listing.json" {
            continue;
        }
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !valid_entry_path(&relative) {
            return Err(error("unsafe source"));
        }
        let file_type = entry.file_type().map_err(|_| error("unsafe source"))?;
        if file_type.is_symlink() {
            return Err(error("unsafe source"));
        }
        if file_type.is_dir() {
            collect_paths(root, &relative, output)?;
        } else if file_type.is_file() {
            if output.len() >= MAX_FILES || !output.insert(relative) {
                return Err(error("file inventory mismatch"));
            }
        } else {
            return Err(error("unsafe source"));
        }
    }
    Ok(())
}

fn validate_manifest(
    manifest: &WireManifest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), PackageError> {
    if manifest.schema_version != 1
        || manifest.api_version != "1"
        || !valid_extension_id(&manifest.id)
        || !canonical_semver(&manifest.version)
        || manifest.files.is_empty()
        || manifest.files.len() >= MAX_FILES
    {
        return Err(error("invalid manifest"));
    }
    if !valid_html_entrypoint(&manifest.entrypoints.view)
        || !manifest.files.contains_key(&manifest.entrypoints.view)
        || manifest
            .entrypoints
            .controller
            .as_ref()
            .is_some_and(|path| !valid_html_entrypoint(path) || !manifest.files.contains_key(path))
    {
        return Err(error("invalid manifest"));
    }
    validate_permissions(&manifest.permissions)?;
    let mut declared = BTreeSet::from(["manifest.json".to_owned()]);
    declared.extend(manifest.files.keys().cloned());
    if declared.len() != files.len() || files.keys().any(|path| !declared.contains(path)) {
        return Err(error("file inventory mismatch"));
    }
    let mut portable_paths = BTreeSet::from(["manifest.json".to_owned()]);
    let mut declared_bytes = 0_u64;
    for (path, declared) in &manifest.files {
        let bytes = files
            .get(path)
            .ok_or_else(|| error("file inventory mismatch"))?;
        declared_bytes = declared_bytes
            .checked_add(declared.bytes)
            .filter(|total| *total <= MAX_PACKAGE_BYTES as u64)
            .ok_or_else(|| error("package too large"))?;
        if !valid_entry_path(path)
            || !portable_paths.insert(path.to_ascii_lowercase())
            || !valid_sha256(&declared.sha256)
            || u64::try_from(bytes.len()).ok() != Some(declared.bytes)
            || sha256_hex(&sha256(bytes)) != declared.sha256
            || native_file(path, bytes)
        {
            return Err(error("file inventory mismatch"));
        }
    }
    Ok(())
}

fn native_file(path: &str, contents: &[u8]) -> bool {
    let lower = path.to_ascii_lowercase();
    NATIVE_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
        || contents.starts_with(b"\x7fELF")
        || contents.starts_with(b"MZ")
}

fn valid_entry_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_FILE_PATH_BYTES
        && path.is_ascii()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && !matches!(segment, "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn valid_html_entrypoint(path: &str) -> bool {
    valid_entry_path(path) && path.ends_with(".html")
}

pub(crate) fn valid_extension_id(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value.is_ascii()
        && value.split('.').count() >= 2
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 63
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

pub(crate) fn canonical_semver(value: &str) -> bool {
    Version::parse(value).is_ok_and(|version| version.to_string() == value)
}

fn validate_permissions(permissions: &WirePermissions) -> Result<(), PackageError> {
    let mut network = BTreeSet::new();
    for permission in &permissions.network {
        if !valid_https_origin(&permission.origin)
            || !valid_network_path_prefix(&permission.path_prefix)
            || !network.insert((
                permission.origin.as_str(),
                permission.method,
                permission.path_prefix.as_str(),
            ))
        {
            return Err(error("invalid permissions"));
        }
    }
    let mut events = BTreeSet::new();
    if permissions
        .game_events
        .iter()
        .any(|event| !valid_game_event_id(event) || !events.insert(event.as_str()))
    {
        return Err(error("invalid permissions"));
    }
    let _ = (permissions.storage, permissions.clipboard_write);
    Ok(())
}

fn valid_https_origin(value: &str) -> bool {
    let Some(authority) = value.strip_prefix("https://") else {
        return false;
    };
    if authority.is_empty()
        || authority
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return false;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if !valid_dns_host(host) {
        return false;
    }
    port.is_none_or(|port| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port
                .parse::<u16>()
                .is_ok_and(|parsed| parsed != 443 && parsed.to_string() == port)
    })
}

fn valid_dns_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn valid_network_path_prefix(value: &str) -> bool {
    value != "/"
        && value.starts_with('/')
        && value.is_ascii()
        && !value.contains("//")
        && !value.contains('\\')
        && value
            .split('/')
            .skip(1)
            .filter(|segment| !segment.is_empty())
            .all(|segment| {
                segment != "."
                    && segment != ".."
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~')
                    })
            })
}

fn valid_game_event_id(value: &str) -> bool {
    let Some(name) = value
        .strip_prefix("overcrow.game.")
        .and_then(|value| value.strip_suffix(".v1"))
    else {
        return false;
    };
    !name.is_empty()
        && name.split('.').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_locale(value: &str) -> bool {
    match value.split_once('-') {
        None => value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_lowercase()),
        Some((language, region)) => {
            language.len() == 2
                && language.bytes().all(|byte| byte.is_ascii_lowercase())
                && region.len() == 2
                && region.bytes().all(|byte| byte.is_ascii_uppercase())
        }
    }
}

fn valid_plain_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.contains(['<', '>'])
        && !value.chars().any(char::is_control)
}

fn valid_spdx_license(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.is_ascii()
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+'))
}

fn valid_source_url(value: &str) -> bool {
    if value.len() > 2_048
        || !value.is_ascii()
        || value.contains(['\\', '%', '?', '#', '@'])
        || value.ends_with('/')
    {
        return false;
    }
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    valid_dns_host(host)
        && !path.is_empty()
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn deserialize_unique_file_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, WireFile>, D::Error>
where
    D: Deserializer<'de>,
{
    struct FileMapVisitor;

    impl<'de> de::Visitor<'de> for FileMapVisitor {
        type Value = BTreeMap<String, WireFile>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a unique, bounded web file map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut files = BTreeMap::new();
            while let Some((path, file)) = map.next_entry()? {
                if files.len() >= MAX_FILES - 1 {
                    return Err(de::Error::custom("web package file limit exceeded"));
                }
                if files.insert(path, file).is_some() {
                    return Err(de::Error::custom("duplicate web package path"));
                }
            }
            Ok(files)
        }
    }

    deserializer.deserialize_map(FileMapVisitor)
}

fn build_stored_archive(entries: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, PackageError> {
    if entries.is_empty() || entries.len() > MAX_FILES {
        return Err(error("package too large"));
    }
    let mut archive = Vec::new();
    let mut records = Vec::with_capacity(entries.len());
    for (path, bytes) in entries {
        let offset = u32::try_from(archive.len()).map_err(|_| error("package too large"))?;
        let size = u32::try_from(bytes.len()).map_err(|_| error("package too large"))?;
        let checksum = crc32fast::hash(bytes);
        push_u32(&mut archive, 0x0403_4b50);
        push_u16(&mut archive, 20);
        push_u16(&mut archive, UTF8_FLAG);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, DOS_DATE_1980_01_01);
        push_u32(&mut archive, checksum);
        push_u32(&mut archive, size);
        push_u32(&mut archive, size);
        push_u16(
            &mut archive,
            u16::try_from(path.len()).map_err(|_| error("invalid manifest"))?,
        );
        push_u16(&mut archive, 0);
        archive.extend_from_slice(path.as_bytes());
        archive.extend_from_slice(bytes);
        records.push((path, size, checksum, offset));
        if archive.len() > MAX_PACKAGE_BYTES {
            return Err(error("package too large"));
        }
    }
    let central_offset = u32::try_from(archive.len()).map_err(|_| error("package too large"))?;
    for (path, size, checksum, offset) in records {
        push_u32(&mut archive, 0x0201_4b50);
        push_u16(&mut archive, (3 << 8) | 20);
        push_u16(&mut archive, 20);
        push_u16(&mut archive, UTF8_FLAG);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, DOS_DATE_1980_01_01);
        push_u32(&mut archive, checksum);
        push_u32(&mut archive, size);
        push_u32(&mut archive, size);
        push_u16(
            &mut archive,
            u16::try_from(path.len()).map_err(|_| error("invalid manifest"))?,
        );
        for _ in 0..4 {
            push_u16(&mut archive, 0);
        }
        push_u32(&mut archive, REGULAR_MODE << 16);
        push_u32(&mut archive, offset);
        archive.extend_from_slice(path.as_bytes());
    }
    let central_size = u32::try_from(archive.len())
        .ok()
        .and_then(|end| end.checked_sub(central_offset))
        .ok_or_else(|| error("package too large"))?;
    let count = u16::try_from(entries.len()).map_err(|_| error("package too large"))?;
    push_u32(&mut archive, 0x0605_4b50);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, count);
    push_u16(&mut archive, count);
    push_u32(&mut archive, central_size);
    push_u32(&mut archive, central_offset);
    push_u16(&mut archive, 0);
    if archive.len() > MAX_PACKAGE_BYTES {
        return Err(error("package too large"));
    }
    Ok(archive)
}

fn parse_stored_zip(archive: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    inspect_bytes_via_writer_roundtrip(archive)
}

fn inspect_bytes_via_writer_roundtrip(
    archive: &[u8],
) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    if archive.len() < 22 {
        return Err(error("invalid package"));
    }
    let end = archive.len() - 22;
    if read_u32(archive, end)? != 0x0605_4b50 {
        return Err(error("invalid package"));
    }
    let count = usize::from(read_u16(archive, end + 10)?);
    let central_size = read_u32(archive, end + 12)? as usize;
    let central_offset = read_u32(archive, end + 16)? as usize;
    if central_offset.checked_add(central_size) != Some(end) || count > MAX_FILES {
        return Err(error("invalid package"));
    }
    let mut files = BTreeMap::new();
    let mut position = central_offset;
    for _ in 0..count {
        if read_u32(archive, position)? != 0x0201_4b50 {
            return Err(error("invalid package"));
        }
        let method = read_u16(archive, position + 10)?;
        if method != 0 {
            return Err(error("compressed packages are unsupported"));
        }
        let name_length = usize::from(read_u16(archive, position + 28)?);
        let size = read_u32(archive, position + 24)? as usize;
        let local_offset = read_u32(archive, position + 42)? as usize;
        let name = std::str::from_utf8(
            archive
                .get(position + 46..position + 46 + name_length)
                .ok_or_else(|| error("invalid package"))?,
        )
        .map_err(|_| error("invalid package"))?
        .to_owned();
        if !valid_entry_path(&name) {
            return Err(error("invalid package"));
        }
        let data_start = local_offset
            .checked_add(30)
            .and_then(|offset| offset.checked_add(name_length))
            .ok_or_else(|| error("invalid package"))?;
        let data = archive
            .get(data_start..data_start + size)
            .ok_or_else(|| error("invalid package"))?
            .to_vec();
        if native_file(&name, &data) && name != "manifest.json" {
            return Err(error("native files are unsupported"));
        }
        if files.insert(name, data).is_some() {
            return Err(error("duplicate package path"));
        }
        position = position
            .checked_add(46 + name_length)
            .ok_or_else(|| error("invalid package"))?;
    }
    Ok(files)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PackageError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| error("invalid package"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, PackageError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| error("invalid package"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn sha256_hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
fn file_sha256_hex(bytes: &[u8]) -> String {
    sha256_hex(&sha256(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    const VIEW: &[u8] = b"<!doctype html><p>hello</p>";

    #[test]
    fn package_is_deterministic_and_inspectable() {
        let source = fixture(&[("index.html", VIEW)]);
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first = first_dir.path().join("a.ocpkg");
        let second = second_dir.path().join("b.ocpkg");
        let written = write_package(source.path(), &first).unwrap();
        write_package(source.path(), &second).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
        let inspected = inspect(&first).unwrap();
        assert_eq!(inspected.id, "com.example.hello");
        assert_eq!(written.digest, sha256(&fs::read(&first).unwrap()));
    }

    #[test]
    fn package_accepts_browser_and_opaque_assets_but_rejects_undeclared_or_native_files() {
        let extra = fixture(&[("index.html", VIEW)]);
        fs::write(extra.path().join("extra.js"), b"no").unwrap();
        assert!(write_package(extra.path(), &extra.path().join("x.ocpkg")).is_err());

        let web = fixture(&[
            ("index.html", VIEW),
            ("module.wasm", b"\0asm\x01\0\0\0"),
            ("catalog.bin", b"opaque browser data"),
            ("font.woff", b"browser font fixture"),
            ("photo.jpg", b"browser image fixture"),
        ]);
        let web_directory = tempfile::tempdir().unwrap();
        let web_output = web_directory.path().join("x.ocpkg");
        write_package(web.path(), &web_output).expect("web asset package");

        let native = fixture(&[("index.html", VIEW), ("module.so", b"\x7fELF")]);
        assert!(write_package(native.path(), &native.path().join("x.ocpkg")).is_err());
    }

    #[test]
    fn package_rejects_manifest_shapes_the_runtime_would_reject() {
        let mutations: [fn(&mut serde_json::Value); 4] = [
            |manifest: &mut serde_json::Value| manifest["version"] = serde_json::json!("latest"),
            |manifest: &mut serde_json::Value| {
                manifest["entrypoints"]["controller"] = serde_json::json!("missing.html")
            },
            |manifest: &mut serde_json::Value| {
                manifest["permissions"] = serde_json::json!({
                    "network": [{
                        "origin": "http://api.example.test",
                        "method": "GET",
                        "pathPrefix": "/"
                    }]
                });
            },
            |manifest: &mut serde_json::Value| manifest["nativeCommand"] = serde_json::json!("id"),
        ];
        for mutation in mutations {
            let source = fixture(&[("index.html", VIEW)]);
            mutate_manifest(source.path(), mutation);
            let output_directory = tempfile::tempdir().unwrap();
            let output = output_directory.path().join("x.ocpkg");
            assert!(write_package(source.path(), &output).is_err());
        }
    }

    #[test]
    fn package_rejects_listing_metadata_the_catalog_would_reject() {
        let source = fixture(&[("index.html", VIEW)]);
        let listing_path = source.path().join("listing.json");
        let mut listing: serde_json::Value =
            serde_json::from_slice(&fs::read(&listing_path).unwrap()).unwrap();
        listing["sourceUrl"] = serde_json::json!("http://example.test/source");
        fs::write(&listing_path, serde_json::to_vec(&listing).unwrap()).unwrap();
        let output_directory = tempfile::tempdir().unwrap();

        assert!(write_package(source.path(), &output_directory.path().join("x.ocpkg")).is_err());
    }

    struct Fixture {
        directory: tempfile::TempDir,
    }

    impl Fixture {
        fn path(&self) -> &Path {
            self.directory.path()
        }
    }

    fn fixture(files: &[(&str, &[u8])]) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut ledger = serde_json::Map::new();
        for (path, bytes) in files {
            let dest = directory.path().join(path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&dest, bytes).unwrap();
            ledger.insert(
                (*path).to_owned(),
                serde_json::json!({"sha256": file_sha256_hex(bytes), "bytes": bytes.len()}),
            );
        }
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "id": "com.example.hello",
            "version": "1.0.0",
            "apiVersion": "1",
            "entrypoints": {"view": "index.html"},
            "permissions": {},
            "files": ledger
        });
        fs::write(
            directory.path().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            directory.path().join("listing.json"),
            serde_json::to_vec(&serde_json::json!({
                "author": "Example",
                "spdxLicense": "MIT",
                "sourceUrl": "https://example.test/source",
                "defaultLocale": "en",
                "localizations": [{
                    "locale": "en",
                    "name": "Hello",
                    "description": "Test extension"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        Fixture { directory }
    }

    fn mutate_manifest(path: &Path, mutation: fn(&mut serde_json::Value)) {
        let manifest_path = path.join("manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        mutation(&mut manifest);
        fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
}
