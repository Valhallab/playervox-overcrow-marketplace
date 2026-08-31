use std::{
    ffi::OsStr,
    fs::File,
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::{Component, Path},
};

use rustix::{
    fd::OwnedFd,
    fs::{CWD, Mode, OFlags, RenameFlags, ResolveFlags, openat, openat2, renameat_with},
};

const MAX_PATH_BYTES: usize = 4_096;
const MAX_TOKEN_BYTES: usize = 64;

pub(crate) fn rename_noreplace(
    live_root: &Path,
    staged_root: &Path,
    public_name: &str,
    source: &Path,
    destination: &Path,
) -> Result<(), ()> {
    if !matches!(public_name, "public" | "published")
        || source == destination
        || !canonical_owned_root(live_root)
        || !canonical_owned_root(staged_root)
        || !allowed_transaction_path(source, live_root, staged_root, public_name)
        || !allowed_transaction_path(destination, live_root, staged_root, public_name)
    {
        return Err(());
    }

    let source_parent = source.parent().ok_or(())?;
    let destination_parent = destination.parent().ok_or(())?;
    let source_name = single_name(source)?;
    let destination_name = single_name(destination)?;
    let source_parent = open_owned_directory(source_parent)?;
    let destination_parent = open_owned_directory(destination_parent)?;
    let source_directory = openat(
        &source_parent,
        source_name,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| ())?;
    if !owned_directory_is_safe(&source_directory) {
        return Err(());
    }

    // There is deliberately no fallback. Linux kernels/filesystems without
    // atomic RENAME_NOREPLACE support fail the publication transaction.
    renameat_with(
        &source_parent,
        source_name,
        &destination_parent,
        destination_name,
        RenameFlags::NOREPLACE,
    )
    .map_err(|_| ())
}

fn canonical_owned_root(path: &Path) -> bool {
    normalized_absolute(path)
        && std::fs::canonicalize(path).is_ok_and(|canonical| canonical == path)
        && open_owned_directory(path).is_ok()
}

fn open_owned_directory(path: &Path) -> Result<OwnedFd, ()> {
    if !normalized_absolute(path) {
        return Err(());
    }
    let descriptor = openat2(
        CWD,
        path,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|_| ())?;
    if !owned_directory_is_safe(&descriptor) {
        return Err(());
    }
    Ok(descriptor)
}

fn owned_directory_is_safe(descriptor: &OwnedFd) -> bool {
    let Ok(cloned) = descriptor.try_clone() else {
        return false;
    };
    let file = File::from(cloned);
    file.metadata().is_ok_and(|metadata| {
        metadata.is_dir()
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.permissions().mode() & 0o022 == 0
    })
}

fn allowed_transaction_path(
    path: &Path,
    live_root: &Path,
    staged_root: &Path,
    public_name: &str,
) -> bool {
    if !normalized_absolute(path) {
        return false;
    }
    if path == staged_root.join("public") {
        return true;
    }
    let Ok(relative) = path.strip_prefix(live_root) else {
        return false;
    };
    let components: Vec<_> = relative.components().collect();
    match components.as_slice() {
        [Component::Normal(name)] => {
            name == &OsStr::new(public_name)
                || reserved_name(name, &format!(".{public_name}-next."))
                || reserved_name(name, &format!(".{public_name}-previous."))
        }
        [Component::Normal(wrapper), Component::Normal(name)] => {
            (reserved_name(wrapper, &format!(".{public_name}-next."))
                && name == &OsStr::new("tree"))
                || (reserved_name(wrapper, &format!(".{public_name}-quarantine."))
                    && valid_token(name))
        }
        _ => false,
    }
}

fn reserved_name(name: &OsStr, prefix: &str) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(prefix))
        .is_some_and(valid_token_str)
}

fn valid_token(value: &OsStr) -> bool {
    value.to_str().is_some_and(valid_token_str)
}

fn valid_token_str(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn single_name(path: &Path) -> Result<&OsStr, ()> {
    let name = path.file_name().ok_or(())?;
    if valid_token(name) || name == OsStr::new("public") || name == OsStr::new("published") {
        Ok(name)
    } else {
        Err(())
    }
}

fn normalized_absolute(path: &Path) -> bool {
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().len() > MAX_PATH_BYTES {
        return false;
    }
    path.components()
        .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}
