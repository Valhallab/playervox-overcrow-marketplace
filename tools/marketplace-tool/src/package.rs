use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write as _},
    os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _, fs::PermissionsExt as _},
    path::Path,
};

use image::{ImageDecoder as _, ImageFormat, ImageReader, Limits};
use rustix::{
    fd::OwnedFd,
    fs::{
        AtFlags, CWD, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags, fchmod, flock,
        fsync, mkdirat, openat, openat2, renameat, renameat_with, unlinkat,
    },
};

use crate::metadata::{TargetSpec, ValidatedMetadata, inspect_component, validate_metadata};

const MAX_PACKAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
const UTF8_FLAG: u16 = 1 << 11;
const DOS_DATE_1980_01_01: u16 = 33;
const REGULAR_MODE: u32 = 0o100644;
const MAX_DECODED_ASSET_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageCode {
    UnsafeSource,
    EntrySize,
    UnsafePath,
    ArchiveSize,
    EntryLimit,
    Metadata,
    Component,
    Digest,
    Asset,
    Preview,
}

pub(crate) struct PackageArtifact {
    metadata: ValidatedMetadata,
    archive: Vec<u8>,
    archive_sha256: String,
    preview: Option<Vec<u8>>,
}

pub(crate) struct PublisherOutput {
    directory: File,
}

impl PublisherOutput {
    pub(crate) fn open(repository: &Path) -> Result<Self, PackageCode> {
        let repository = SourceDirectory::open(repository)?;
        let public = ensure_directory(&repository.0, "public")?;
        let marketplace = ensure_directory(&public, "marketplace")?;
        let directory = ensure_directory(&marketplace, "v1")?;
        flock(&directory, FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| PackageCode::UnsafeSource)?;
        Ok(Self { directory })
    }

    pub(crate) fn publish_objects(&self, packages: &[PackageArtifact]) -> Result<(), PackageCode> {
        let package_root = ensure_directory(&self.directory, "packages")?;
        let preview_root = ensure_directory(&self.directory, "previews")?;
        for package in packages {
            let manifest = package.metadata().manifest();
            let id = ensure_directory(&package_root, manifest.id())?;
            let version = ensure_directory(&id, manifest.version())?;
            let package_name = format!("{}.ocpkg", package.archive_sha256());
            write_object(&version, &package_name, package.archive())?;
            if let Some(preview) = package.preview() {
                let id = ensure_directory(&preview_root, manifest.id())?;
                let version = ensure_directory(&id, manifest.version())?;
                let preview_name = format!("{}.png", crate::lower_hex(&crate::sha256(preview)));
                write_object(&version, &preview_name, preview)?;
            }
        }
        Ok(())
    }

    pub(crate) fn publish_catalog(&self, bytes: &[u8]) -> Result<(), PackageCode> {
        if bytes.is_empty() || bytes.len() > 1024 * 1024 {
            return Err(PackageCode::ArchiveSize);
        }
        write_atomic(&self.directory, "catalog.json", bytes)
    }
}

impl PackageArtifact {
    pub(crate) fn metadata(&self) -> &ValidatedMetadata {
        &self.metadata
    }

    pub(crate) fn archive(&self) -> &[u8] {
        &self.archive
    }

    pub(crate) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub(crate) fn preview(&self) -> Option<&[u8]> {
        self.preview.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        metadata: ValidatedMetadata,
        archive: Vec<u8>,
        preview: Option<Vec<u8>>,
    ) -> Self {
        let archive_sha256 = crate::lower_hex(&crate::sha256(&archive));
        Self {
            metadata,
            archive,
            archive_sha256,
            preview,
        }
    }
}

struct SourceDirectory(OwnedFd);

impl SourceDirectory {
    fn open(path: &Path) -> Result<Self, PackageCode> {
        if !normalized_absolute(path) {
            return Err(PackageCode::UnsafeSource);
        }
        let descriptor = openat2(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| PackageCode::UnsafeSource)?;
        Self::from_descriptor(descriptor)
    }

