use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    path::Path,
};

use semver::Version;
use toml::{Table, Value};

use crate::{
    metadata::{TargetSpec, validate_targets},
    package::{read_source_file, validate_source_directory},
};

const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_LOCK_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOML_VALUES: usize = 8_192;
const MAX_TOML_DEPTH: usize = 16;
const MAX_WORKSPACE_MEMBERS: usize = 100;
const MAX_LOCK_PACKAGES: usize = 1_000;
const MAX_DEPENDENCIES: usize = 128;

struct LockIndex {
    local: BTreeSet<(String, String)>,
    registry: BTreeSet<(String, String)>,
}

struct PackageManifest {
    name: String,
    version: String,
    library_name: Option<String>,
    crate_types: Vec<String>,
    dependencies: Vec<DependencySpec>,
}

enum DependencySpec {
    Registry { name: String, version: String },
    Workspace { name: String },
    Path { name: String, member: String },
}

pub(crate) fn load(repository: &Path) -> Result<Vec<TargetSpec>, ()> {
    reject_cargo_config(repository)?;
    let workspace_bytes =
        read_source_file(repository, "Cargo.toml", MAX_MANIFEST_BYTES).map_err(|_| ())?;
    let lock_bytes = read_source_file(repository, "Cargo.lock", MAX_LOCK_BYTES).map_err(|_| ())?;
    let targets_bytes =
        read_source_file(repository, "marketplace/targets.json", 128 * 1024).map_err(|_| ())?;
    let targets = validate_targets(&targets_bytes).map_err(|_| ())?;
    let workspace = parse_table(&workspace_bytes)?;
    let (members, workspace_dependencies) = validate_workspace(&workspace)?;
    let lock = validate_lock(&lock_bytes)?;

    let mut packages = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for member in &members {
        validate_source_directory(repository, member).map_err(|_| ())?;
        reject_member_cargo_config(repository, member)?;
        if repository
            .join(member)
            .join("build.rs")
            .try_exists()
            .map_err(|_| ())?
        {
            return Err(());
        }
        let manifest_path = format!("{member}/Cargo.toml");
        let bytes =
            read_source_file(repository, &manifest_path, MAX_MANIFEST_BYTES).map_err(|_| ())?;
        let manifest = validate_package_manifest(member, &parse_table(&bytes)?)?;
        if !identities.insert((manifest.name.clone(), manifest.version.clone()))
            || packages.insert(member.clone(), manifest).is_some()
        {
            return Err(());
        }
    }

    let local: BTreeSet<_> = packages
        .values()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    if local != lock.local {
        return Err(());
    }

    for package in packages.values() {
        if package.dependencies.len() > MAX_DEPENDENCIES {
            return Err(());
        }
        for dependency in &package.dependencies {
            match dependency {
                DependencySpec::Registry { name, version } => {
                    require_locked_registry(&lock, name, version)?;
                }
                DependencySpec::Workspace { name } => {
                    let version = workspace_dependencies.get(name).ok_or(())?;
                    require_locked_registry(&lock, name, version)?;
                }
                DependencySpec::Path { name, member } => {
                    let dependency_package = packages.get(member).ok_or(())?;
                    if dependency_package.name != *name {
                        return Err(());
                    }
                }
            }
        }
    }

    for target in &targets {
        validate_source_directory(repository, target.source_directory()).map_err(|_| ())?;
        let package = packages.get(target.source_directory()).ok_or(())?;
        let library_name = package.library_name.as_deref().ok_or(())?;
        if package.name != target.cargo_package()
            || library_name != target.component_artifact()
            || package
                .crate_types
                .iter()
                .filter(|crate_type| crate_type.as_str() == "cdylib")
                .count()
                != 1
        {
            return Err(());
        }
    }
    Ok(targets)
}

