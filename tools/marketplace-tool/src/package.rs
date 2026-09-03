use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const UTF8_FLAG: u16 = 1 << 11;
const DOS_DATE_1980_01_01: u16 = 33;
const REGULAR_MODE: u32 = 0o100644;
const MAX_FILES: usize = 4096;
const MAX_PACKAGE_BYTES: usize = 128 * 1024 * 1024;
const NATIVE_SUFFIXES: &[&str] = &[".wasm", ".so", ".dll", ".dylib", ".exe", ".node"];

#[derive(Clone, Debug)]
pub struct WrittenPackage {
    pub path: PathBuf,
    pub digest: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct InspectedManifest {
    pub id: String,
    pub version: String,
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
struct WireManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    id: String,
    version: String,
    #[serde(rename = "apiVersion")]
    api_version: String,
    entrypoints: WireEntrypoints,
    files: BTreeMap<String, WireFile>,
}

#[derive(Deserialize)]
struct WireEntrypoints {
    view: String,
}

#[derive(Deserialize)]
struct WireFile {
    sha256: String,
    bytes: u64,
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
    let manifest: WireManifest =
        serde_json::from_slice(manifest_bytes).map_err(|_| error("invalid manifest"))?;
    validate_manifest(&manifest, &files)?;
    Ok(InspectedManifest {
        id: manifest.id,
        version: manifest.version,
    })
}

fn collect_entries(source: &Path) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
    let manifest_bytes =
        fs::read(source.join("manifest.json")).map_err(|_| error("missing manifest.json"))?;
    let manifest: WireManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| error("invalid manifest"))?;
    let mut actual = BTreeSet::new();
    collect_paths(source, "", &mut actual)?;
    let mut expected = BTreeSet::from(["manifest.json".to_owned()]);
    expected.extend(manifest.files.keys().cloned());
    if actual != expected {
        return Err(error("file inventory mismatch"));
    }
    let mut entries = BTreeMap::new();
    for (path, declared) in &manifest.files {
        let bytes = fs::read(source.join(path)).map_err(|_| error("missing declared file"))?;
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
    if manifest.schema_version != 1 || manifest.api_version != "1" || manifest.id.is_empty() {
        return Err(error("invalid manifest"));
    }
    if !manifest.entrypoints.view.ends_with(".html")
        || !manifest.files.contains_key(&manifest.entrypoints.view)
    {
        return Err(error("invalid manifest"));
    }
    let mut declared = BTreeSet::from(["manifest.json".to_owned()]);
    declared.extend(manifest.files.keys().cloned());
    if declared.len() != files.len() || files.keys().any(|path| !declared.contains(path)) {
        return Err(error("file inventory mismatch"));
    }
    for (path, declared) in &manifest.files {
        let bytes = files
            .get(path)
            .ok_or_else(|| error("file inventory mismatch"))?;
        if u64::try_from(bytes.len()).ok() != Some(declared.bytes)
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
        || contents.starts_with(b"\0asm")
        || contents.starts_with(b"\x7fELF")
        || contents.starts_with(b"MZ")
}

fn valid_entry_path(path: &str) -> bool {
    !path.is_empty()
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
    fn package_rejects_undeclared_and_native_files() {
        let extra = fixture(&[("index.html", VIEW)]);
        fs::write(extra.path().join("extra.js"), b"no").unwrap();
        assert!(write_package(extra.path(), &extra.path().join("x.ocpkg")).is_err());

        let native = fixture(&[("index.html", VIEW), ("module.wasm", b"\0asm\x01\0\0\0")]);
        assert!(write_package(native.path(), &native.path().join("x.ocpkg")).is_err());
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
        Fixture { directory }
    }
}