    fn beneath(repository: &Path, relative: &str) -> Result<Self, PackageCode> {
        let repository = Self::open(repository)?;
        let descriptor = openat2(
            &repository.0,
            relative,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| PackageCode::UnsafeSource)?;
        Self::from_descriptor(descriptor)
    }

    fn from_descriptor(descriptor: OwnedFd) -> Result<Self, PackageCode> {
        let directory = File::from(descriptor);
        if !crate::owned_directory_is_safe(&directory, false) {
            return Err(PackageCode::UnsafeSource);
        }
        Ok(Self(directory.into()))
    }

    fn read(&self, relative: &str, maximum: usize) -> Result<Vec<u8>, PackageCode> {
        if !valid_entry_path(relative) {
            return Err(PackageCode::UnsafeSource);
        }
        let descriptor = openat2(
            &self.0,
            relative,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map_err(|_| PackageCode::UnsafeSource)?;
        read_descriptor(descriptor, maximum)
    }
}

pub(crate) fn read_source_file(
    root: &Path,
    relative: &str,
    maximum: usize,
) -> Result<Vec<u8>, PackageCode> {
    SourceDirectory::open(root)?.read(relative, maximum)
}

fn read_descriptor(descriptor: OwnedFd, maximum: usize) -> Result<Vec<u8>, PackageCode> {
    let file = File::from(descriptor);
    let metadata = file.metadata().map_err(|_| PackageCode::UnsafeSource)?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(PackageCode::UnsafeSource);
    }
    let length = usize::try_from(metadata.len())
        .ok()
        .filter(|length| *length <= maximum)
        .ok_or(PackageCode::EntrySize)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| PackageCode::EntrySize)?;
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageCode::UnsafeSource)?;
    if bytes.len() > maximum || bytes.len() != length {
        return Err(PackageCode::EntrySize);
    }
    Ok(bytes)
}

fn ensure_directory(parent: &impl std::os::fd::AsFd, name: &str) -> Result<File, PackageCode> {
    if !valid_segment(name) {
        return Err(PackageCode::UnsafePath);
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let descriptor = match openat(parent, name, flags, Mode::empty()) {
        Ok(descriptor) => descriptor,
        Err(error) if error == rustix::io::Errno::NOENT => {
            mkdirat(
                parent,
                name,
                Mode::RUSR
                    | Mode::WUSR
                    | Mode::XUSR
                    | Mode::RGRP
                    | Mode::XGRP
                    | Mode::ROTH
                    | Mode::XOTH,
            )
            .map_err(|_| PackageCode::UnsafeSource)?;
            fsync(parent).map_err(|_| PackageCode::UnsafeSource)?;
            openat(parent, name, flags, Mode::empty()).map_err(|_| PackageCode::UnsafeSource)?
        }
        Err(_) => return Err(PackageCode::UnsafeSource),
    };
    let directory = File::from(descriptor);
    if !crate::owned_directory_is_safe(&directory, false) {
        return Err(PackageCode::UnsafeSource);
    }
    Ok(directory)
}

fn write_object(directory: &File, name: &str, bytes: &[u8]) -> Result<(), PackageCode> {
    if !valid_segment(name) {
        return Err(PackageCode::UnsafePath);
    }
    let temporary_name = format!(".{name}.tmp");
    stage_object(directory, &temporary_name, bytes)?;
    let result = match renameat_with(
        directory,
        &temporary_name,
        directory,
        name,
        RenameFlags::NOREPLACE,
    ) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::EXIST => compare_existing(directory, name, bytes),
        Err(_) => Err(PackageCode::UnsafeSource),
    }
    .and_then(|()| fsync(directory).map_err(|_| PackageCode::UnsafeSource));
    let _ = unlinkat(directory, &temporary_name, AtFlags::empty());
    result
}

