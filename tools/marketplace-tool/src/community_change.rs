use std::{collections::BTreeSet, fs, io::ErrorKind, path::Path};

use crate::{metadata::TargetSpec, package::read_private_file};

const MAX_CHANGED_BYTES: usize = 256 * 1024;
const MAX_CHANGED_PATHS: usize = 512;
const MAX_CHANGED_PATH_BYTES: usize = 512;
const MAX_COMMUNITY_ROOTS: usize = 100;

pub(crate) fn validate(
    repository: &Path,
    changed_paths: &Path,
    targets: &[TargetSpec],
) -> Result<(), ()> {
    let bytes = read_private_file(changed_paths, MAX_CHANGED_BYTES).map_err(|_| ())?;
    if bytes.is_empty() {
        return Ok(());
    }
    let records = bytes.strip_suffix(&[0]).ok_or(())?;
    let records: Vec<_> = records.split(|byte| *byte == 0).collect();
    if records.is_empty() || records.len() > MAX_CHANGED_PATHS {
        return Err(());
    }

    let mut roots = BTreeSet::new();
    for record in records {
        let path = std::str::from_utf8(record).map_err(|_| ())?;
        if !valid_changed_path(path) {
            return Err(());
        }
        if !path.starts_with("community/") || path == "community/README.md" {
            continue;
        }
        let mut parts = path.split('/');
        if parts.next() != Some("community") {
            return Err(());
        }
        let publisher = parts.next().ok_or(())?;
        let widget = parts.next().ok_or(())?;
        if !valid_identifier(publisher) || !valid_identifier(widget) || parts.next().is_none() {
            return Err(());
        }
        roots.insert(format!("community/{publisher}/{widget}"));
        if roots.len() > MAX_COMMUNITY_ROOTS {
            return Err(());
        }
    }

    let sources: BTreeSet<_> = targets
        .iter()
        .map(|target| target.source_directory())
        .collect();
    for root in roots {
        let submission = repository.join(&root);
        match fs::symlink_metadata(&submission) {
            Ok(metadata) => {
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || fs::canonicalize(&submission).ok().as_deref() != Some(submission.as_path())
                    || !sources.contains(root.as_str())
                {
                    return Err(());
                }
            }
            Err(error)
                if error.kind() == ErrorKind::NotFound && !sources.contains(root.as_str()) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

fn valid_changed_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_CHANGED_PATH_BYTES
        && !path.starts_with('/')
        && !path.ends_with('/')
        && path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'@' | b'-' | b'/')
        })
        && path
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

fn valid_identifier(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value.as_bytes().first() != Some(&b'-')
        && value.as_bytes().last() != Some(&b'-')
}
