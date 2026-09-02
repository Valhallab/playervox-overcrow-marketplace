use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, de};
use url::{Host, Url};
use wasmparser::{ComponentTypeRef, Encoding, Parser, Payload};
use wit_component::{DecodedWasm, WitPrinter};

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_LISTING_BYTES: usize = 64 * 1024;
const MAX_TARGETS_BYTES: usize = 128 * 1024;
const MAX_TARGETS: usize = 500;
const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILE_PATH_BYTES: usize = 192;
const MAX_PACKAGE_ENTRIES: usize = 64;
const MAX_MANIFEST_LOCALES: usize = 32;
const MAX_MANIFEST_GAMES: usize = 32;
const MAX_MANIFEST_DEPENDENCIES: usize = 32;
const MAX_HTTP_HOSTS: usize = 16;
const MAX_LISTING_LOCALIZATIONS: usize = 32;
const EXPECTED_WIT_V1: &str = include_str!("../../../wit/widget-v1.wit");
const EXPECTED_WIT_V2: &str = include_str!("../../../wit/widget-v2.wit");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PolicyCode {
    Manifest,
    Listing,
    LocaleMismatch,
    Targets,
    Component,
    ForbiddenImport,
    ForbiddenExport,
}

#[derive(Debug)]
pub(crate) struct PolicyError(PolicyCode);

impl PolicyError {
    const fn new(code: PolicyCode) -> Self {
        Self(code)
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            PolicyCode::Manifest => "invalid extension manifest",
            PolicyCode::Listing => "invalid marketplace listing",
            PolicyCode::LocaleMismatch => "listing locales do not match the manifest",
            PolicyCode::Targets => "invalid marketplace targets",
            PolicyCode::Component => "invalid WebAssembly component",
            PolicyCode::ForbiddenImport => "component imports are forbidden",
            PolicyCode::ForbiddenExport => "component exports do not match its declared widget API",
        })
    }
}

impl Error for PolicyError {}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    schema_version: u32,
    id: String,
    version: String,
    kind: PackageKind,
    api_version: String,
    default_locale: String,
    available_locales: Vec<String>,
    games: Vec<GameScope>,
    dependencies: Vec<Dependency>,
    capabilities: Capabilities,
    display: Option<DisplayDefaults>,
    files: PackageFiles,
}

impl Manifest {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn api_version(&self) -> &str {
        &self.api_version
    }

    pub(crate) const fn kind(&self) -> PackageKind {
        self.kind
    }

    pub(crate) fn files(&self) -> &PackageFiles {
        &self.files
    }