fn stage_object(directory: &File, temporary_name: &str, bytes: &[u8]) -> Result<(), PackageCode> {
    match openat(
        directory,
        temporary_name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(descriptor) => {
            validate_staging_file(&File::from(descriptor))?;
            unlinkat(directory, temporary_name, AtFlags::empty())
                .map_err(|_| PackageCode::UnsafeSource)?;
        }
        Err(error) if error == rustix::io::Errno::NOENT => {}
        Err(_) => return Err(PackageCode::UnsafeSource),
    }

    let descriptor = openat(
        directory,
        temporary_name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| PackageCode::UnsafeSource)?;
    let mut file = File::from(descriptor);
    let result = (|| {
        validate_file_mode(&file, 0o600)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|_| PackageCode::UnsafeSource)?;
        fchmod(&file, Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH)
            .map_err(|_| PackageCode::UnsafeSource)?;
        file.sync_all().map_err(|_| PackageCode::UnsafeSource)?;
        validate_public_file(&file)
    })();
    if result.is_err() {
        let _ = unlinkat(directory, temporary_name, AtFlags::empty());
        return Err(PackageCode::UnsafeSource);
    }
    Ok(())
}

fn compare_existing(directory: &File, name: &str, expected: &[u8]) -> Result<(), PackageCode> {
    let descriptor = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| PackageCode::UnsafeSource)?;
    let file = File::from(descriptor);
    validate_public_file(&file)?;
    let metadata = file.metadata().map_err(|_| PackageCode::UnsafeSource)?;
    if metadata.len() != expected.len() as u64 {
        return Err(PackageCode::UnsafeSource);
    }
    let mut actual = Vec::new();
    actual
        .try_reserve_exact(expected.len())
        .map_err(|_| PackageCode::EntrySize)?;
    file.take(expected.len().saturating_add(1) as u64)
        .read_to_end(&mut actual)
        .map_err(|_| PackageCode::UnsafeSource)?;
    if actual == expected {
        Ok(())
    } else {
        Err(PackageCode::UnsafeSource)
    }
}

fn write_atomic(directory: &File, final_name: &str, bytes: &[u8]) -> Result<(), PackageCode> {
    let temporary_name = format!(".catalog.{}.tmp", std::process::id());
    let descriptor = openat(
        directory,
        &temporary_name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
    )
    .map_err(|_| PackageCode::UnsafeSource)?;
    let mut temporary = File::from(descriptor);
    let result = (|| {
        validate_public_file(&temporary)?;
        temporary
            .write_all(bytes)
            .and_then(|()| temporary.sync_all())
            .map_err(|_| PackageCode::UnsafeSource)?;
        renameat(directory, &temporary_name, directory, final_name)
            .map_err(|_| PackageCode::UnsafeSource)?;
        fsync(directory).map_err(|_| PackageCode::UnsafeSource)
    })();
    if result.is_err() {
        let _ = unlinkat(directory, &temporary_name, AtFlags::empty());
    }
    result
}

fn validate_public_file(file: &File) -> Result<(), PackageCode> {
    validate_file_mode(file, 0o644)
}

fn validate_staging_file(file: &File) -> Result<(), PackageCode> {
    let metadata = file.metadata().map_err(|_| PackageCode::UnsafeSource)?;
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.is_file()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.nlink() == 1
        && matches!(mode, 0o600 | 0o644)
    {
        Ok(())
    } else {
        Err(PackageCode::UnsafeSource)
    }
}

fn validate_file_mode(file: &File, mode: u32) -> Result<(), PackageCode> {
    let metadata = file.metadata().map_err(|_| PackageCode::UnsafeSource)?;
    if metadata.is_file()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.nlink() == 1
        && metadata.permissions().mode() & 0o7777 == mode
    {
        Ok(())
    } else {
        Err(PackageCode::UnsafeSource)
    }
}

fn valid_segment(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 192
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !matches!(name, "." | "..")
}

