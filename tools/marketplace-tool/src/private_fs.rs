use std::{
    fs,
    io::{Read as _, Write as _},
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
    let current_uid = process_uid()?;
    // Protect each name before resolving its child. A sticky shared directory
    // protects only children owned by the caller or root, so every component
    // must have a trusted owner, including descendants of /tmp and /var/tmp.
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| PrivateFsError)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !trusted_ancestor(metadata.uid(), metadata.permissions().mode(), current_uid)
        {
            return Err(PrivateFsError);
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| PrivateFsError)?;
    if metadata.uid() != current_uid || metadata.permissions().mode() & 0o077 != 0 {
        return Err(PrivateFsError);
    }
    Ok(())
}

fn trusted_ancestor(owner: u32, mode: u32, current_uid: u32) -> bool {
    (owner == 0 || owner == current_uid) && (mode & 0o022 == 0 || mode & 0o1000 != 0)
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
    let file = open_regular_file(path)?;
    let metadata = file.metadata().map_err(|_| PrivateFsError)?;
    if metadata.uid() != process_uid()? || metadata.permissions().mode() & 0o022 != 0 {
        return Err(PrivateFsError);
    }
    read_bounded_file(file, maximum)
}

pub(crate) fn open_regular_file(path: &Path) -> Result<fs::File, PrivateFsError> {
    // Validate the descriptor we will read. NONBLOCK also makes FIFO replacement
    // fail promptly; NOFOLLOW rejects a symlink swapped in before open.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| PrivateFsError)?;
    if !file.metadata().map_err(|_| PrivateFsError)?.is_file() {
        return Err(PrivateFsError);
    }
    Ok(file)
}

pub(crate) fn read_bounded_file(file: fs::File, maximum: u64) -> Result<Vec<u8>, PrivateFsError> {
    if file.metadata().map_err(|_| PrivateFsError)?.len() > maximum {
        return Err(PrivateFsError);
    }
    let limit = maximum.checked_add(1).ok_or(PrivateFsError)?;
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| PrivateFsError)?;
    if bytes.len() as u64 > maximum {
        return Err(PrivateFsError);
    }
    Ok(bytes)
}

pub(crate) fn lock_private_directory(path: &Path) -> Result<fs::File, PrivateFsError> {
    validate_private_directory(path)?;
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
        .map_err(|_| PrivateFsError)?;
    let metadata = directory.metadata().map_err(|_| PrivateFsError)?;
    if metadata.uid() != process_uid()? || metadata.permissions().mode() & 0o077 != 0 {
        return Err(PrivateFsError);
    }
    // Keep the directory descriptor alive for the whole transaction. Failing
    // immediately on contention bounds work without polling or stale lock files.
    directory.try_lock().map_err(|_| PrivateFsError)?;
    Ok(directory)
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

pub(crate) fn replace_file(
    directory: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), PrivateFsError> {
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".replace-{}-{sequence}", std::process::id()));
    let result = (|| {
        commit_file(directory, &temporary, bytes)?;
        fs::rename(&temporary, destination).map_err(|_| PrivateFsError)?;
        sync_directory(directory)
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn sync_directory(path: &Path) -> Result<(), PrivateFsError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PrivateFsError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek as _;

    #[test]
    fn private_directory_rejects_writable_ancestors_without_sticky_protection() {
        let scratch = tempfile::tempdir().unwrap();
        let parent = scratch.path().join("shared");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
        let private = parent.join("private");
        fs::create_dir(&private).unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(validate_private_directory(&private).is_err());
        assert!(lock_private_directory(&private).is_err());

        fs::set_permissions(&parent, fs::Permissions::from_mode(0o1777)).unwrap();
        validate_private_directory(&private).expect("owned sticky ancestor protects private child");
        let alias = scratch.path().join("alias");
        std::os::unix::fs::symlink(&parent, &alias).unwrap();
        assert!(validate_private_directory(&alias.join("private")).is_err());
    }

    #[test]
    fn ancestor_policy_requires_a_trusted_owner_even_with_sticky_protection() {
        for mode in [0o700, 0o755, 0o1777] {
            assert!(trusted_ancestor(0, mode, 1000));
            assert!(trusted_ancestor(1000, mode, 1000));
            assert!(!trusted_ancestor(1001, mode, 1000));
        }
        for owner in [0, 1000, 1001] {
            assert!(!trusted_ancestor(owner, 0o777, 1000));
            assert!(!trusted_ancestor(owner, 0o775, 1000));
        }
    }

    #[test]
    fn bounded_read_limits_actual_bytes_when_metadata_reports_zero() {
        // Procfs regular files have zero metadata length but return real bytes.
        let file = open_regular_file(Path::new("/proc/self/status")).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
        let mut shared_position = file.try_clone().unwrap();
        assert!(read_bounded_file(file, 16).is_err());
        assert_eq!(shared_position.stream_position().unwrap(), 17);
    }

    #[test]
    fn bounded_read_keeps_the_validated_descriptor_when_path_is_replaced() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("input");
        fs::write(&path, b"original").unwrap();
        let file = open_regular_file(&path).unwrap();
        fs::rename(&path, scratch.path().join("original")).unwrap();
        let replacement = scratch.path().join("replacement");
        fs::write(&replacement, b"replacement").unwrap();
        std::os::unix::fs::symlink(&replacement, &path).unwrap();
        assert_eq!(read_bounded_file(file, 8).unwrap(), b"original");
        assert!(read_regular_file(&path, 32).is_err());
    }

    #[test]
    fn private_read_preserves_permissions_and_size_checks() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("input");
        fs::write(&path, b"four").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_regular_file(&path, 4).unwrap(), b"four");
        assert!(read_regular_file(&path, 3).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o622)).unwrap();
        assert!(read_regular_file(&path, 4).is_err());
        assert!(read_regular_file(scratch.path(), 4).is_err());
    }
}
