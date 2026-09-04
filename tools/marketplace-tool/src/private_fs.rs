use std::{
    fs,
    io::Write as _,
    os::unix::fs::{
        DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(crate) struct PrivateFsError;

pub(crate) fn validate_private_directory(path: &Path) -> Result<(), PrivateFsError> {
    if !path.is_absolute() || fs::canonicalize(path).map_err(|_| PrivateFsError)? != path {
        return Err(PrivateFsError);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| PrivateFsError)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != process_uid()?
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(PrivateFsError);
    }
    Ok(())
}

pub(crate) fn ensure_private_directory(path: &Path) -> Result<PathBuf, PrivateFsError> {
    if !path.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|_| PrivateFsError)?;
        sync_directory(path.parent().ok_or(PrivateFsError)?)?;
    }
    validate_private_directory(path)?;
    Ok(path.to_path_buf())
}

pub(crate) fn read_regular_file(path: &Path, maximum: u64) -> Result<Vec<u8>, PrivateFsError> {
    if !path.is_absolute() {
        return Err(PrivateFsError);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| PrivateFsError)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != process_uid()?
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.len() > maximum
    {
        return Err(PrivateFsError);
    }
    let bytes = fs::read(path).map_err(|_| PrivateFsError)?;
    if u64::try_from(bytes.len())
        .ok()
        .is_none_or(|size| size > maximum)
    {
        return Err(PrivateFsError);
    }
    Ok(bytes)
}

pub(crate) fn commit_file(
    store: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), PrivateFsError> {
    if destination.exists() {
        return if read_regular_file(destination, bytes.len() as u64)? == bytes {
            Ok(())
        } else {
            Err(PrivateFsError)
        };
    }
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = store.join(format!(".commit-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| PrivateFsError)?;
        file.write_all(bytes).map_err(|_| PrivateFsError)?;
        file.sync_all().map_err(|_| PrivateFsError)?;
        drop(file);
        match fs::hard_link(&temporary, destination) {
            Ok(()) => sync_directory(destination.parent().ok_or(PrivateFsError)?),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if read_regular_file(destination, bytes.len() as u64)? == bytes {
                    Ok(())
                } else {
                    Err(PrivateFsError)
                }
            }
            Err(_) => Err(PrivateFsError),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn process_uid() -> Result<u32, PrivateFsError> {
    // OverCrow Marketplace is Linux-only; /proc/self avoids an unsafe geteuid call.
    fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .map_err(|_| PrivateFsError)
}

fn sync_directory(path: &Path) -> Result<(), PrivateFsError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PrivateFsError)
}