pub(crate) fn build_package(
    repository: &Path,
    target: &TargetSpec,
) -> Result<PackageArtifact, PackageCode> {
    let source = SourceDirectory::beneath(repository, target.source_directory())?;
    let manifest_bytes = source.read("manifest.json", 64 * 1024)?;
    let listing_bytes = source.read("listing.json", 64 * 1024)?;
    let metadata =
        validate_metadata(&manifest_bytes, &listing_bytes).map_err(|_| PackageCode::Metadata)?;

    let mut files = BTreeMap::from([("manifest.json".to_owned(), manifest_bytes)]);
    let component = read_declared(
        &source,
        metadata.manifest().files().component(),
        4 * 1024 * 1024,
    )?;
    inspect_component(&component).map_err(|_| PackageCode::Component)?;
    files.insert("component.wasm".to_owned(), component);

    for file in metadata.manifest().files().locales().values() {
        let bytes = read_declared(&source, file, 64 * 1024)?;
        files.insert(file.path().to_owned(), bytes);
    }
    let mut asset_total = 0usize;
    let mut decoded_asset_total = 0usize;
    for file in metadata.manifest().files().assets().values() {
        let bytes = read_declared(&source, file, 2 * 1024 * 1024)?;
        asset_total = asset_total
            .checked_add(bytes.len())
            .filter(|total| *total <= 8 * 1024 * 1024)
            .ok_or(PackageCode::EntrySize)?;
        let decoded = validate_png(&bytes, 2_048).map_err(|_| PackageCode::Asset)?;
        decoded_asset_total = decoded_asset_total
            .checked_add(decoded)
            .filter(|total| *total <= MAX_DECODED_ASSET_BYTES)
            .ok_or(PackageCode::Asset)?;
        files.insert(file.path().to_owned(), bytes);
    }
    let preview = metadata
        .preview_file()
        .map(|path| {
            if let Some(bytes) = files.get(path) {
                return Ok(bytes.clone());
            }
            source.read(path, 256 * 1024)
        })
        .transpose()?;
    if let Some(bytes) = preview.as_deref() {
        if bytes.is_empty() || bytes.len() > 256 * 1024 {
            return Err(PackageCode::Preview);
        }
        validate_png(bytes, 1_024).map_err(|_| PackageCode::Preview)?;
    }
    let archive = build_stored_archive(&files)?;
    let archive_sha256 = crate::lower_hex(&crate::sha256(&archive));
    Ok(PackageArtifact {
        metadata,
        archive,
        archive_sha256,
        preview,
    })
}

fn read_declared(
    source: &SourceDirectory,
    declared: &crate::metadata::DeclaredFile,
    maximum: usize,
) -> Result<Vec<u8>, PackageCode> {
    let bytes = source.read(declared.path(), maximum)?;
    if crate::lower_hex(&crate::sha256(&bytes)) != declared.sha256() {
        return Err(PackageCode::Digest);
    }
    Ok(bytes)
}

pub(crate) fn build_stored_archive(
    entries: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, PackageCode> {
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err(PackageCode::EntryLimit);
    }
    let mut folded = BTreeSet::new();
    for path in entries.keys() {
        if !valid_entry_path(path) {
            return Err(PackageCode::UnsafePath);
        }
        let lower = path.to_ascii_lowercase();
        if folded.iter().any(|existing: &String| {
            lower == *existing
                || lower
                    .strip_prefix(existing)
                    .is_some_and(|suffix| suffix.starts_with('/'))
                || existing
                    .strip_prefix(&lower)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) {
            return Err(PackageCode::UnsafePath);
        }
        folded.insert(lower);
    }

    let mut archive = Vec::new();
    let mut records = Vec::with_capacity(entries.len());
    for (path, bytes) in entries {
        let local_offset = u32::try_from(archive.len()).map_err(|_| PackageCode::ArchiveSize)?;
        let size = u32::try_from(bytes.len()).map_err(|_| PackageCode::EntrySize)?;
        let name_length = u16::try_from(path.len()).map_err(|_| PackageCode::UnsafePath)?;
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
        push_u16(&mut archive, name_length);
        push_u16(&mut archive, 0);
        archive.extend_from_slice(path.as_bytes());
        archive.extend_from_slice(bytes);
        records.push((path, size, checksum, local_offset));
        if archive.len() > MAX_PACKAGE_BYTES {
            return Err(PackageCode::ArchiveSize);
        }
    }

    let central_offset = u32::try_from(archive.len()).map_err(|_| PackageCode::ArchiveSize)?;
    for (path, size, checksum, local_offset) in records {
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
            u16::try_from(path.len()).map_err(|_| PackageCode::UnsafePath)?,
        );
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u16(&mut archive, 0);
        push_u32(&mut archive, REGULAR_MODE << 16);
        push_u32(&mut archive, local_offset);
        archive.extend_from_slice(path.as_bytes());
    }
    let central_size = u32::try_from(archive.len())
        .ok()
        .and_then(|end| end.checked_sub(central_offset))
        .ok_or(PackageCode::ArchiveSize)?;
    let count = u16::try_from(entries.len()).map_err(|_| PackageCode::EntryLimit)?;
    push_u32(&mut archive, 0x0605_4b50);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, 0);
    push_u16(&mut archive, count);
    push_u16(&mut archive, count);
    push_u32(&mut archive, central_size);
    push_u32(&mut archive, central_offset);
    push_u16(&mut archive, 0);
    if archive.len() > MAX_PACKAGE_BYTES {
        return Err(PackageCode::ArchiveSize);
    }
    Ok(archive)
}