    pub(crate) fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PackageKind {
    Widget,
    Provider,
    Bundle,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GameScope {
    platform: String,
    id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Dependency {
    id: String,
    version: String,
    sha256: String,
}

impl Dependency {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Capabilities {
    #[serde(default, rename = "http")]
    http_hosts: Vec<String>,
    #[serde(default)]
    game_data: Vec<String>,
    #[serde(default)]
    storage: bool,
    #[serde(default)]
    clipboard_write: bool,
    #[serde(default)]
    provider: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DisplayDefaults {
    show_in_passive: bool,
    x_milli: u32,
    y_milli: u32,
    width: u32,
    height: u32,
    scale_milli: u32,
    transparent: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackageFiles {
    component: DeclaredFile,
    #[serde(deserialize_with = "deserialize_unique_file_map")]
    locales: BTreeMap<String, DeclaredFile>,
    #[serde(deserialize_with = "deserialize_unique_file_map")]
    assets: BTreeMap<String, DeclaredFile>,
}

impl PackageFiles {
    pub(crate) fn component(&self) -> &DeclaredFile {
        &self.component
    }

    pub(crate) fn locales(&self) -> &BTreeMap<String, DeclaredFile> {
        &self.locales
    }

    pub(crate) fn assets(&self) -> &BTreeMap<String, DeclaredFile> {
        &self.assets
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeclaredFile {
    path: String,
    sha256: String,
}

impl DeclaredFile {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Listing {
    author: String,
    spdx_license: String,
    source_url: String,
    localizations: Vec<Localization>,
    #[serde(skip_serializing)]
    preview_file: Option<String>,
}

impl Listing {
    pub(crate) fn author(&self) -> &str {
        &self.author
    }

    pub(crate) fn spdx_license(&self) -> &str {
        &self.spdx_license
    }

    pub(crate) fn source_url(&self) -> &str {
        &self.source_url
    }

    pub(crate) fn localizations(&self) -> &[Localization] {
        &self.localizations
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Localization {
    locale: String,
    name: String,
    description: String,
}

impl Localization {
    pub(crate) fn locale(&self) -> &str {
        &self.locale
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn description(&self) -> &str {
        &self.description
    }
}

#[derive(Clone)]
pub(crate) struct ValidatedMetadata {
    manifest: Manifest,
    listing: Listing,
    preview_file: Option<String>,
}

impl ValidatedMetadata {
    pub(crate) fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub(crate) fn listing(&self) -> &Listing {
        &self.listing
    }

    pub(crate) fn preview_file(&self) -> Option<&str> {
        self.preview_file.as_deref()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TargetSpec {
    source_directory: String,
    cargo_package: String,
    component_artifact: String,
    status: CatalogStatus,
}

impl TargetSpec {
    pub(crate) fn source_directory(&self) -> &str {
        &self.source_directory
    }

    pub(crate) const fn status(&self) -> CatalogStatus {
        self.status
    }

    pub(crate) fn cargo_package(&self) -> &str {
        &self.cargo_package
    }

    pub(crate) fn component_artifact(&self) -> &str {
        &self.component_artifact
    }

    pub(crate) fn build_plan_entry(&self) -> BuildPlanEntry<'_> {
        BuildPlanEntry {
            cargo_package: &self.cargo_package,
            component_artifact: &self.component_artifact,
            source_directory: &self.source_directory,
        }
    }
}

pub(crate) struct BuildPlanEntry<'a> {
    pub(crate) cargo_package: &'a str,
    pub(crate) component_artifact: &'a str,
    pub(crate) source_directory: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BuildBindings {
    schema_version: u32,
    components: Vec<ComponentBinding>,
    providers: Vec<ProviderBinding>,
}

impl BuildBindings {
    pub(crate) fn components(&self) -> &[ComponentBinding] {
        &self.components
    }

    pub(crate) fn providers(&self) -> &[ProviderBinding] {
        &self.providers
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComponentBinding {
    source_directory: String,
    sha256: String,
}

impl ComponentBinding {
    pub(crate) fn source_directory(&self) -> &str {
        &self.source_directory
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProviderBinding {
    id: String,
    version: String,
    sha256: String,
}

impl ProviderBinding {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CatalogStatus {
    Verified,
    SecuritySuspended,
    Revoked,
}

pub(crate) fn validate_metadata(
    manifest_bytes: &[u8],
    listing_bytes: &[u8],
) -> Result<ValidatedMetadata, PolicyError> {
    if manifest_bytes.len() > MAX_MANIFEST_BYTES || listing_bytes.len() > MAX_LISTING_BYTES {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)
        .map_err(|_| PolicyError::new(PolicyCode::Manifest))?;
    let manifest = validate_manifest(manifest)?;
    let listing: Listing =
        serde_json::from_slice(listing_bytes).map_err(|_| PolicyError::new(PolicyCode::Listing))?;
    validate_listing(listing, manifest)
}

pub(crate) fn validate_targets(bytes: &[u8]) -> Result<Vec<TargetSpec>, PolicyCode> {
    if bytes.len() > MAX_TARGETS_BYTES {
        return Err(PolicyCode::Targets);
    }
    let mut targets: Vec<TargetSpec> =
        serde_json::from_slice(bytes).map_err(|_| PolicyCode::Targets)?;
    if targets.len() > MAX_TARGETS {
        return Err(PolicyCode::Targets);
    }
    let mut paths = BTreeSet::new();
    let mut packages = BTreeSet::new();
    let mut artifacts = BTreeSet::new();
    for target in &targets {
        if !valid_package_path(&target.source_directory)
            || !paths.insert(target.source_directory.clone())
            || !valid_cargo_package(&target.cargo_package)
            || !packages.insert(target.cargo_package.clone())
            || !valid_component_artifact(&target.component_artifact)
            || !artifacts.insert(target.component_artifact.clone())
        {
            return Err(PolicyCode::Targets);
        }
    }
    targets.sort_by(|left, right| left.source_directory.cmp(&right.source_directory));
    Ok(targets)
}

pub(crate) fn validate_build_bindings(bytes: &[u8]) -> Result<BuildBindings, PolicyCode> {
    if bytes.len() > MAX_TARGETS_BYTES {
        return Err(PolicyCode::Targets);
    }
    let bindings: BuildBindings = serde_json::from_slice(bytes).map_err(|_| PolicyCode::Targets)?;
    if bindings.schema_version != 1
        || bindings.components.len() > MAX_TARGETS
        || bindings.providers.len() > MAX_MANIFEST_DEPENDENCIES * MAX_TARGETS
    {
        return Err(PolicyCode::Targets);
    }
    let mut components = BTreeSet::new();
    for binding in &bindings.components {
        if !valid_package_path(&binding.source_directory)
            || !valid_sha256(&binding.sha256)
            || !components.insert(binding.source_directory.clone())
        {
            return Err(PolicyCode::Targets);
        }
    }
    let mut providers = BTreeSet::new();
    for binding in &bindings.providers {
        let version = Version::parse(&binding.version).map_err(|_| PolicyCode::Targets)?;
        if !valid_extension_id(&binding.id)
            || version.to_string() != binding.version
            || !valid_sha256(&binding.sha256)
            || !providers.insert((binding.id.clone(), binding.version.clone()))
        {
            return Err(PolicyCode::Targets);
        }
    }
    Ok(bindings)
}

pub(crate) fn bind_manifest_digests(
    manifest_bytes: &[u8],
    listing_bytes: &[u8],
    component_sha256: &str,
    providers: &BTreeMap<(String, String), String>,
) -> Result<Vec<u8>, PolicyError> {
    if !valid_sha256(component_sha256) {
        return Err(PolicyError::new(PolicyCode::Targets));
    }
    let mut metadata = validate_metadata(manifest_bytes, listing_bytes)?;
    metadata.manifest.files.component.sha256 = component_sha256.to_owned();
    for dependency in &mut metadata.manifest.dependencies {
        dependency.sha256 = providers
            .get(&(dependency.id.clone(), dependency.version.clone()))
            .ok_or_else(|| PolicyError::new(PolicyCode::Targets))?
            .clone();
    }
    let mut encoded = serde_json::to_vec_pretty(&metadata.manifest)
        .map_err(|_| PolicyError::new(PolicyCode::Manifest))?;
    encoded.push(b'\n');
    Ok(encoded)
}

pub(crate) fn inspect_component(bytes: &[u8]) -> Result<(), PolicyCode> {
    inspect_component_for_versions(bytes, &["1", "2"])
}

pub(crate) fn inspect_component_for_api(bytes: &[u8], api_version: &str) -> Result<(), PolicyCode> {
    inspect_component_for_versions(bytes, &[api_version])
}

fn inspect_component_for_versions(bytes: &[u8], api_versions: &[&str]) -> Result<(), PolicyCode> {
    if bytes.len() > MAX_COMPONENT_BYTES {
        return Err(PolicyCode::Component);
    }
    let mut root_encoding = None;
    let mut depth = 0_u32;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|_| PolicyCode::Component)? {
            Payload::Version { encoding, .. } if root_encoding.is_none() => {
                root_encoding = Some(encoding);
            }
            Payload::ComponentImportSection(section) if depth == 0 => {
                for import in section {
                    let import = import.map_err(|_| PolicyCode::Component)?;
                    if !matches!(import.ty, ComponentTypeRef::Type(_)) {
                        return Err(PolicyCode::ForbiddenImport);
                    }
                }
            }
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    if root_encoding != Some(Encoding::Component) {
        return Err(PolicyCode::Component);
    }
    let (actual_resolve, actual_world) = match wit_component::decode(bytes) {
        Ok(DecodedWasm::Component(resolve, world)) => (resolve, world),
        _ => return Err(PolicyCode::Component),
    };

    let actual_package = actual_resolve.worlds[actual_world]
        .package
        .ok_or(PolicyCode::Component)?;
    let actual_world_name = actual_resolve.worlds[actual_world].name.as_str();
    let actual = print_wit(&actual_resolve, actual_package)?;
    if api_versions
        .iter()
        .any(|api_version| !matches!(*api_version, "1" | "2"))
    {
        return Err(PolicyCode::Manifest);
    }
    for api_version in api_versions {
        let (file_name, expected_world_name, expected_source) = match *api_version {
            "1" => ("widget-v1.wit", "widget-v1", EXPECTED_WIT_V1),
            "2" => ("widget-v2.wit", "widget-v2", EXPECTED_WIT_V2),
            _ => return Err(PolicyCode::Manifest),
        };
        let mut expected_resolve = wit_parser::Resolve::default();
        let expected_package = expected_resolve
            .push_str(file_name, expected_source)
            .map_err(|_| PolicyCode::Component)?;
        let expected = print_wit(&expected_resolve, expected_package)?;
        if world_body(&actual, actual_world_name) == world_body(&expected, expected_world_name) {
            return Ok(());
        }
    }
    Err(PolicyCode::ForbiddenExport)
}

fn world_body<'a>(document: &'a str, world_name: &str) -> Option<&'a str> {
    let marker = format!("world {world_name} {{");
    let body_start = document.find(&marker)?.checked_add(marker.len())?;
    let bytes = document.as_bytes();
    let mut depth = 1_usize;
    let mut cursor = body_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => depth = depth.checked_add(1)?,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return document.get(body_start..cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn print_wit(
    resolve: &wit_parser::Resolve,
    package: wit_parser::PackageId,
) -> Result<String, PolicyCode> {
    let mut printer = WitPrinter::default();
    printer
        .print(resolve, package, &[])
        .map_err(|_| PolicyCode::Component)?;
    Ok(printer.output.to_string())
}

fn validate_manifest(mut manifest: Manifest) -> Result<Manifest, PolicyError> {
    if manifest.schema_version != 1
        || !valid_extension_id(&manifest.id)
        || !matches!(manifest.api_version.as_str(), "1" | "2")
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    manifest.version = Version::parse(&manifest.version)
        .map_err(|_| PolicyError::new(PolicyCode::Manifest))?
        .to_string();
    if !valid_locale(&manifest.default_locale)
        || manifest.available_locales.is_empty()
        || manifest.available_locales.len() > MAX_MANIFEST_LOCALES
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let locale_count = manifest.available_locales.len();
    manifest.available_locales = manifest
        .available_locales
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if manifest.available_locales.len() != locale_count
        || manifest
            .available_locales
            .iter()
            .any(|locale| !valid_locale(locale))
        || manifest
            .available_locales
            .binary_search(&manifest.default_locale)
            .is_err()
        || manifest
            .available_locales
            .binary_search_by(|locale| locale.as_str().cmp("en"))
            .is_err()
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    validate_games(&mut manifest.games)?;
    validate_dependencies(&mut manifest.dependencies)?;
    validate_capabilities(&mut manifest.capabilities)?;
    validate_display(manifest.kind, manifest.display.as_ref())?;
    validate_files(&manifest.files, &manifest.available_locales)?;
    Ok(manifest)
}

fn validate_games(values: &mut Vec<GameScope>) -> Result<(), PolicyError> {
    if values.len() > MAX_MANIFEST_GAMES {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let mut ids = BTreeSet::new();
    for value in values.iter() {
        let id = value
            .id
            .parse::<u32>()
            .ok()
            .filter(|id| *id != 0 && value.id == id.to_string())
            .ok_or_else(|| PolicyError::new(PolicyCode::Manifest))?;
        if value.platform != "steam" || !ids.insert(id) {
            return Err(PolicyError::new(PolicyCode::Manifest));
        }
    }
    *values = ids
        .into_iter()
        .map(|id| GameScope {
            platform: "steam".to_owned(),
            id: id.to_string(),
        })
        .collect();
    Ok(())
}

fn validate_dependencies(values: &mut [Dependency]) -> Result<(), PolicyError> {
    if values.len() > MAX_MANIFEST_DEPENDENCIES {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let mut seen = BTreeSet::new();
    for value in values.iter_mut() {
        if !valid_extension_id(&value.id)
            || !seen.insert(value.id.clone())
            || !valid_sha256(&value.sha256)
        {
            return Err(PolicyError::new(PolicyCode::Manifest));
        }
        value.version = Version::parse(&value.version)
            .map_err(|_| PolicyError::new(PolicyCode::Manifest))?
            .to_string();
    }
    values.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(())
}

fn validate_capabilities(capabilities: &mut Capabilities) -> Result<(), PolicyError> {
    if capabilities.http_hosts.len() > MAX_HTTP_HOSTS || capabilities.game_data.len() > 1 {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let http_count = capabilities.http_hosts.len();
    capabilities.http_hosts = std::mem::take(&mut capabilities.http_hosts)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let game_count = capabilities.game_data.len();
    capabilities.game_data = std::mem::take(&mut capabilities.game_data)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if capabilities.http_hosts.len() != http_count
        || capabilities
            .http_hosts
            .iter()
            .any(|host| !valid_http_host(host))
        || capabilities.game_data.len() != game_count
        || capabilities
            .game_data
            .iter()
            .any(|value| value != "overcrow.session.v1")
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    Ok(())
}

fn validate_display(
    kind: PackageKind,
    display: Option<&DisplayDefaults>,
) -> Result<(), PolicyError> {
    let required = kind != PackageKind::Provider;
    let Some(display) = display else {
        return if required {
            Err(PolicyError::new(PolicyCode::Manifest))
        } else {
            Ok(())
        };
    };
    if !required
        || display.x_milli > 1_000
        || display.y_milli > 1_000
        || !(280..=900).contains(&display.width)
        || !(160..=900).contains(&display.height)
        || !(750..=1_750).contains(&display.scale_milli)
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    Ok(())
}

fn validate_files(files: &PackageFiles, locales: &[String]) -> Result<(), PolicyError> {
    if 2usize
        .checked_add(files.locales.len())
        .and_then(|count| count.checked_add(files.assets.len()))
        .is_none_or(|count| count > MAX_PACKAGE_ENTRIES)
    {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    validate_file(&files.component)?;
    if files.component.path != "component.wasm" {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    let mut paths = BTreeSet::from(["manifest.json".to_owned(), files.component.path.clone()]);
    for (locale, file) in &files.locales {
        validate_file(file)?;
        if !valid_locale(locale) || !paths.insert(file.path.clone()) {
            return Err(PolicyError::new(PolicyCode::Manifest));
        }
    }
    if files.locales.keys().ne(locales.iter()) {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    for (id, file) in &files.assets {
        validate_file(file)?;
        if !valid_asset_id(id) || !file.path.ends_with(".png") || !paths.insert(file.path.clone()) {
            return Err(PolicyError::new(PolicyCode::Manifest));
        }
    }
    Ok(())
}

fn validate_file(file: &DeclaredFile) -> Result<(), PolicyError> {
    if !valid_package_path(&file.path) || !valid_sha256(&file.sha256) {
        return Err(PolicyError::new(PolicyCode::Manifest));
    }
    Ok(())
}

fn validate_listing(
    listing: Listing,
    manifest: Manifest,
) -> Result<ValidatedMetadata, PolicyError> {
    if !valid_plain_text(&listing.author, 128)
        || !valid_spdx_license(&listing.spdx_license)
        || !canonical_source_url(&listing.source_url)
        || listing.localizations.is_empty()
        || listing.localizations.len() > MAX_LISTING_LOCALIZATIONS
        || listing
            .preview_file
            .as_deref()
            .is_some_and(|path| !valid_package_path(path) || !path.ends_with(".png"))
        || (manifest.kind == PackageKind::Provider && listing.preview_file.is_some())
    {
        return Err(PolicyError::new(PolicyCode::Listing));
    }
    let mut locales = BTreeSet::new();
    for localization in &listing.localizations {
        if !valid_locale(&localization.locale)
            || !manifest.available_locales.contains(&localization.locale)
            || !locales.insert(localization.locale.clone())
            || !valid_plain_text(&localization.name, 128)
            || !valid_plain_text(&localization.description, 512)
        {
            return Err(PolicyError::new(PolicyCode::LocaleMismatch));
        }
    }
    if locales.iter().ne(manifest.available_locales.iter()) {
        return Err(PolicyError::new(PolicyCode::LocaleMismatch));
    }
    Ok(ValidatedMetadata {
        manifest,
        preview_file: listing.preview_file.clone(),
        listing,
    })
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

fn canonical_source_url(value: &str) -> bool {
    if value.len() > 512 || !value.is_ascii() || value.contains(['\\', '%']) || value.ends_with('/')
    {
        return false;
    }
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.as_str() == value
        && url.scheme() == "https"
        && matches!(url.host(), Some(Host::Domain(_)))
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() != "/"
        && url
            .path()
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn valid_extension_id(value: &str) -> bool {
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

fn valid_cargo_package(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_component_artifact(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
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

fn valid_asset_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_package_path(path: &str) -> bool {
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

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_http_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || !host.is_ascii() || !host.contains('.') {
        return false;
    }
    let Ok(url) = Url::parse(&format!("https://{host}/")) else {
        return false;
    };
    matches!(url.host(), Some(Host::Domain(domain)) if domain == host)
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

fn deserialize_unique_file_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, DeclaredFile>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UniqueFiles;
    impl<'de> de::Visitor<'de> for UniqueFiles {
        type Value = BTreeMap<String, DeclaredFile>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded package file map")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: de::MapAccess<'de>,
        {
            let mut files = BTreeMap::new();
            while let Some((name, file)) = map.next_entry()? {
                if files.len() == MAX_PACKAGE_ENTRIES || files.insert(name, file).is_some() {
                    return Err(de::Error::custom(
                        "duplicate or excessive package file entry",
                    ));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(UniqueFiles)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{PolicyCode, inspect_component, validate_metadata, validate_targets};

    const MANIFEST: &str = include_str!("../../../examples/hello-widget/manifest.json");

    fn manifest() -> Value {
        serde_json::from_str(MANIFEST).expect("manifest JSON")
    }

    fn listing() -> Value {
        json!({
            "author": "PlayerVox",
            "spdxLicense": "AGPL-3.0-only",
            "sourceUrl": "https://github.com/PlayerVox/playervox-overcrow-marketplace",
            "localizations": [
                {"locale": "en", "name": "Hello Widget", "description": "A safe example."},
                {"locale": "fr", "name": "Widget Bonjour", "description": "Un exemple sûr."}
            ],
            "previewFile": "preview.png"
        })
    }

    fn validate(manifest: &Value, listing: &Value) -> Result<(), PolicyCode> {
        validate_metadata(
            &serde_json::to_vec(manifest).expect("manifest JSON"),
            &serde_json::to_vec(listing).expect("listing JSON"),
        )
        .map(|_| ())
        .map_err(|error| error.0)
    }

    fn validate_listing(value: &Value) -> Result<(), PolicyCode> {
        validate(&manifest(), value)
    }

    fn metadata_with_locales(locales: &[&str]) -> (Value, Value) {
        let mut manifest = manifest();
        manifest["defaultLocale"] = json!("en");
        manifest["availableLocales"] = json!(locales);
        manifest["files"]["locales"] = Value::Object(
            locales
                .iter()
                .map(|locale| {
                    (
                        (*locale).to_owned(),
                        json!({
                            "path": format!("locales/{locale}.json"),
                            "sha256": "a".repeat(64)
                        }),
                    )
                })
                .collect(),
        );

        let mut listing = listing();
        listing["localizations"] = Value::Array(
            locales
                .iter()
                .map(|locale| {
                    json!({
                        "locale": locale,
                        "name": format!("Name {locale}"),
                        "description": format!("Description {locale}")
                    })
                })
                .collect(),
        );
        (manifest, listing)
    }

    #[test]
    fn manifest_requires_english_locale() {
        let mut manifest = manifest();
        manifest["defaultLocale"] = json!("fr");
        manifest["availableLocales"] = json!(["fr"]);
        manifest["files"]["locales"]
            .as_object_mut()
            .expect("locale file map")
            .remove("en");

        let mut listing = listing();
        listing["localizations"] = json!([
            {"locale": "fr", "name": "Widget Bonjour", "description": "Un exemple sûr."}
        ]);

        assert_eq!(validate(&manifest, &listing), Err(PolicyCode::Manifest));
    }

    #[test]
    fn manifest_allows_empty_game_scope() {
        let mut manifest = manifest();
        manifest["games"] = json!([]);

        validate(&manifest, &listing()).expect("empty game scope means all games");
    }

    #[test]
    fn listing_accepts_only_the_narrow_projection() {
        validate_listing(&listing()).expect("narrow listing");
        for field in [
            "id",
            "version",
            "kind",
            "apiVersion",
            "defaultLocale",
            "availableLocales",
            "games",
            "dependencies",
            "capabilities",
            "display",
            "files",
            "permissions",
            "status",
        ] {
            let mut value = listing();
            value
                .as_object_mut()
                .expect("object")
                .insert(field.into(), json!(null));
            assert_eq!(
                validate_listing(&value),
                Err(PolicyCode::Listing),
                "field {field}"
            );
        }
    }

    #[test]
    fn listing_locales_must_exactly_match_manifest() {
        let mut subset = listing();
        subset["localizations"] = json!([
            {"locale": "en", "name": "Hello", "description": "English"}
        ]);

        assert_eq!(validate_listing(&subset), Err(PolicyCode::LocaleMismatch));
    }

    #[test]
    fn listing_accepts_thirty_two_localizations() {
        let locales = [
            "en", "aa", "ab", "ac", "ad", "ae", "af", "ag", "ah", "ai", "aj", "ak", "al", "am",
            "an", "ao", "ap", "aq", "ar", "as", "at", "au", "av", "aw", "ax", "ay", "az", "ba",
            "bb", "bc", "bd", "be",
        ];
        let (manifest, listing) = metadata_with_locales(&locales);

        validate(&manifest, &listing).expect("32 matching localizations");
    }

    #[test]
    fn listing_locales_are_unique_bounded_and_manifest_scoped() {
        let mut value = listing();
        value["localizations"] = json!([
            {"locale": "en", "name": "Hello", "description": "English"},
            {"locale": "fr", "name": "Bonjour", "description": "French"}
        ]);
        validate_listing(&value).expect("exact translation set");

        for localizations in [
            json!([{"locale": "fr", "name": "Bonjour", "description": "French"}]),
            json!([
                {"locale": "en", "name": "Hello", "description": "English"},
                {"locale": "en", "name": "Again", "description": "Duplicate"}
            ]),
            json!([
                {"locale": "en", "name": "Hello", "description": "English"},
                {"locale": "de", "name": "Hallo", "description": "Undeclared"}
            ]),
        ] {
            let mut invalid = listing();
            invalid["localizations"] = localizations;
            assert_eq!(validate_listing(&invalid), Err(PolicyCode::LocaleMismatch));
        }
    }

    #[test]
    fn listing_rejects_noncanonical_or_active_content() {
        for (field, value) in [
            ("author", " <b>PlayerVox</b>"),
            ("spdxLicense", "AGPL-3.0-only OR MIT"),
            (
                "sourceUrl",
                "https://github.com/PlayerVox/playervox-overcrow-marketplace/",
            ),
            ("sourceUrl", "http://github.com/PlayerVox/project"),
            ("previewFile", "../preview.png"),
            ("previewFile", "preview.svg"),
        ] {
            let mut invalid = listing();
            invalid[field] = json!(value);
            assert_eq!(
                validate_listing(&invalid),
                Err(PolicyCode::Listing),
                "{field}={value}"
            );
        }
    }

    #[test]
    fn listing_source_url_is_bounded_to_512_bytes() {
        let prefix = "https://example.test/";
        let mut boundary = listing();
        boundary["sourceUrl"] = json!(format!("{prefix}{}", "a".repeat(512 - prefix.len())));
        validate_listing(&boundary).expect("512-byte source URL");

        boundary["sourceUrl"] = json!(format!("{prefix}{}", "a".repeat(513 - prefix.len())));
        assert_eq!(validate_listing(&boundary), Err(PolicyCode::Listing));
    }

    #[test]
    fn listing_schema_documents_the_runtime_grammar() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../marketplace/listing.schema.json"))
                .expect("listing schema JSON");
        let properties = &schema["properties"];
        for (actual, expected) in [
            (
                &properties["spdxLicense"]["pattern"],
                "^[A-Za-z0-9][A-Za-z0-9.+-]{0,63}$",
            ),
            (
                &properties["sourceUrl"]["pattern"],
                "^https://[A-Za-z0-9.-]+/(?:[^/\\\\%?#]+/)*[^/\\\\%?#]+$",
            ),
            (
                &properties["localizations"]["items"]["properties"]["locale"]["pattern"],
                "^[a-z]{2}(?:-[A-Z]{2})?$",
            ),
            (
                &properties["previewFile"]["pattern"],
                "^(?!/)(?!.*(?:^|/)\\.{1,2}(?:/|$))[A-Za-z0-9._-]+(?:/[A-Za-z0-9._-]+)*\\.png$",
            ),
        ] {
            assert_eq!(actual, expected);
        }
        assert_eq!(properties["sourceUrl"]["maxLength"], 512);
        assert_eq!(properties["localizations"]["maxItems"], 32);
        assert_eq!(properties["previewFile"]["maxLength"], 192);
        assert!(
            schema["$comment"]
                .as_str()
                .is_some_and(|value| value.contains("UTF-8 byte limits"))
        );
    }

    #[test]
    fn targets_are_strict_unique_and_publisher_owned() {
        validate_targets(
            br#"[{"sourceDirectory":"examples/hello-widget","cargoPackage":"hello-widget","componentArtifact":"hello_widget","status":"verified"}]"#,
        )
        .expect("valid target");
        validate_targets(
            br#"[{"sourceDirectory":"examples/hello-widget","cargoPackage":"1-widget","componentArtifact":"1_widget","status":"verified"}]"#,
        )
        .expect("identifiers may start with an ASCII digit");
        for invalid in [
            br#"[{"sourceDirectory":"../hello","cargoPackage":"hello-widget","componentArtifact":"hello_widget","status":"verified"}]"#.as_slice(),
            br#"[{"sourceDirectory":"examples/hello-widget","cargoPackage":"hello-widget","componentArtifact":"hello_widget","status":"available"}]"#.as_slice(),
            br#"[{"sourceDirectory":"examples/hello-widget","cargoPackage":"hello-widget","componentArtifact":"hello_widget","status":"verified","url":"https://evil.invalid"}]"#.as_slice(),
            br#"[{"sourceDirectory":"examples/hello-widget","cargoPackage":"hello-widget","componentArtifact":"hello_widget","status":"verified"},{"sourceDirectory":"examples/hello-widget","cargoPackage":"hello-widget-two","componentArtifact":"hello_widget_two","status":"revoked"}]"#.as_slice(),
        ] {
            assert_eq!(validate_targets(invalid).map(|_| ()), Err(PolicyCode::Targets));
        }
    }

    #[test]
    fn component_inspection_rejects_imports_and_non_widget_components() {
        use wasm_encoder::{
            Component, ComponentImportSection, ComponentTypeRef, ComponentTypeSection,
            PrimitiveValType,
        };

        let mut types = ComponentTypeSection::new();
        types
            .function()
            .params([("value", PrimitiveValType::Bool)])
            .result(None);
        let mut imports = ComponentImportSection::new();
        imports.import(
            "wasi:cli/environment@0.2.0#get-environment",
            ComponentTypeRef::Func(0),
        );
        let mut component = Component::new();
        component.section(&types);
        component.section(&imports);

        assert_eq!(
            inspect_component(&component.finish()),
            Err(PolicyCode::ForbiddenImport)
        );
        assert_eq!(inspect_component(b"not wasm"), Err(PolicyCode::Component));
    }
}