fn reject_cargo_config(repository: &Path) -> Result<(), ()> {
    for relative in [".cargo/config", ".cargo/config.toml"] {
        match std::fs::symlink_metadata(repository.join(relative)) {
            Ok(_) => return Err(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

fn reject_member_cargo_config(repository: &Path, member: &str) -> Result<(), ()> {
    for relative in [".cargo/config", ".cargo/config.toml"] {
        match std::fs::symlink_metadata(repository.join(member).join(relative)) {
            Ok(_) => return Err(()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

fn parse_table(bytes: &[u8]) -> Result<Table, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let table = toml::from_str::<Table>(text).map_err(|_| ())?;
    let mut values = 0;
    bound_table(&table, 0, &mut values)?;
    Ok(table)
}

fn bound_table(table: &Table, depth: usize, values: &mut usize) -> Result<(), ()> {
    if depth > MAX_TOML_DEPTH || table.len() > 1_024 {
        return Err(());
    }
    for (key, value) in table {
        if key.is_empty() || key.len() > 128 {
            return Err(());
        }
        *values = values.checked_add(1).ok_or(())?;
        if *values > MAX_TOML_VALUES {
            return Err(());
        }
        match value {
            Value::String(value) if value.len() <= 1_024 => {}
            Value::Integer(_) | Value::Boolean(_) => {}
            Value::Array(array) if array.len() <= 1_024 => {
                for value in array {
                    let mut singleton = Table::new();
                    singleton.insert("value".to_owned(), value.clone());
                    bound_table(&singleton, depth + 1, values)?;
                }
            }
            Value::Table(table) => bound_table(table, depth + 1, values)?,
            _ => return Err(()),
        }
    }
    Ok(())
}

fn validate_workspace(table: &Table) -> Result<(BTreeSet<String>, BTreeMap<String, String>), ()> {
    only_keys(table, &["workspace"])?;
    let workspace = table.get("workspace").and_then(Value::as_table).ok_or(())?;
    only_keys(
        workspace,
        &["members", "resolver", "package", "dependencies"],
    )?;
    if workspace.get("resolver").and_then(Value::as_str) != Some("3") {
        return Err(());
    }
    let member_values = workspace
        .get("members")
        .and_then(Value::as_array)
        .ok_or(())?;
    if member_values.is_empty() || member_values.len() > MAX_WORKSPACE_MEMBERS {
        return Err(());
    }
    let mut members = BTreeSet::new();
    for member in member_values {
        let member = member.as_str().ok_or(())?;
        if !valid_relative_path(member) || !members.insert(member.to_owned()) {
            return Err(());
        }
    }
    if let Some(package) = workspace.get("package") {
        let package = package.as_table().ok_or(())?;
        only_keys(package, &["edition", "license", "rust-version"])?;
        if package.get("edition").and_then(Value::as_str) != Some("2024")
            || package.get("license").and_then(Value::as_str) != Some("AGPL-3.0-only")
            || package.get("rust-version").and_then(Value::as_str) != Some("1.98")
        {
            return Err(());
        }
    }
    let mut dependencies = BTreeMap::new();
    if let Some(value) = workspace.get("dependencies") {
        let table = value.as_table().ok_or(())?;
        if table.len() > MAX_DEPENDENCIES {
            return Err(());
        }
        for (name, value) in table {
            if !valid_package_name(name) {
                return Err(());
            }
            let version = registry_dependency(value)?;
            if dependencies.insert(name.clone(), version).is_some() {
                return Err(());
            }
        }
    }
    Ok((members, dependencies))
}

fn validate_package_manifest(member: &str, table: &Table) -> Result<PackageManifest, ()> {
    only_keys(
        table,
        &[
            "package",
            "lib",
            "dependencies",
            "dev-dependencies",
            "features",
            "lints",
            "target",
        ],
    )?;
    let package = table.get("package").and_then(Value::as_table).ok_or(())?;
    only_keys(
        package,
        &[
            "name",
            "version",
            "edition",
            "license",
            "rust-version",
            "autotests",
        ],
    )?;
    let name = package.get("name").and_then(Value::as_str).ok_or(())?;
    let version = package.get("version").and_then(Value::as_str).ok_or(())?;
    if !valid_package_name(name) || Version::parse(version).is_err() {
        return Err(());
    }
    require_edition(package.get("edition"))?;
    require_workspace_or_string(package.get("license"), "AGPL-3.0-only")?;
    require_workspace_or_string(package.get("rust-version"), "1.98")?;
    if package
        .get("autotests")
        .is_some_and(|value| value.as_bool() != Some(false))
    {
        return Err(());
    }
    validate_reviewed_sdk_features(member, table.get("features"))?;

    let (library_name, crate_types) = if let Some(value) = table.get("lib") {
        let library = value.as_table().ok_or(())?;
        only_keys(library, &["name", "crate-type"])?;
        let library_name = library
            .get("name")
            .map(|value| value.as_str().ok_or(()))
            .transpose()?
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        if !valid_artifact_name(&library_name) {
            return Err(());
        }
        let crate_types = library
            .get("crate-type")
            .and_then(Value::as_array)
            .ok_or(())?;
        if crate_types.is_empty() || crate_types.len() > 2 {
            return Err(());
        }
        let mut seen = BTreeSet::new();
        let mut decoded = Vec::new();
        for crate_type in crate_types {
            let crate_type = crate_type.as_str().ok_or(())?;
            if !matches!(crate_type, "cdylib" | "rlib") || !seen.insert(crate_type) {
                return Err(());
            }
            decoded.push(crate_type.to_owned());
        }
        (Some(library_name), decoded)
    } else {
        (None, Vec::new())
    };

    if let Some(lints) = table.get("lints") {
        let lints = lints.as_table().ok_or(())?;
        only_keys(lints, &["rust"])?;
        let rust = lints.get("rust").and_then(Value::as_table).ok_or(())?;
        only_keys(rust, &["unsafe_code"])?;
        if rust.get("unsafe_code").and_then(Value::as_str) != Some("forbid") {
            return Err(());
        }
    }

    let mut dependencies = Vec::new();
    for section in ["dependencies", "dev-dependencies"] {
        let Some(value) = table.get(section) else {
            continue;
        };
        let entries = value.as_table().ok_or(())?;
        if entries.len() > MAX_DEPENDENCIES {
            return Err(());
        }
        for (dependency_name, value) in entries {
            dependencies.push(parse_member_dependency(member, dependency_name, value)?);
        }
    }
    if let Some(target) = table.get("target") {
        dependencies.extend(validate_reviewed_sdk_target(member, target)?);
    }
    Ok(PackageManifest {
        name: name.to_owned(),
        version: version.to_owned(),
        library_name,
        crate_types,
        dependencies,
    })
}

fn validate_reviewed_sdk_features(member: &str, value: Option<&Value>) -> Result<(), ()> {
    if member != "sdk/rust/overcrow-widget-sdk" {
        return if value.is_none() { Ok(()) } else { Err(()) };
    }
    let Some(value) = value else {
        return Ok(());
    };
    let features = value.as_table().ok_or(())?;
    only_keys(features, &["default", "api-v1", "api-v2"])?;
    let default = features
        .get("default")
        .and_then(Value::as_array)
        .ok_or(())?;
    let api_v1 = features.get("api-v1").and_then(Value::as_array).ok_or(())?;
    let api_v2 = features.get("api-v2").and_then(Value::as_array).ok_or(())?;
    if default.len() != 1
        || default[0].as_str() != Some("api-v1")
        || !api_v1.is_empty()
        || !api_v2.is_empty()
    {
        return Err(());
    }
    Ok(())
}

fn validate_reviewed_sdk_target(member: &str, value: &Value) -> Result<Vec<DependencySpec>, ()> {
    if member != "sdk/rust/overcrow-widget-sdk" {
        return Err(());
    }
    let targets = value.as_table().ok_or(())?;
    only_keys(targets, &["cfg(target_arch = \"wasm32\")"])?;
    let target = targets
        .get("cfg(target_arch = \"wasm32\")")
        .and_then(Value::as_table)
        .ok_or(())?;
    only_keys(target, &["dependencies"])?;
    let dependencies = target
        .get("dependencies")
        .and_then(Value::as_table)
        .ok_or(())?;
    if dependencies.len() != 2
        || !dependencies.contains_key("dlmalloc")
        || !dependencies.contains_key("rlibc")
    {
        return Err(());
    }
    dependencies
        .iter()
        .map(|(name, value)| {
            if value.as_table().is_none_or(|table| !workspace_true(table)) {
                return Err(());
            }
            Ok(DependencySpec::Workspace { name: name.clone() })
        })
        .collect()
}

fn parse_member_dependency(member: &str, name: &str, value: &Value) -> Result<DependencySpec, ()> {
    if !valid_package_name(name) {
        return Err(());
    }
    if value.is_str() {
        return Ok(DependencySpec::Registry {
            name: name.to_owned(),
            version: registry_dependency(value)?,
        });
    }
    let table = value.as_table().ok_or(())?;
    if table.contains_key("workspace") {
        only_keys(table, &["workspace", "default-features", "features"])?;
        if table.get("workspace").and_then(Value::as_bool) != Some(true) {
            return Err(());
        }
        validate_dependency_options(table)?;
        return Ok(DependencySpec::Workspace {
            name: name.to_owned(),
        });
    }
    if let Some(path) = table.get("path") {
        only_keys(table, &["path", "default-features", "features"])?;
        let path = path.as_str().ok_or(())?;
        let dependency_member = normalize_dependency_path(member, path)?;
        if table.len() > 1 {
            validate_dependency_options(table)?;
            let features = table.get("features").and_then(Value::as_array).ok_or(())?;
            if name != "overcrow-widget-sdk"
                || dependency_member != "sdk/rust/overcrow-widget-sdk"
                || table.get("default-features").and_then(Value::as_bool) != Some(false)
                || features.len() != 1
                || !matches!(features[0].as_str(), Some("api-v1" | "api-v2"))
            {
                return Err(());
            }
        }
        return Ok(DependencySpec::Path {
            name: name.to_owned(),
            member: dependency_member,
        });
    }
    Ok(DependencySpec::Registry {
        name: name.to_owned(),
        version: registry_dependency(value)?,
    })
}

fn registry_dependency(value: &Value) -> Result<String, ()> {
    let version = if let Some(version) = value.as_str() {
        version
    } else {
        let table = value.as_table().ok_or(())?;
        only_keys(table, &["version", "default-features", "features"])?;
        validate_dependency_options(table)?;
        table.get("version").and_then(Value::as_str).ok_or(())?
    };
    let exact = version.strip_prefix('=').ok_or(())?;
    let parsed = Version::parse(exact).map_err(|_| ())?;
    if version != format!("={parsed}") {
        return Err(());
    }
    Ok(exact.to_owned())
}

fn validate_dependency_options(table: &Table) -> Result<(), ()> {
    if table
        .get("default-features")
        .is_some_and(|value| value.as_bool().is_none())
    {
        return Err(());
    }
    if let Some(features) = table.get("features") {
        let features = features.as_array().ok_or(())?;
        if features.len() > 32
            || features.iter().any(|feature| {
                feature
                    .as_str()
                    .is_none_or(|feature| !valid_feature(feature))
            })
        {
            return Err(());
        }
    }
    Ok(())
}

fn validate_lock(bytes: &[u8]) -> Result<LockIndex, ()> {
    let table = parse_table(bytes)?;
    only_keys(&table, &["version", "package"])?;
    if table.get("version").and_then(Value::as_integer) != Some(4) {
        return Err(());
    }
    let packages = table.get("package").and_then(Value::as_array).ok_or(())?;
    if packages.is_empty() || packages.len() > MAX_LOCK_PACKAGES {
        return Err(());
    }
    let mut local = BTreeSet::new();
    let mut registry = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for package in packages {
        let package = package.as_table().ok_or(())?;
        only_keys(
            package,
            &["name", "version", "source", "checksum", "dependencies"],
        )?;
        let name = package.get("name").and_then(Value::as_str).ok_or(())?;
        let version = package.get("version").and_then(Value::as_str).ok_or(())?;
        if !valid_package_name(name) || Version::parse(version).is_err() {
            return Err(());
        }
        if let Some(dependencies) = package.get("dependencies") {
            let dependencies = dependencies.as_array().ok_or(())?;
            if dependencies.len() > MAX_DEPENDENCIES
                || dependencies.iter().any(|dependency| {
                    dependency
                        .as_str()
                        .is_none_or(|dependency| dependency.is_empty() || dependency.len() > 256)
                })
            {
                return Err(());
            }
        }
        let source = package
            .get("source")
            .map(|value| value.as_str().ok_or(()))
            .transpose()?;
        let identity = (
            name.to_owned(),
            version.to_owned(),
            source.map(str::to_owned),
        );
        if !identities.insert(identity) {
            return Err(());
        }
        match source {
            Some(CRATES_IO_SOURCE) => {
                let checksum = package.get("checksum").and_then(Value::as_str).ok_or(())?;
                if !valid_sha256(checksum)
                    || !registry.insert((name.to_owned(), version.to_owned()))
                {
                    return Err(());
                }
            }
            Some(_) => return Err(()),
            None => {
                if package.contains_key("checksum")
                    || !local.insert((name.to_owned(), version.to_owned()))
                {
                    return Err(());
                }
            }
        }
    }
    Ok(LockIndex { local, registry })
}

fn require_locked_registry(lock: &LockIndex, name: &str, version: &str) -> Result<(), ()> {
    let matches = lock
        .registry
        .iter()
        .filter(|(locked_name, locked_version)| {
            locked_name == name
                && locked_version
                    .split_once('+')
                    .map_or(locked_version.as_str(), |pair| pair.0)
                    == version
        })
        .count();
    if matches == 1 { Ok(()) } else { Err(()) }
}

fn require_edition(value: Option<&Value>) -> Result<(), ()> {
    match value {
        Some(Value::String(value)) if value == "2024" => Ok(()),
        Some(Value::Table(table)) if workspace_true(table) => Ok(()),
        _ => Err(()),
    }
}

fn require_workspace_or_string(value: Option<&Value>, expected: &str) -> Result<(), ()> {
    match value {
        None => Ok(()),
        Some(Value::String(value)) if value == expected => Ok(()),
        Some(Value::Table(table)) if workspace_true(table) => Ok(()),
        _ => Err(()),
    }
}

fn workspace_true(table: &Table) -> bool {
    table.len() == 1 && table.get("workspace").and_then(Value::as_bool) == Some(true)
}

fn normalize_dependency_path(member: &str, dependency: &str) -> Result<String, ()> {
    if dependency.is_empty() || dependency.starts_with('/') || dependency.contains('\\') {
        return Err(());
    }
    let mut components: Vec<&str> = member.split('/').collect();
    for component in dependency.split('/') {
        match component {
            "" => return Err(()),
            "." => {}
            ".." => {
                components.pop().ok_or(())?;
            }
            component if valid_path_component(component) => components.push(component),
            _ => return Err(()),
        }
    }
    if components.is_empty() {
        return Err(());
    }
    Ok(components.join("/"))
}

fn only_keys(table: &Table, allowed: &[&str]) -> Result<(), ()> {
    if table.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(())
    }
}

fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && !value.starts_with('/')
        && !value.ends_with('/')
        && value.split('/').all(valid_path_component)
}

fn valid_path_component(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_package_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn valid_artifact_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_feature(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'/' | b':' | b'?')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