fn valid_entry_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 192
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

fn normalized_absolute(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    bytes.starts_with(b"/")
        && bytes.len() > 1
        && !bytes[1..]
            .split(|byte| *byte == b'/')
            .any(|segment| segment.is_empty() || matches!(segment, b"." | b".."))
}

fn validate_png(encoded: &[u8], maximum_dimension: u32) -> Result<usize, ()> {
    if !encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(());
    }
    let mut offset = 8usize;
    let mut dimensions = None;
    let mut saw_pixels = false;
    let mut saw_palette = false;
    let mut saw_transparency = false;
    let mut saw_end = false;
    while offset < encoded.len() {
        let length = usize::try_from(read_be_u32(encoded, offset)?).map_err(|_| ())?;
        let kind_offset = offset.checked_add(4).ok_or(())?;
        let data_offset = kind_offset.checked_add(4).ok_or(())?;
        let checksum_offset = data_offset.checked_add(length).ok_or(())?;
        let end = checksum_offset
            .checked_add(4)
            .filter(|end| *end <= encoded.len())
            .ok_or(())?;
        let kind = encoded.get(kind_offset..data_offset).ok_or(())?;
        let data = encoded.get(data_offset..checksum_offset).ok_or(())?;
        if crc32fast::hash(encoded.get(kind_offset..checksum_offset).ok_or(())?)
            != read_be_u32(encoded, checksum_offset)?
        {
            return Err(());
        }
        match kind {
            b"IHDR" if offset == 8 && length == 13 && dimensions.is_none() => {
                let width = read_be_u32(data, 0)?;
                let height = read_be_u32(data, 4)?;
                let rgba = u64::from(width)
                    .checked_mul(u64::from(height))
                    .and_then(|pixels| pixels.checked_mul(4))
                    .filter(|bytes| *bytes <= 16 * 1024 * 1024)
                    .ok_or(())?;
                if width == 0
                    || height == 0
                    || width > maximum_dimension
                    || height > maximum_dimension
                {
                    return Err(());
                }
                dimensions = Some((width, height, rgba));
            }
            b"PLTE"
                if dimensions.is_some()
                    && !saw_pixels
                    && !saw_palette
                    && !data.is_empty()
                    && data.len() % 3 == 0 =>
            {
                saw_palette = true;
            }
            b"tRNS" if dimensions.is_some() && !saw_pixels && !saw_transparency => {
                saw_transparency = true;
            }
            b"IDAT" if dimensions.is_some() => saw_pixels = true,
            b"IEND" if length == 0 && saw_pixels && end == encoded.len() => {
                saw_end = true;
                break;
            }
            _ => return Err(()),
        }
        offset = end;
    }
    let (width, height, rgba) = dimensions.filter(|_| saw_end).ok_or(())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(maximum_dimension);
    limits.max_image_height = Some(maximum_dimension);
    limits.max_alloc = Some(rgba);
    let mut reader = ImageReader::with_format(std::io::Cursor::new(encoded), ImageFormat::Png);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|_| ())?;
    decoder.set_limits(limits).map_err(|_| ())?;
    if decoder.dimensions() != (width, height) || decoder.total_bytes() > rgba {
        return Err(());
    }
    let decoded_size = usize::try_from(decoder.total_bytes()).map_err(|_| ())?;
    let mut decoded = Vec::new();
    decoded.try_reserve_exact(decoded_size).map_err(|_| ())?;
    decoded.resize(decoded_size, 0);
    decoder.read_image(&mut decoded).map_err(|_| ())?;
    usize::try_from(rgba).map_err(|_| ())
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Result<u32, ()> {
    let value = bytes
        .get(offset..offset.checked_add(4).ok_or(())?)
        .ok_or(())?;
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs::{File, Permissions},
        os::unix::fs::{PermissionsExt as _, symlink},
    };

    use super::{
        PackageArtifact, PackageCode, PublisherOutput, build_stored_archive, read_source_file,
        write_object,
    };
    use crate::metadata::validate_metadata;

    const MANIFEST: &[u8] = include_bytes!("../../../examples/hello-widget/manifest.json");
    const LISTING: &[u8] = br#"{
        "author":"PlayerVox",
        "spdxLicense":"AGPL-3.0-only",
        "sourceUrl":"https://github.com/PlayerVox/playervox-overcrow-marketplace",
        "localizations":[{"locale":"en","name":"Hello","description":"Safe"}],
        "previewFile":"preview.png"
    }"#;

    #[test]
    fn stored_archive_is_byte_reproducible_and_canonical() {
        let entries = BTreeMap::from([
            ("manifest.json".to_owned(), b"manifest".to_vec()),
            ("component.wasm".to_owned(), b"component".to_vec()),
        ]);
        let first = build_stored_archive(&entries).expect("archive");
        let second = build_stored_archive(&entries).expect("archive");
        assert_eq!(first, second);

        assert_eq!(&first[..4], b"PK\x03\x04");
        assert_eq!(u16::from_le_bytes([first[6], first[7]]), 1 << 11);
        assert_eq!(u16::from_le_bytes([first[8], first[9]]), 0);
        assert_eq!(u16::from_le_bytes([first[10], first[11]]), 0);
        assert_eq!(u16::from_le_bytes([first[12], first[13]]), 33);
        assert!(first.windows(4).any(|bytes| bytes == b"PK\x01\x02"));
        assert!(first.ends_with(&[0; 2]), "empty ZIP comment");
    }

    #[test]
    fn source_snapshot_rejects_symlinks_and_enforces_bounds() {
        let temporary = tempfile::tempdir().expect("temporary source");
        std::fs::write(temporary.path().join("regular"), b"1234").expect("regular fixture");
        symlink("regular", temporary.path().join("linked")).expect("symlink fixture");

        assert_eq!(
            read_source_file(temporary.path(), "linked", 16),
            Err(PackageCode::UnsafeSource)
        );
        assert_eq!(
            read_source_file(temporary.path(), "regular", 3),
            Err(PackageCode::EntrySize)
        );
        assert_eq!(
            read_source_file(temporary.path(), "regular", 4).expect("bounded regular file"),
            b"1234"
        );

        std::fs::set_permissions(temporary.path(), Permissions::from_mode(0o770))
            .expect("unsafe source mode");
        assert_eq!(
            read_source_file(temporary.path(), "regular", 4),
            Err(PackageCode::UnsafeSource)
        );
        std::fs::set_permissions(temporary.path(), Permissions::from_mode(0o700))
            .expect("restore source mode");
    }

    #[test]
    fn archive_rejects_oversized_or_unsafe_entry_names() {
        for name in ["../component.wasm", "/component.wasm"] {
            let entries = BTreeMap::from([(name.to_owned(), vec![0])]);
            assert_eq!(build_stored_archive(&entries), Err(PackageCode::UnsafePath));
        }
        let entries =
            BTreeMap::from([("component.wasm".to_owned(), vec![0; 16 * 1024 * 1024 + 1])]);
        assert_eq!(
            build_stored_archive(&entries),
            Err(PackageCode::ArchiveSize)
        );
    }

    #[test]
    fn deterministic_object_stage_recovers_safe_stale_bytes_only() {
        let temporary = tempfile::tempdir().expect("object directory");
        let directory = File::open(temporary.path()).expect("object directory descriptor");
        let staged = temporary.path().join(".object.ocpkg.tmp");
        std::fs::write(&staged, b"partial").expect("stale staging bytes");
        std::fs::set_permissions(&staged, Permissions::from_mode(0o600))
            .expect("stale staging mode");
        write_object(&directory, "object.ocpkg", b"complete").expect("recover stale staging");
        assert_eq!(
            std::fs::read(temporary.path().join("object.ocpkg")).expect("published object"),
            b"complete"
        );
        assert!(!staged.exists(), "staging file must not leak");
    }

    #[test]
    fn deterministic_object_stage_refuses_untrusted_existing_entries() {
        for fixture in ["writable", "linked", "hard-linked"] {
            let temporary = tempfile::tempdir().expect("object directory");
            let directory = File::open(temporary.path()).expect("object directory descriptor");
            let staged = temporary.path().join(".object.ocpkg.tmp");
            let source = temporary.path().join("source");
            match fixture {
                "writable" => {
                    std::fs::write(&staged, b"hostile").expect("writable staging entry");
                    std::fs::set_permissions(&staged, Permissions::from_mode(0o666))
                        .expect("writable staging mode");
                }
                "linked" => symlink("source", &staged).expect("staging symlink"),
                "hard-linked" => {
                    std::fs::write(&source, b"hostile").expect("hard-link source");
                    std::fs::hard_link(&source, &staged).expect("staging hard link");
                }
                _ => unreachable!("closed fixture table"),
            }
            assert_eq!(
                write_object(&directory, "object.ocpkg", b"complete"),
                Err(PackageCode::UnsafeSource),
                "fixture {fixture}"
            );
            assert!(!temporary.path().join("object.ocpkg").exists());
        }
    }

    #[test]
    fn publication_is_content_addressed_and_refuses_existing_mismatched_bytes() {
        let repository = tempfile::tempdir().expect("repository");
        let metadata = validate_metadata(MANIFEST, LISTING).expect("metadata");
        let package = PackageArtifact::fixture(
            metadata,
            b"deterministic package".to_vec(),
            Some(b"preview bytes".to_vec()),
        );
        {
            let output = PublisherOutput::open(repository.path()).expect("publisher output");
            output
                .publish_objects(std::slice::from_ref(&package))
                .expect("publish objects");
            output
                .publish_catalog(b"catalog one")
                .expect("publish catalog");
        }
        let package_path = repository.path().join(format!(
            "public/marketplace/v1/packages/{}/{}/{}.ocpkg",
            package.metadata().manifest().id(),
            package.metadata().manifest().version(),
            package.archive_sha256()
        ));
        assert_eq!(
            std::fs::read(&package_path).expect("published package"),
            package.archive()
        );

        std::fs::write(&package_path, b"hostile replacement").expect("tamper fixture");
        let output = PublisherOutput::open(repository.path()).expect("publisher retry");
        assert_eq!(
            output.publish_objects(std::slice::from_ref(&package)),
            Err(PackageCode::UnsafeSource)
        );
        assert_eq!(
            std::fs::read(repository.path().join("public/marketplace/v1/catalog.json"))
                .expect("old catalog"),
            b"catalog one"
        );

        drop(output);
        let public_root = repository.path().join("public/marketplace/v1");
        std::fs::set_permissions(&public_root, Permissions::from_mode(0o777))
            .expect("unsafe public mode");
        assert!(PublisherOutput::open(repository.path()).is_err());
        std::fs::set_permissions(public_root, Permissions::from_mode(0o755))
            .expect("restore public mode");
    }
}
