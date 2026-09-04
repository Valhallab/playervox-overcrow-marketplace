use std::{
    collections::BTreeSet,
    fmt, fs,
    io::{Read as _, Write as _},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_PLAN_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 1_000;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AGGREGATE_BYTES: u64 = 16 * 1024 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct SnapshotError;

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("snapshot plan rejected")
    }
}

struct Entry<'a> {
    mode: &'a str,
    size: u64,
    object: &'a str,
    path: &'a str,
}

pub fn write_plan(repository: &Path, revision: &str) -> Result<(), SnapshotError> {
    if !safe_repository(repository) || !valid_object_id(revision) {
        return Err(SnapshotError);
    }
    let repository = repository.to_str().ok_or(SnapshotError)?;
    let mut child = Command::new("/usr/bin/git")
        .args([
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.excludesFile=/dev/null",
            "-c",
            "credential.helper=",
            "-c",
            "protocol.file.allow=never",
            "-c",
            "protocol.ext.allow=never",
            "-C",
            repository,
            "ls-tree",
            "-r",
            "-z",
            "-l",
            "--full-tree",
            revision,
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SnapshotError)?;
    let stdout = child.stdout.take().ok_or(SnapshotError)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_PLAN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| SnapshotError)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SnapshotError);
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let bytes = reader
        .join()
        .map_err(|_| SnapshotError)?
        .map_err(|_| SnapshotError)?;
    if !status.success() || bytes.len() > MAX_PLAN_BYTES {
        return Err(SnapshotError);
    }
    let entries = validate_plan(&bytes)?;
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    for entry in entries {
        writeln!(
            output,
            "{}\t{}\t{}\t{}",
            entry.mode, entry.size, entry.object, entry.path
        )
        .map_err(|_| SnapshotError)?;
    }
    Ok(())
}

fn safe_repository(repository: &Path) -> bool {
    repository.is_absolute()
        && fs::canonicalize(repository).is_ok_and(|canonical| canonical == repository)
        && fs::metadata(repository).is_ok_and(|metadata| metadata.is_dir())
}

fn validate_plan(bytes: &[u8]) -> Result<Vec<Entry<'_>>, SnapshotError> {
    let mut paths = BTreeSet::new();
    let mut aggregate = 0_u64;
    let mut entries = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(SnapshotError)?;
        let header = std::str::from_utf8(&record[..tab]).map_err(|_| SnapshotError)?;
        let mut fields = header.split_ascii_whitespace();
        let mode = fields.next().ok_or(SnapshotError)?;
        let kind = fields.next().ok_or(SnapshotError)?;
        let object = fields.next().ok_or(SnapshotError)?;
        let size = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(SnapshotError)?;
        if fields.next().is_some()
            || !matches!(mode, "100644" | "100755")
            || kind != "blob"
            || !valid_object_id(object)
            || size > MAX_FILE_BYTES
        {
            return Err(SnapshotError);
        }
        let path = std::str::from_utf8(&record[tab + 1..]).map_err(|_| SnapshotError)?;
        if !valid_path(path) || !paths.insert(path) {
            return Err(SnapshotError);
        }
        if paths.len() > MAX_ENTRIES {
            return Err(SnapshotError);
        }
        aggregate = aggregate.checked_add(size).ok_or(SnapshotError)?;
        if aggregate > MAX_AGGREGATE_BYTES {
            return Err(SnapshotError);
        }
        entries.push(Entry {
            mode,
            size,
            object,
            path,
        });
    }
    if entries.is_empty() {
        return Err(SnapshotError);
    }
    Ok(entries)
}

fn valid_object_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && value
            .split('/')
            .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::{SnapshotError, validate_plan};

    const OBJECT: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn plan_accepts_only_bounded_regular_files_with_portable_paths() {
        let valid = format!("100644 blob {OBJECT} 12\twidgets/example/index.html\0");
        let entries = validate_plan(valid.as_bytes()).expect("valid snapshot plan");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "widgets/example/index.html");

        for invalid in [
            format!("120000 blob {OBJECT} 12\twidgets/example/link\0"),
            format!("100644 blob {OBJECT} 12\twidgets/example/bad name\0"),
            format!("100644 blob {OBJECT} 8388609\twidgets/example/large\0"),
        ] {
            assert!(matches!(
                validate_plan(invalid.as_bytes()),
                Err(SnapshotError)
            ));
        }
    }
}
