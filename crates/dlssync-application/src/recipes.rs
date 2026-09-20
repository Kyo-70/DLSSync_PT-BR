use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256 as Sha256Hasher};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const RECIPE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RecipeId(String);

impl RecipeId {
    pub fn parse(value: &str) -> Result<Self, RecipeValueError> {
        let value = value.trim();
        let valid_characters = value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'/' | b'_')
        });
        let valid_namespace = value == "reshade"
            || matches!(value.split('/').collect::<Vec<_>>().as_slice(), ["renodx", game] if !game.is_empty())
            || matches!(value.split('/').collect::<Vec<_>>().as_slice(), ["luma-framework", game] if !game.is_empty())
            || matches!(value.split('/').collect::<Vec<_>>().as_slice(), ["dlss-path", project, game, path] if !project.is_empty() && !game.is_empty() && matches!(*path, "rr" | "nr"));
        if value.is_empty() || !valid_characters || !valid_namespace {
            return Err(RecipeValueError::InvalidRecipeId(value.to_string()));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecipeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RecipeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(value: &str) -> Result<Self, RecipeValueError> {
        let normalized = value.replace('\\', "/");
        let path = Path::new(&normalized);
        if normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.starts_with("//")
            || normalized.contains(':')
            || path.is_absolute()
        {
            return Err(RecipeValueError::InvalidRelativePath(value.to_string()));
        }

        let mut has_component = false;
        for component in path.components() {
            let Component::Normal(component) = component else {
                return Err(RecipeValueError::InvalidRelativePath(value.to_string()));
            };
            has_component = true;
            let name = component.to_string_lossy();
            if name.is_empty()
                || name.ends_with('.')
                || name.ends_with(' ')
                || is_windows_reserved_name(&name)
            {
                return Err(RecipeValueError::InvalidRelativePath(value.to_string()));
            }
        }
        if !has_component || normalized.split('/').any(str::is_empty) {
            return Err(RecipeValueError::InvalidRelativePath(value.to_string()));
        }
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn comparison_key(&self) -> String {
        self.0.to_ascii_lowercase()
    }

    fn parent_key(&self) -> String {
        self.0
            .rsplit_once('/')
            .map_or_else(String::new, |(parent, _)| parent.to_ascii_lowercase())
    }

    fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256(String);

impl Sha256 {
    pub fn parse(value: &str) -> Result<Self, RecipeValueError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(RecipeValueError::InvalidSha256(value.to_string()));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn digest(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256Hasher::digest(bytes)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct HttpsUrl(String);

impl HttpsUrl {
    pub fn parse(value: &str) -> Result<Self, RecipeValueError> {
        let remainder = value.strip_prefix("https://").unwrap_or_default();
        let host = remainder.split(['/', '#', '?']).next().unwrap_or_default();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err(RecipeValueError::InvalidHttpsUrl(value.to_string()));
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for HttpsUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EvidenceId(String);

impl EvidenceId {
    pub fn new(value: impl Into<String>) -> Result<Self, RecipeValueError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(RecipeValueError::EmptyEvidenceId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RecipeValueError {
    #[error("invalid recipe id `{0}`")]
    InvalidRecipeId(String),
    #[error("invalid Windows-relative recipe path `{0}`")]
    InvalidRelativePath(String),
    #[error("invalid SHA-256 `{0}`")]
    InvalidSha256(String),
    #[error("recipe URL must be HTTPS with a host: `{0}`")]
    InvalidHttpsUrl(String),
    #[error("evidence id must not be empty")]
    EmptyEvidenceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    Recipe,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecipeV1 {
    pub schema_version: u16,
    pub kind: RecipeKind,
    pub id: RecipeId,
    pub revision: u32,
    pub upstream_version: String,
    pub experimental: bool,
    pub origin: Origin,
    pub license: LicenseEvidence,
    pub target: TargetScope,
    pub artifacts: Vec<Artifact>,
    pub dependencies: Vec<Dependency>,
    pub conflicts: Vec<DeclaredConflict>,
    pub installed_files: Vec<InstalledFile>,
    pub config_keys: Vec<ConfigEdit>,
    pub removal: RemovalRules,
    pub evidence: Vec<SourceEvidence>,
    pub compatibility_evidence: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub project_home: HttpsUrl,
    pub source_repository: HttpsUrl,
    pub release_channel: HttpsUrl,
    pub author_identity: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LicenseEvidence {
    pub license_id: LicenseId,
    pub text_url: HttpsUrl,
    pub text_revision: String,
    pub observed_at: String,
    pub redistribution: RedistributionPolicy,
    pub conditions: Vec<String>,
    pub permission_evidence: Vec<EvidenceId>,
    pub acquisition_policy: AcquisitionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LicenseId {
    #[serde(rename = "BSD-3-Clause")]
    Bsd3Clause,
    #[serde(rename = "MIT")]
    Mit,
    #[serde(rename = "LicenseRef-Luma-Custom-MIT")]
    LumaCustomMit,
    #[serde(rename = "UNVERIFIED")]
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RedistributionPolicy {
    PermittedWithConditions,
    ProhibitedByDistributionPolicy,
    PermissionRequired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionPolicy {
    ExplicitOriginalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetScope {
    pub game_ids: Vec<String>,
    pub executable_relative_path: RelativePath,
    pub executable_sha256: Sha256,
    pub game_build: String,
    pub architecture: Architecture,
    pub graphics_api: String,
    pub launch_mode: String,
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub upstream_release_id: String,
    pub source_url: HttpsUrl,
    pub allowed_redirect_hosts: Vec<String>,
    pub sha256: Sha256,
    pub size_bytes: u64,
    pub format: ArtifactFormat,
    pub max_expanded_bytes: u64,
    pub license_evidence: EvidenceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    Raw,
    Zip,
    ReviewedReshadeContainer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstalledFile {
    pub artifact_id: String,
    pub archive_entry: Option<RelativePath>,
    pub destination: RelativePath,
    pub sha256: Sha256,
    pub size_bytes: u64,
    pub role: FileRole,
    pub architecture: Option<Architecture>,
    pub install_rule: InstallRule,
    pub remove_rule: FileRemoveRule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    Proxy,
    Addon,
    Shader,
    Data,
    Notice,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X64,
    X86,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallRule {
    CreateOnly,
    ReplaceOwned,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileRemoveRule {
    UndoOwnedIfHashMatches,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub id: String,
    pub requirement: DependencyRequirement,
    pub evidence: Vec<EvidenceId>,
    pub allow_external_observation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DependencyRequirement {
    Recipe {
        id: RecipeId,
        revision: u32,
        manifest_sha256: Sha256,
    },
    Files {
        exact_paths_and_hashes: Vec<FileHashRequirement>,
    },
    Capability {
        name: String,
        documented_constraint: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileHashRequirement {
    pub path: RelativePath,
    pub sha256: Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeclaredConflict {
    OccupiedPath {
        path: RelativePath,
    },
    Recipe {
        id: RecipeId,
    },
    ConfigKey {
        path: RelativePath,
        selector: KeySelector,
    },
    ProxyChain {
        chain_id: String,
        evidence: Vec<EvidenceId>,
    },
    Protection {
        policy: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigEdit {
    pub path: RelativePath,
    pub format: ConfigFormat,
    pub selector: KeySelector,
    pub desired: Option<String>,
    pub expected_before: KeyCondition,
    pub source_evidence: EvidenceId,
    pub preserve_unrelated_bytes: bool,
    pub remove_rule: ConfigRemoveRule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    LosslessIni,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct KeySelector {
    pub section: String,
    pub key: String,
    pub case_rule: String,
    pub duplicates: DuplicateKeyPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateKeyPolicy {
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyCondition {
    Absent,
    Equals { value: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRemoveRule {
    RestoreIfValueStillOwned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemovalRules {
    pub require_committed_receipt: bool,
    pub require_current_hash: bool,
    pub retain_modified: bool,
    pub recursive_delete: bool,
    pub retain_undo_backups: bool,
    pub shared_dependency: SharedDependencyRemoval,
    pub config_policy: ConfigRemovalPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedDependencyRemoval {
    RetainWhileReferenced,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigRemovalPolicy {
    ThreeWayKeyRestore,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceEvidence {
    pub id: EvidenceId,
    pub url: HttpsUrl,
    pub observed_at: String,
    pub source_revision: String,
    pub claim: String,
    pub kind: EvidenceKind,
    pub environment: Option<String>,
    pub result: Option<EvidenceResult>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    OriginalDocumentation,
    License,
    FixtureTest,
    GameTest,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceResult {
    Pass,
    Fail,
}

#[derive(Debug, Deserialize)]
struct RecipeHeader {
    schema_version: u16,
}

pub fn parse_recipe_json(input: &str) -> Result<RecipeV1, RecipeValidationError> {
    let header: RecipeHeader = serde_json::from_str(input)
        .map_err(|error| RecipeValidationError::InvalidJson(error.to_string()))?;
    if header.schema_version != RECIPE_SCHEMA_VERSION {
        return Err(RecipeValidationError::UnsupportedSchemaVersion {
            observed: header.schema_version,
        });
    }
    let recipe: RecipeV1 = serde_json::from_str(input)
        .map_err(|error| RecipeValidationError::InvalidRecipe(error.to_string()))?;
    validate_recipe(&recipe)?;
    Ok(recipe)
}

pub fn validate_recipe(recipe: &RecipeV1) -> Result<(), RecipeValidationError> {
    if recipe.schema_version != RECIPE_SCHEMA_VERSION {
        return Err(RecipeValidationError::UnsupportedSchemaVersion {
            observed: recipe.schema_version,
        });
    }
    if recipe.revision == 0 {
        return Err(RecipeValidationError::ZeroRevision);
    }
    if recipe.upstream_version.trim().is_empty()
        || recipe.upstream_version.eq_ignore_ascii_case("latest")
    {
        return Err(RecipeValidationError::UnresolvedUpstreamVersion);
    }
    if recipe.artifacts.is_empty() {
        return Err(RecipeValidationError::EmptyArtifacts);
    }
    if recipe.installed_files.is_empty() {
        return Err(RecipeValidationError::EmptyInstalledFiles);
    }
    if recipe.evidence.is_empty() {
        return Err(RecipeValidationError::EmptyEvidence);
    }
    if recipe.compatibility_evidence.is_empty() {
        return Err(RecipeValidationError::MissingCompatibilityEvidence);
    }

    let mut artifact_ids = HashSet::new();
    for artifact in &recipe.artifacts {
        if artifact.id.trim().is_empty() || !artifact_ids.insert(artifact.id.as_str()) {
            return Err(RecipeValidationError::DuplicateArtifactId(
                artifact.id.clone(),
            ));
        }
        if matches!(artifact.format, ArtifactFormat::ReviewedReshadeContainer) {
            return Err(RecipeValidationError::DisabledArtifactFormat(
                artifact.id.clone(),
            ));
        }
    }

    let mut destinations = HashSet::new();
    for file in &recipe.installed_files {
        if !artifact_ids.contains(file.artifact_id.as_str()) {
            return Err(RecipeValidationError::UnknownArtifactReference {
                artifact_id: file.artifact_id.clone(),
                path: file.destination.clone(),
            });
        }
        if !destinations.insert(file.destination.comparison_key()) {
            return Err(RecipeValidationError::DuplicateDestination(
                file.destination.clone(),
            ));
        }
    }
    for edit in &recipe.config_keys {
        if !edit.preserve_unrelated_bytes {
            return Err(RecipeValidationError::DestructiveConfigEdit(
                edit.path.clone(),
            ));
        }
        if destinations.contains(&edit.path.comparison_key()) {
            return Err(RecipeValidationError::OverlappingFileAndConfig(
                edit.path.clone(),
            ));
        }
    }
    let removal = &recipe.removal;
    if !removal.require_committed_receipt
        || !removal.require_current_hash
        || !removal.retain_modified
        || removal.recursive_delete
        || !removal.retain_undo_backups
    {
        return Err(RecipeValidationError::UnsafeRemovalRules);
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RecipeValidationError {
    #[error("invalid recipe JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported recipe schema version {observed}; supported version is 1")]
    UnsupportedSchemaVersion { observed: u16 },
    #[error("invalid schema-1 recipe: {0}")]
    InvalidRecipe(String),
    #[error("recipe revision must be greater than zero")]
    ZeroRevision,
    #[error("recipe upstream version must be an immutable non-floating value")]
    UnresolvedUpstreamVersion,
    #[error("recipe artifact inventory must not be empty")]
    EmptyArtifacts,
    #[error("recipe installed-file inventory must not be empty")]
    EmptyInstalledFiles,
    #[error("recipe source evidence must not be empty")]
    EmptyEvidence,
    #[error("recipe compatibility evidence must not be empty")]
    MissingCompatibilityEvidence,
    #[error("recipe artifact id is empty or duplicated: `{0}`")]
    DuplicateArtifactId(String),
    #[error("artifact `{0}` uses a parser capability that is disabled")]
    DisabledArtifactFormat(String),
    #[error("file `{path}` references unknown artifact `{artifact_id}`")]
    UnknownArtifactReference {
        artifact_id: String,
        path: RelativePath,
    },
    #[error("recipe destination is duplicated using Windows path comparison: `{0}`")]
    DuplicateDestination(RelativePath),
    #[error("configuration edit for `{0}` does not preserve unrelated bytes")]
    DestructiveConfigEdit(RelativePath),
    #[error("file installation and configuration edit overlap at `{0}`")]
    OverlappingFileAndConfig(RelativePath),
    #[error("recipe removal rules do not require receipt, hash, retention, and non-recursive removal guards")]
    UnsafeRemovalRules,
}

impl RecipeValidationError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidJson(_) => "invalid_json",
            Self::UnsupportedSchemaVersion { .. } => "unsupported_schema_version",
            Self::InvalidRecipe(_) => "invalid_recipe",
            Self::ZeroRevision => "zero_revision",
            Self::UnresolvedUpstreamVersion => "unresolved_upstream_version",
            Self::EmptyArtifacts => "empty_artifacts",
            Self::EmptyInstalledFiles => "empty_installed_files",
            Self::EmptyEvidence => "empty_evidence",
            Self::MissingCompatibilityEvidence => "missing_compatibility_evidence",
            Self::DuplicateArtifactId(_) => "duplicate_artifact_id",
            Self::DisabledArtifactFormat(_) => "disabled_artifact_format",
            Self::UnknownArtifactReference { .. } => "unknown_artifact_reference",
            Self::DuplicateDestination(_) => "duplicate_destination",
            Self::DestructiveConfigEdit(_) => "destructive_config_edit",
            Self::OverlappingFileAndConfig(_) => "overlapping_file_and_config",
            Self::UnsafeRemovalRules => "unsafe_removal_rules",
        }
    }

    pub const fn schema_version(&self) -> Option<u16> {
        match self {
            Self::UnsupportedSchemaVersion { observed } => Some(*observed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeFileClaim {
    pub recipe_id: RecipeId,
    pub path: RelativePath,
    pub role: FileRole,
}

impl RecipeFileClaim {
    pub const fn new(recipe_id: RecipeId, path: RelativePath, role: FileRole) -> Self {
        Self {
            recipe_id,
            path,
            role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialFileClaim {
    pub component_id: String,
    pub path: RelativePath,
}

impl OfficialFileClaim {
    pub fn new(component_id: impl Into<String>, path: RelativePath) -> Self {
        Self {
            component_id: component_id.into(),
            path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxySlot {
    Dxgi,
    Dinput8,
    Version,
}

impl fmt::Display for ProxySlot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Dxgi => "dxgi.dll",
            Self::Dinput8 => "dinput8.dll",
            Self::Version => "version.dll",
        })
    }
}

pub fn validate_file_claims(
    recipe_claims: &[RecipeFileClaim],
    official_claims: &[OfficialFileClaim],
) -> Result<(), RecipeConflictError> {
    let official_by_path: HashMap<_, _> = official_claims
        .iter()
        .map(|claim| (claim.path.comparison_key(), claim))
        .collect();
    let mut recipe_by_path: HashMap<String, &RecipeFileClaim> = HashMap::new();
    let mut proxy_by_directory: HashMap<String, (&RecipeFileClaim, ProxySlot)> = HashMap::new();

    for claim in recipe_claims {
        if let Some(official) = official_by_path.get(&claim.path.comparison_key()) {
            return Err(RecipeConflictError::OfficialComponentPathConflict {
                recipe_id: claim.recipe_id.clone(),
                component_id: official.component_id.clone(),
                path: claim.path.clone(),
            });
        }
        if let Some(first) = recipe_by_path.insert(claim.path.comparison_key(), claim) {
            return Err(RecipeConflictError::RecipePathConflict {
                first_recipe: first.recipe_id.clone(),
                second_recipe: claim.recipe_id.clone(),
                path: claim.path.clone(),
            });
        }

        if claim.role == FileRole::Proxy {
            let slot = proxy_slot(&claim.path).ok_or_else(|| {
                RecipeConflictError::UnsupportedProxyFilename {
                    recipe_id: claim.recipe_id.clone(),
                    path: claim.path.clone(),
                }
            })?;
            if let Some((first, first_slot)) =
                proxy_by_directory.insert(claim.path.parent_key(), (claim, slot))
            {
                return Err(RecipeConflictError::UnsupportedProxyChain {
                    first_recipe: first.recipe_id.clone(),
                    first_slot,
                    second_recipe: claim.recipe_id.clone(),
                    second_slot: slot,
                    directory: claim.path.parent_key(),
                });
            }
        }
    }
    Ok(())
}

fn proxy_slot(path: &RelativePath) -> Option<ProxySlot> {
    match path.file_name().to_ascii_lowercase().as_str() {
        "dxgi.dll" => Some(ProxySlot::Dxgi),
        "dinput8.dll" => Some(ProxySlot::Dinput8),
        "version.dll" => Some(ProxySlot::Version),
        _ => None,
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RecipeConflictError {
    #[error("recipes `{first_recipe}` and `{second_recipe}` both claim `{path}`")]
    RecipePathConflict {
        first_recipe: RecipeId,
        second_recipe: RecipeId,
        path: RelativePath,
    },
    #[error("recipe `{recipe_id}` claims `{path}`, which is managed by official component `{component_id}`")]
    OfficialComponentPathConflict {
        recipe_id: RecipeId,
        component_id: String,
        path: RelativePath,
    },
    #[error("recipe `{recipe_id}` marks unsupported proxy filename `{path}` as a proxy")]
    UnsupportedProxyFilename {
        recipe_id: RecipeId,
        path: RelativePath,
    },
    #[error("recipes `{first_recipe}` ({first_slot}) and `{second_recipe}` ({second_slot}) request an unverified proxy chain in `{directory}`")]
    UnsupportedProxyChain {
        first_recipe: RecipeId,
        first_slot: ProxySlot,
        second_recipe: RecipeId,
        second_slot: ProxySlot,
        directory: String,
    },
}

impl RecipeConflictError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::RecipePathConflict { .. } => "recipe_path_conflict",
            Self::OfficialComponentPathConflict { .. } => "official_component_path_conflict",
            Self::UnsupportedProxyFilename { .. } => "unsupported_proxy_filename",
            Self::UnsupportedProxyChain { .. } => "unsupported_proxy_chain",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OwnershipStatus {
    None,
    InstalledByDlssync { receipt_id: String },
    OwnedModified { paths: Vec<RelativePath> },
    OwnedMissing { paths: Vec<RelativePath> },
    PartiallyRemoved { retained_paths: Vec<RelativePath> },
    Removed { receipt_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ObservationStatus {
    NotObserved,
    FilesDetected {
        method: String,
        paths: Vec<RelativePath>,
        hashes: Vec<Sha256>,
        observed_at: String,
    },
    RuntimeDetected {
        process_identity: String,
        module_paths: Vec<RelativePath>,
        observed_at: String,
    },
    UnknownOrInaccessible {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Unknown,
    DocumentedOnly { evidence_ids: Vec<EvidenceId> },
    TestedPass { evidence_id: EvidenceId },
    TestedFail { evidence_id: EvidenceId },
    Stale { previous_evidence_id: EvidenceId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SupportStatus {
    Unknown,
    Experimental,
    Official { evidence_ids: Vec<EvidenceId> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecipeState {
    pub ownership: OwnershipStatus,
    pub observation: ObservationStatus,
    pub compatibility: CompatibilityStatus,
    pub support: SupportStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRestoration {
    pub path: PathBuf,
    pub observed_sha256: Sha256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestorationVerification {
    Verified(VerifiedRestoration),
    Failed(RemovalFailure),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RemovalFailure {
    #[error("could not read restored path `{path}`: {reason}")]
    ReadFailed { path: PathBuf, reason: String },
    #[error("restored path `{path}` has SHA-256 {observed}, expected {expected}")]
    HashMismatch {
        path: PathBuf,
        expected: Sha256,
        observed: Sha256,
    },
    #[error("removal cannot complete without restoration verification")]
    MissingVerification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalOutcome {
    Removed {
        verified_restorations: Vec<VerifiedRestoration>,
    },
    Failed {
        verified_restorations: Vec<VerifiedRestoration>,
        failures: Vec<RemovalFailure>,
    },
}

pub fn verify_restoration(path: &Path, expected: &Sha256) -> RestorationVerification {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return RestorationVerification::Failed(RemovalFailure::ReadFailed {
                path: path.to_path_buf(),
                reason: error.to_string(),
            });
        }
    };
    let observed = Sha256::digest(&bytes);
    if &observed == expected {
        RestorationVerification::Verified(VerifiedRestoration {
            path: path.to_path_buf(),
            observed_sha256: observed,
        })
    } else {
        RestorationVerification::Failed(RemovalFailure::HashMismatch {
            path: path.to_path_buf(),
            expected: expected.clone(),
            observed,
        })
    }
}

pub fn finalize_removal(verifications: Vec<RestorationVerification>) -> RemovalOutcome {
    if verifications.is_empty() {
        return RemovalOutcome::Failed {
            verified_restorations: Vec::new(),
            failures: vec![RemovalFailure::MissingVerification],
        };
    }

    let mut verified_restorations = Vec::new();
    let mut failures = Vec::new();
    for verification in verifications {
        match verification {
            RestorationVerification::Verified(verified) => {
                verified_restorations.push(verified);
            }
            RestorationVerification::Failed(failure) => failures.push(failure),
        }
    }
    if failures.is_empty() {
        RemovalOutcome::Removed {
            verified_restorations,
        }
    } else {
        RemovalOutcome::Failed {
            verified_restorations,
            failures,
        }
    }
}

const RECIPE_STORE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Error)]
pub enum RecipeTransactionError {
    #[error(transparent)]
    Execution(#[from] crate::execution::ExecutionError),
    #[error("recipe receipt store failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("recipe receipt payload failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("recipe operation journal failed: {0}")]
    Journal(#[from] operation_journal::JournalError),
    #[error("recipe receipt `{0}` was not found")]
    MissingReceipt(String),
    #[error("recipe `{0}` has no active receipt")]
    MissingActiveReceipt(RecipeId),
    #[error("recipe transaction `{0}` was not found")]
    MissingTransaction(String),
    #[error("recipe mutation is stale: {0}")]
    Stale(String),
    #[error("recipe transaction recovery failed: {0}")]
    Recovery(String),
    #[error("recipe transaction interrupted after write boundary: {0}")]
    Interrupted(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipeTransactionKind {
    Apply,
    Remove,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipeTransactionStage {
    Planned,
    Applying,
    Completed,
    RollingBack,
    RolledBack,
    RollbackFailed,
}

impl RecipeTransactionStage {
    const fn needs_recovery(self) -> bool {
        matches!(
            self,
            Self::Applying | Self::RollingBack | Self::RollbackFailed
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecipeMutationKind {
    Create,
    Replace,
    Remove,
    Configure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "presence", rename_all = "snake_case")]
pub enum RecipeFilePresence {
    Absent,
    Present {
        sha256: Sha256,
        undo_backup: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeReceiptMember {
    pub target: PathBuf,
    pub kind: RecipeMutationKind,
    pub before: RecipeFilePresence,
    pub owned_sha256: Sha256,
    pub config: Option<RecipeConfigOwnership>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeConfigOwnership {
    pub selector: KeySelector,
    pub original: KeyCondition,
    pub owned: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipeReceipt {
    #[serde(default)]
    pub upstream_version: String,
    pub schema_version: u16,
    pub installation_id: String,
    pub receipt_id: String,
    pub recipe_id: RecipeId,
    pub revision: u32,
    pub operation_id: String,
    pub transaction_id: String,
    pub supersedes_receipt_id: Option<String>,
    pub created_at: String,
    pub members: Vec<RecipeReceiptMember>,
}

#[derive(Debug, Clone)]
pub struct PreparedRecipeMutation {
    pub relative_path: RelativePath,
    pub artifact_id: Option<String>,
    pub archive_entry: Option<RelativePath>,
    pub target: PathBuf,
    pub staged: PathBuf,
    pub undo_backup: Option<PathBuf>,
    pub expected_before: RecipeFilePresence,
    pub expected_after: Sha256,
    pub kind: RecipeMutationKind,
    pub config: Option<RecipeConfigOwnership>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RecipeTransactionMember {
    target: PathBuf,
    kind: RecipeMutationKind,
    before: RecipeFilePresence,
    receipt_before: RecipeFilePresence,
    after: RecipeFilePresence,
    staged: Option<PathBuf>,
    config: Option<RecipeConfigOwnership>,
    applied: bool,
    retained_external_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RecipeTransactionRecord {
    schema_version: u16,
    installation_id: String,
    id: String,
    operation_id: String,
    recipe_id: RecipeId,
    receipt_id: String,
    supersedes_receipt_id: Option<String>,
    kind: RecipeTransactionKind,
    stage: RecipeTransactionStage,
    members: Vec<RecipeTransactionMember>,
    error: Option<String>,
    started_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ActiveRecipeReceipt {
    recipe_id: RecipeId,
    receipt_id: String,
    #[serde(default)]
    removed: bool,
}

#[derive(Debug, Clone)]
pub struct RecipeStore {
    root: PathBuf,
    installation_root: PathBuf,
    installation_id: String,
}

impl RecipeStore {
    pub fn open_for_install(
        data_root: impl Into<PathBuf>,
        installation_root: impl Into<PathBuf>,
    ) -> Result<Self, RecipeTransactionError> {
        let installation_root = installation_root.into().canonicalize()?;
        if !installation_root.is_dir() {
            return Err(RecipeTransactionError::Stale(format!(
                "installation root is not a directory: {}",
                installation_root.display()
            )));
        }
        let installation_id = safe_name(&normalized_path_key(&installation_root));
        let store = Self {
            root: data_root
                .into()
                .join("installations")
                .join(&installation_id),
            installation_root,
            installation_id,
        };
        std::fs::create_dir_all(store.receipts_dir())?;
        std::fs::create_dir_all(store.transactions_dir())?;
        std::fs::create_dir_all(store.active_dir())?;
        std::fs::create_dir_all(store.undo_dir())?;
        restore_interrupted_state_files(&store.transactions_dir())?;
        restore_interrupted_state_files(&store.active_dir())?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn installation_root(&self) -> &Path {
        &self.installation_root
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn target(&self, relative: &RelativePath) -> Result<PathBuf, RecipeTransactionError> {
        let target = self.installation_root.join(relative.as_str());
        let parent = target.parent().ok_or_else(|| {
            RecipeTransactionError::Stale(format!("recipe target has no parent: {}", relative))
        })?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            RecipeTransactionError::Stale(format!(
                "recipe target parent is unavailable for {}: {error}",
                relative
            ))
        })?;
        if !canonical_parent.starts_with(&self.installation_root) {
            return Err(RecipeTransactionError::Stale(format!(
                "recipe target escapes the installation root: {}",
                relative
            )));
        }
        Ok(canonical_parent.join(target.file_name().unwrap_or_default()))
    }

    pub fn active_receipts(&self) -> Result<Vec<RecipeReceipt>, RecipeTransactionError> {
        let mut receipts = Vec::new();
        for entry in std::fs::read_dir(self.active_dir())? {
            let entry = entry?;
            if !entry.file_type()?.is_file()
                || entry.file_name().to_string_lossy().ends_with(".previous")
            {
                continue;
            }
            let active: ActiveRecipeReceipt = read_json(&entry.path())?;
            if !active.removed {
                receipts.push(self.receipt(&active.receipt_id)?);
            }
        }
        receipts.sort_by(|left, right| left.recipe_id.as_str().cmp(right.recipe_id.as_str()));
        Ok(receipts)
    }

    pub fn receipt(&self, receipt_id: &str) -> Result<RecipeReceipt, RecipeTransactionError> {
        read_json(&self.receipt_path(receipt_id)).map_err(|error| match error {
            RecipeTransactionError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                RecipeTransactionError::MissingReceipt(receipt_id.to_string())
            }
            other => other,
        })
    }

    pub fn active_receipt(
        &self,
        recipe_id: &RecipeId,
    ) -> Result<RecipeReceipt, RecipeTransactionError> {
        let active: ActiveRecipeReceipt =
            read_json(&self.active_path(recipe_id)).map_err(|error| match error {
                RecipeTransactionError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                    RecipeTransactionError::MissingActiveReceipt(recipe_id.clone())
                }
                other => other,
            })?;
        if active.removed {
            return Err(RecipeTransactionError::MissingActiveReceipt(
                recipe_id.clone(),
            ));
        }
        self.receipt(&active.receipt_id)
    }

    pub fn transaction(
        &self,
        transaction_id: &str,
    ) -> Result<(RecipeTransactionStage, Vec<PathBuf>), RecipeTransactionError> {
        let record = self.read_transaction(transaction_id)?;
        Ok((
            record.stage,
            record
                .members
                .into_iter()
                .map(|member| member.target)
                .collect(),
        ))
    }

    fn receipts_dir(&self) -> PathBuf {
        self.root.join("receipts")
    }

    fn transactions_dir(&self) -> PathBuf {
        self.root.join("transactions")
    }

    fn active_dir(&self) -> PathBuf {
        self.root.join("active")
    }

    fn undo_dir(&self) -> PathBuf {
        self.root.join("undo")
    }

    fn receipt_path(&self, receipt_id: &str) -> PathBuf {
        self.receipts_dir()
            .join(format!("{}.json", safe_name(receipt_id)))
    }

    fn transaction_path(&self, transaction_id: &str) -> PathBuf {
        self.transactions_dir()
            .join(format!("{}.json", safe_name(transaction_id)))
    }

    fn active_path(&self, recipe_id: &RecipeId) -> PathBuf {
        self.active_dir()
            .join(format!("{}.json", safe_name(recipe_id.as_str())))
    }

    fn save_receipt(&self, receipt: &RecipeReceipt) -> Result<(), RecipeTransactionError> {
        let path = self.receipt_path(&receipt.receipt_id);
        if path.try_exists()? {
            let existing: RecipeReceipt = read_json(&path)?;
            if existing != *receipt {
                return Err(RecipeTransactionError::Stale(format!(
                    "receipt id {} already contains different evidence",
                    receipt.receipt_id
                )));
            }
            return Ok(());
        }
        write_json_atomic_new(&path, receipt)
    }

    fn set_active_receipt(&self, receipt: &RecipeReceipt) -> Result<(), RecipeTransactionError> {
        write_json_atomic_replace(
            &self.active_path(&receipt.recipe_id),
            &ActiveRecipeReceipt {
                recipe_id: receipt.recipe_id.clone(),
                receipt_id: receipt.receipt_id.clone(),
                removed: false,
            },
        )
    }

    fn clear_active_receipt(
        &self,
        recipe_id: &RecipeId,
        receipt_id: &str,
    ) -> Result<(), RecipeTransactionError> {
        let path = self.active_path(recipe_id);
        let active: ActiveRecipeReceipt = read_json(&path)?;
        if active.receipt_id != receipt_id {
            return Err(RecipeTransactionError::Stale(format!(
                "active receipt changed from {} to {}",
                receipt_id, active.receipt_id
            )));
        }
        write_json_atomic_replace(
            &path,
            &ActiveRecipeReceipt {
                recipe_id: active.recipe_id,
                receipt_id: active.receipt_id,
                removed: true,
            },
        )
    }

    fn save_transaction(
        &self,
        record: &RecipeTransactionRecord,
    ) -> Result<(), RecipeTransactionError> {
        write_json_atomic_replace(&self.transaction_path(&record.id), record)
    }

    fn read_transaction(
        &self,
        transaction_id: &str,
    ) -> Result<RecipeTransactionRecord, RecipeTransactionError> {
        read_json(&self.transaction_path(transaction_id)).map_err(|error| match error {
            RecipeTransactionError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => {
                RecipeTransactionError::MissingTransaction(transaction_id.to_string())
            }
            other => other,
        })
    }
}

pub fn apply_recipe_durable(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
    operation_id: &str,
    recipe: &RecipeV1,
    prepared: &[PreparedRecipeMutation],
) -> Result<RecipeReceipt, RecipeTransactionError> {
    apply_recipe_durable_scoped(
        store,
        journal,
        operation_id,
        recipe,
        prepared,
        RecipeApplyScope::Files,
        |_| Ok(()),
    )
}

#[doc(hidden)]
pub fn apply_recipe_durable_observed(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
    operation_id: &str,
    recipe: &RecipeV1,
    prepared: &[PreparedRecipeMutation],
    after_write: impl FnMut(usize) -> Result<(), String>,
) -> Result<RecipeReceipt, RecipeTransactionError> {
    apply_recipe_durable_scoped(
        store,
        journal,
        operation_id,
        recipe,
        prepared,
        RecipeApplyScope::Files,
        after_write,
    )
}

pub fn configure_recipe_durable(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
    operation_id: &str,
    recipe: &RecipeV1,
) -> Result<RecipeReceipt, RecipeTransactionError> {
    let previous = match store.active_receipt(&recipe.id) {
        Ok(previous) => previous,
        Err(RecipeTransactionError::MissingActiveReceipt(_))
            if recipe.installed_files.is_empty() =>
        {
            RecipeReceipt {
                upstream_version: recipe.upstream_version.clone(),
                schema_version: RECIPE_STORE_SCHEMA_VERSION,
                installation_id: store.installation_id().to_string(),
                receipt_id: String::new(),
                recipe_id: recipe.id.clone(),
                revision: recipe.revision,
                operation_id: String::new(),
                transaction_id: String::new(),
                supersedes_receipt_id: None,
                created_at: String::new(),
                members: Vec::new(),
            }
        }
        Err(error) => return Err(error),
    };
    let prepared = prepare_configuration_mutations(store, recipe, &previous)?;
    apply_recipe_durable_scoped(
        store,
        journal,
        operation_id,
        recipe,
        &prepared,
        RecipeApplyScope::Configuration,
        |_| Ok(()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecipeApplyScope {
    Files,
    Configuration,
}

fn apply_recipe_durable_scoped(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
    operation_id: &str,
    recipe: &RecipeV1,
    prepared: &[PreparedRecipeMutation],
    scope: RecipeApplyScope,
    mut after_write: impl FnMut(usize) -> Result<(), String>,
) -> Result<RecipeReceipt, RecipeTransactionError> {
    validate_recipe(recipe).map_err(|error| RecipeTransactionError::Stale(error.to_string()))?;
    if prepared.is_empty() {
        return Err(RecipeTransactionError::Stale(
            "recipe mutation list is empty".into(),
        ));
    }
    let paths = unique_paths(prepared.iter().map(|mutation| &mutation.target))?;
    let coordinated = crate::transaction::coordinate_targets(journal, &paths, None)?;
    let previous_receipt = match store.active_receipt(&recipe.id) {
        Ok(receipt) => Some(receipt),
        Err(RecipeTransactionError::MissingActiveReceipt(_))
            if scope == RecipeApplyScope::Files || recipe.installed_files.is_empty() =>
        {
            None
        }
        Err(RecipeTransactionError::MissingActiveReceipt(_)) => {
            return Err(RecipeTransactionError::Stale(
                "configuration cannot create ownership while file members remain unapplied".into(),
            ));
        }
        Err(error) => return Err(error),
    };
    validate_prepared_recipe_mutations(store, recipe, prepared, previous_receipt.as_ref(), scope)?;
    let previous_by_path: HashMap<_, _> = previous_receipt
        .iter()
        .flat_map(|receipt| receipt.members.iter())
        .map(|member| (normalized_path_key(&member.target), member))
        .collect();

    let now = chrono::Utc::now().to_rfc3339();
    let mut record = RecipeTransactionRecord {
        schema_version: RECIPE_STORE_SCHEMA_VERSION,
        installation_id: store.installation_id().to_string(),
        id: uuid::Uuid::new_v4().to_string(),
        operation_id: operation_id.to_string(),
        recipe_id: recipe.id.clone(),
        receipt_id: uuid::Uuid::new_v4().to_string(),
        supersedes_receipt_id: previous_receipt
            .as_ref()
            .map(|receipt| receipt.receipt_id.clone()),
        kind: RecipeTransactionKind::Apply,
        stage: RecipeTransactionStage::Planned,
        members: prepared
            .iter()
            .map(|mutation| RecipeTransactionMember {
                target: mutation.target.clone(),
                kind: mutation.kind,
                before: mutation.expected_before.clone(),
                receipt_before: previous_by_path
                    .get(&normalized_path_key(&mutation.target))
                    .map_or_else(
                        || mutation.expected_before.clone(),
                        |member| member.before.clone(),
                    ),
                after: RecipeFilePresence::Present {
                    sha256: mutation.expected_after.clone(),
                    undo_backup: None,
                },
                staged: Some(mutation.staged.clone()),
                config: mutation.config.clone(),
                applied: false,
                retained_external_change: false,
            })
            .collect(),
        error: None,
        started_at: now.clone(),
        updated_at: now,
    };
    let fence = recovery_fence(&record, journal)?;
    coordinated.register_recovery(journal, &fence)?;
    store.save_transaction(&record)?;
    record.stage = RecipeTransactionStage::Applying;
    record.updated_at = chrono::Utc::now().to_rfc3339();
    store.save_transaction(&record)?;
    journal.save_recovery(&fence)?;

    let result = (|| -> Result<(), RecipeTransactionError> {
        for (index, mutation) in prepared.iter().enumerate() {
            verify_presence(&mutation.target, &mutation.expected_before)?;
            match &mutation.expected_before {
                RecipeFilePresence::Absent => {
                    if mutation.undo_backup.is_some() {
                        return Err(RecipeTransactionError::Stale(format!(
                            "created target {} must not declare an undo backup",
                            mutation.target.display()
                        )));
                    }
                }
                RecipeFilePresence::Present { sha256, .. } => {
                    if mutation.kind == RecipeMutationKind::Configure {
                        let backup = store
                            .undo_dir()
                            .join(&record.id)
                            .join(format!("config-{index}.bin"));
                        copy_verified_new(&mutation.target, &backup, sha256)?;
                        if let RecipeFilePresence::Present { undo_backup, .. } =
                            &mut record.members[index].before
                        {
                            *undo_backup = Some(backup);
                        }
                        store.save_transaction(&record)?;
                        replace_from_staged(
                            &mutation.staged,
                            &mutation.target,
                            &mutation.expected_after,
                        )?;
                        after_write(index).map_err(RecipeTransactionError::Interrupted)?;
                        record.members[index].applied = true;
                        record.updated_at = chrono::Utc::now().to_rfc3339();
                        store.save_transaction(&record)?;
                        continue;
                    }
                    let backup = mutation.undo_backup.as_ref().ok_or_else(|| {
                        RecipeTransactionError::Stale(format!(
                            "target {} requires an undo backup",
                            mutation.target.display()
                        ))
                    })?;
                    copy_verified_new(&mutation.target, backup, sha256)?;
                    if let RecipeFilePresence::Present { undo_backup, .. } =
                        &mut record.members[index].before
                    {
                        *undo_backup = Some(backup.clone());
                    }
                    if let RecipeFilePresence::Present { undo_backup, .. } =
                        &mut record.members[index].receipt_before
                    {
                        if undo_backup.is_none() {
                            *undo_backup = Some(backup.clone());
                        }
                    }
                }
            }
            store.save_transaction(&record)?;
            replace_from_staged(&mutation.staged, &mutation.target, &mutation.expected_after)?;
            after_write(index).map_err(RecipeTransactionError::Interrupted)?;
            record.members[index].applied = true;
            record.updated_at = chrono::Utc::now().to_rfc3339();
            store.save_transaction(&record)?;
        }
        Ok(())
    })();

    if let Err(error @ RecipeTransactionError::Interrupted(_)) = result {
        record.error = Some(error.to_string());
        record.updated_at = chrono::Utc::now().to_rfc3339();
        store.save_transaction(&record)?;
        return Err(error);
    }
    if let Err(error) = result {
        record.error = Some(error.to_string());
        rollback_recipe_record(store, &mut record)?;
        return Err(error);
    }

    let prepared_paths: HashSet<_> = record
        .members
        .iter()
        .map(|member| normalized_path_key(&member.target))
        .collect();
    let mut receipt_members = previous_receipt
        .iter()
        .flat_map(|receipt| receipt.members.iter().cloned())
        .filter(|member| !prepared_paths.contains(&normalized_path_key(&member.target)))
        .collect::<Vec<_>>();
    receipt_members.extend(record.members.iter().map(|member| {
        RecipeReceiptMember {
            target: member.target.clone(),
            kind: member.kind,
            before: member.receipt_before.clone(),
            owned_sha256: presence_hash(&member.after)
                .expect("apply members always end present")
                .clone(),
            config: member.config.clone(),
        }
    }));
    receipt_members.sort_by_key(|member| normalized_path_key(&member.target));
    let receipt = RecipeReceipt {
        upstream_version: recipe.upstream_version.clone(),
        schema_version: RECIPE_STORE_SCHEMA_VERSION,
        installation_id: store.installation_id().to_string(),
        receipt_id: record.receipt_id.clone(),
        recipe_id: recipe.id.clone(),
        revision: recipe.revision,
        operation_id: operation_id.to_string(),
        transaction_id: record.id.clone(),
        supersedes_receipt_id: record.supersedes_receipt_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        members: receipt_members,
    };
    store.save_receipt(&receipt)?;
    store.set_active_receipt(&receipt)?;
    record.stage = RecipeTransactionStage::Completed;
    record.updated_at = chrono::Utc::now().to_rfc3339();
    store.save_transaction(&record)?;
    complete_recovery_fence(journal, &record)?;

    if let Some(previous) = previous_receipt {
        if previous.transaction_id.is_empty() {
            return Ok(receipt);
        }
        crate::transaction::supersede_recovery_targets(
            journal,
            &previous.transaction_id,
            &previous
                .members
                .iter()
                .map(|member| member.target.clone())
                .collect::<Vec<_>>(),
            &format!(
                "recipe receipt {} superseded by {}",
                previous.receipt_id, receipt.receipt_id
            ),
        )?;
    }
    Ok(receipt)
}

pub fn remove_recipe_durable(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
    operation_id: &str,
    recipe_id: &RecipeId,
) -> Result<RemovalOutcome, RecipeTransactionError> {
    let receipt = store.active_receipt(recipe_id)?;
    let paths: Vec<_> = receipt
        .members
        .iter()
        .map(|member| member.target.clone())
        .collect();
    let coordinated = crate::transaction::coordinate_targets(journal, &paths, None)?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut record = RecipeTransactionRecord {
        schema_version: RECIPE_STORE_SCHEMA_VERSION,
        installation_id: store.installation_id().to_string(),
        id: uuid::Uuid::new_v4().to_string(),
        operation_id: operation_id.to_string(),
        recipe_id: recipe_id.clone(),
        receipt_id: receipt.receipt_id.clone(),
        supersedes_receipt_id: None,
        kind: RecipeTransactionKind::Remove,
        stage: RecipeTransactionStage::Planned,
        members: receipt
            .members
            .iter()
            .map(|member| RecipeTransactionMember {
                target: member.target.clone(),
                kind: RecipeMutationKind::Remove,
                before: RecipeFilePresence::Present {
                    sha256: member.owned_sha256.clone(),
                    undo_backup: None,
                },
                receipt_before: RecipeFilePresence::Present {
                    sha256: member.owned_sha256.clone(),
                    undo_backup: None,
                },
                after: member.before.clone(),
                staged: None,
                config: member.config.clone(),
                applied: false,
                retained_external_change: false,
            })
            .collect(),
        error: None,
        started_at: now.clone(),
        updated_at: now,
    };
    let fence = recovery_fence(&record, journal)?;
    coordinated.register_recovery(journal, &fence)?;
    store.save_transaction(&record)?;
    record.stage = RecipeTransactionStage::Applying;
    store.save_transaction(&record)?;
    journal.save_recovery(&fence)?;

    let mut verifications = Vec::new();
    for index in (0..record.members.len()).rev() {
        let member = record.members[index].clone();
        if let Some(config) = &member.config {
            let current = std::fs::read(&member.target)?;
            let observed_value = ini_key_value(&current, &config.selector)?;
            if observed_value != config.owned {
                record.members[index].retained_external_change = true;
                record.error = Some(format!(
                    "externally changed recipe configuration retained: {}",
                    member.target.display()
                ));
                store.save_transaction(&record)?;
                verifications.push(RestorationVerification::Failed(
                    RemovalFailure::ReadFailed {
                        path: member.target.clone(),
                        reason: "recipe-owned configuration key changed externally".into(),
                    },
                ));
                continue;
            }
            let removal_undo = store
                .undo_dir()
                .join(&record.id)
                .join(format!("{index}.bin"));
            let current_hash = Sha256::digest(&current);
            copy_verified_new(&member.target, &removal_undo, &current_hash)?;
            record.members[index].before = RecipeFilePresence::Present {
                sha256: current_hash,
                undo_backup: Some(removal_undo),
            };
            store.save_transaction(&record)?;
            let restored = patch_ini_key(
                &current,
                &config.selector,
                key_condition_value(&config.original),
            )?;
            replace_bytes(&restored, &member.target)?;
            record.members[index].after = observed_presence(&member.target)?;
            record.members[index].applied = true;
            record.updated_at = chrono::Utc::now().to_rfc3339();
            store.save_transaction(&record)?;
            verifications.push(verify_config_restoration(&member.target, config));
            continue;
        }
        let observed = observed_presence(&member.target)?;
        if !presence_matches(&observed, &member.before) {
            record.members[index].retained_external_change = true;
            record.error = Some(format!(
                "externally changed recipe target retained: {}",
                member.target.display()
            ));
            store.save_transaction(&record)?;
            verifications.push(RestorationVerification::Failed(removal_presence_failure(
                &member.target,
                &member.before,
                observed,
            )));
            continue;
        }
        let removal_undo = store
            .undo_dir()
            .join(&record.id)
            .join(format!("{index}.bin"));
        let owned_sha256 = presence_hash(&member.before)
            .expect("removal members start present")
            .clone();
        copy_verified_new(&member.target, &removal_undo, &owned_sha256)?;
        if let RecipeFilePresence::Present { undo_backup, .. } = &mut record.members[index].before {
            *undo_backup = Some(removal_undo);
        }
        store.save_transaction(&record)?;
        restore_presence(&member.target, &member.after)?;
        record.members[index].applied = true;
        record.updated_at = chrono::Utc::now().to_rfc3339();
        store.save_transaction(&record)?;
        verifications.push(verify_removed_member(&member.target, &member.after));
    }
    verifications.reverse();
    let outcome = finalize_removal(verifications);
    match &outcome {
        RemovalOutcome::Removed { .. } => {
            store.clear_active_receipt(recipe_id, &receipt.receipt_id)?;
            record.stage = RecipeTransactionStage::Completed;
            store.save_transaction(&record)?;
            complete_recovery_fence(journal, &record)?;
            crate::transaction::supersede_recovery_targets(
                journal,
                &receipt.transaction_id,
                &paths,
                &format!(
                    "recipe receipt {} removed by {}",
                    receipt.receipt_id, operation_id
                ),
            )?;
        }
        RemovalOutcome::Failed { .. } => {
            rollback_recipe_record(store, &mut record)?;
            complete_recovery_fence(journal, &record)?;
        }
    }
    Ok(outcome)
}

pub fn recover_recipe_transactions(
    store: &RecipeStore,
    journal: &operation_journal::JournalStore,
) -> Result<Vec<String>, RecipeTransactionError> {
    let mut transaction_paths = Vec::new();
    for entry in std::fs::read_dir(store.transactions_dir())? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            transaction_paths.push(entry.path());
        }
    }
    transaction_paths.sort();
    let mut recovered = Vec::new();
    for path in transaction_paths {
        let mut record: RecipeTransactionRecord = read_json(&path)?;
        if !record.stage.needs_recovery() {
            if matches!(
                record.stage,
                RecipeTransactionStage::Completed
                    | RecipeTransactionStage::RolledBack
                    | RecipeTransactionStage::RollbackFailed
            ) {
                complete_recovery_fence(journal, &record)?;
            }
            continue;
        }
        let paths: Vec<_> = record
            .members
            .iter()
            .map(|member| member.target.clone())
            .collect();
        let _coordinated =
            crate::transaction::coordinate_targets(journal, &paths, Some(&record.id))?;
        rollback_recipe_record(store, &mut record)?;
        if record.stage == RecipeTransactionStage::RolledBack {
            complete_recovery_fence(journal, &record)?;
        }
        recovered.push(record.id);
    }
    Ok(recovered)
}

fn validate_prepared_recipe_mutations(
    store: &RecipeStore,
    recipe: &RecipeV1,
    prepared: &[PreparedRecipeMutation],
    previous: Option<&RecipeReceipt>,
    scope: RecipeApplyScope,
) -> Result<(), RecipeTransactionError> {
    if !recipe.dependencies.is_empty() {
        return Err(RecipeTransactionError::Stale(
            "recipe dependencies are not supported by the durable adapter".into(),
        ));
    }
    if !recipe.target.required_capabilities.is_empty() {
        return Err(RecipeTransactionError::Stale(
            "recipe capability requirements are not supported by the durable adapter".into(),
        ));
    }
    let previous_by_path: HashMap<_, _> = previous
        .into_iter()
        .flat_map(|receipt| receipt.members.iter())
        .map(|member| (normalized_path_key(&member.target), member))
        .collect();
    let expected_count = match scope {
        RecipeApplyScope::Files => recipe.installed_files.len(),
        RecipeApplyScope::Configuration => recipe.config_keys.len(),
    };
    if prepared.len() != expected_count {
        return Err(RecipeTransactionError::Stale(format!(
            "prepared mutation count {} does not match recipe count {}",
            prepared.len(),
            expected_count
        )));
    }
    for mutation in prepared {
        let target = store.target(&mutation.relative_path)?;
        if normalized_path_key(&target) != normalized_path_key(&mutation.target) {
            return Err(RecipeTransactionError::Stale(format!(
                "prepared target does not match recipe-relative destination {}",
                mutation.relative_path
            )));
        }
        let previous_member = previous_by_path.get(&normalized_path_key(&target));
        let staged = Sha256::digest(&std::fs::read(&mutation.staged)?);
        if staged != mutation.expected_after {
            return Err(RecipeTransactionError::Stale(format!(
                "staged bytes do not match expected SHA-256 for {}",
                mutation.target.display()
            )));
        }
        match scope {
            RecipeApplyScope::Files => {
                let file = recipe
                    .installed_files
                    .iter()
                    .find(|file| file.destination == mutation.relative_path)
                    .ok_or_else(|| {
                        RecipeTransactionError::Stale(format!(
                            "prepared file is not declared by recipe: {}",
                            mutation.relative_path
                        ))
                    })?;
                if mutation.artifact_id.as_deref() != Some(file.artifact_id.as_str())
                    || mutation.archive_entry != file.archive_entry
                    || mutation.expected_after != file.sha256
                    || mutation.config.is_some()
                {
                    return Err(RecipeTransactionError::Stale(format!(
                        "prepared source identity or hash differs from recipe for {}",
                        mutation.relative_path
                    )));
                }
                let staged_size = std::fs::metadata(&mutation.staged)?.len();
                if staged_size != file.size_bytes {
                    return Err(RecipeTransactionError::Stale(format!(
                        "prepared size differs from recipe for {}",
                        mutation.relative_path
                    )));
                }
                match (file.install_rule, previous_member) {
                    (InstallRule::CreateOnly, None)
                        if mutation.kind == RecipeMutationKind::Create
                            && matches!(mutation.expected_before, RecipeFilePresence::Absent) => {}
                    (InstallRule::ReplaceOwned, Some(previous_member))
                        if mutation.kind == RecipeMutationKind::Replace
                            && presence_matches(
                                &mutation.expected_before,
                                &RecipeFilePresence::Present {
                                    sha256: previous_member.owned_sha256.clone(),
                                    undo_backup: None,
                                },
                            ) => {}
                    (InstallRule::CreateOnly, Some(_)) => {
                        return Err(RecipeTransactionError::Stale(format!(
                            "create-only recipe member is already owned: {}",
                            mutation.relative_path
                        )));
                    }
                    (InstallRule::ReplaceOwned, None) => {
                        return Err(RecipeTransactionError::Stale(format!(
                            "replace-owned recipe member has no committed ownership receipt: {}",
                            mutation.relative_path
                        )));
                    }
                    _ => {
                        return Err(RecipeTransactionError::Stale(format!(
                            "prepared mutation kind or precondition differs from recipe for {}",
                            mutation.relative_path
                        )));
                    }
                }
            }
            RecipeApplyScope::Configuration => {
                let edit = recipe
                    .config_keys
                    .iter()
                    .find(|edit| edit.path == mutation.relative_path)
                    .ok_or_else(|| {
                        RecipeTransactionError::Stale(format!(
                            "prepared configuration is not declared by recipe: {}",
                            mutation.relative_path
                        ))
                    })?;
                let ownership = mutation.config.as_ref().ok_or_else(|| {
                    RecipeTransactionError::Stale(format!(
                        "prepared configuration lacks owned-key evidence: {}",
                        mutation.relative_path
                    ))
                })?;
                if mutation.kind != RecipeMutationKind::Configure
                    || mutation.artifact_id.is_some()
                    || mutation.archive_entry.is_some()
                    || ownership.selector != edit.selector
                    || ownership.owned != edit.desired
                {
                    return Err(RecipeTransactionError::Stale(format!(
                        "prepared configuration differs from recipe for {}",
                        mutation.relative_path
                    )));
                }
            }
        }
    }
    Ok(())
}

fn prepare_configuration_mutations(
    store: &RecipeStore,
    recipe: &RecipeV1,
    previous: &RecipeReceipt,
) -> Result<Vec<PreparedRecipeMutation>, RecipeTransactionError> {
    if recipe.config_keys.is_empty() {
        return Err(RecipeTransactionError::Stale(
            "recipe has no configuration edits".into(),
        ));
    }
    let mut prepared = Vec::with_capacity(recipe.config_keys.len());
    for (index, edit) in recipe.config_keys.iter().enumerate() {
        if edit.format != ConfigFormat::LosslessIni
            || edit.selector.duplicates != DuplicateKeyPolicy::Reject
            || edit.selector.case_rule != "ascii_case_insensitive"
            || !edit.preserve_unrelated_bytes
            || edit.remove_rule != ConfigRemoveRule::RestoreIfValueStillOwned
        {
            return Err(RecipeTransactionError::Stale(format!(
                "unsupported configuration semantics for {}",
                edit.path
            )));
        }
        let target = store.target(&edit.path)?;
        let bytes = match std::fs::read(&target) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let current = ini_key_value(&bytes, &edit.selector)?;
        let previous_config = previous
            .members
            .iter()
            .find(|member| normalized_path_key(&member.target) == normalized_path_key(&target))
            .and_then(|member| member.config.as_ref());
        let original = if let Some(previous_config) = previous_config {
            if current != previous_config.owned {
                return Err(RecipeTransactionError::Stale(format!(
                    "owned configuration key changed externally: {}",
                    edit.path
                )));
            }
            previous_config.original.clone()
        } else {
            if !key_condition_matches(&current, &edit.expected_before) {
                return Err(RecipeTransactionError::Stale(format!(
                    "configuration precondition differs for {}",
                    edit.path
                )));
            }
            edit.expected_before.clone()
        };
        let rendered = patch_ini_key(&bytes, &edit.selector, edit.desired.as_deref())?;
        let staged = store
            .root()
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string())
            .join(format!("{index}.ini"));
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&staged, &rendered)?;
        sync_file(&staged)?;
        let before = if bytes.is_empty() && !target.exists() {
            RecipeFilePresence::Absent
        } else {
            RecipeFilePresence::Present {
                sha256: Sha256::digest(&bytes),
                undo_backup: None,
            }
        };
        prepared.push(PreparedRecipeMutation {
            relative_path: edit.path.clone(),
            artifact_id: None,
            archive_entry: None,
            target,
            staged,
            undo_backup: None,
            expected_before: before,
            expected_after: Sha256::digest(&rendered),
            kind: RecipeMutationKind::Configure,
            config: Some(RecipeConfigOwnership {
                selector: edit.selector.clone(),
                original,
                owned: edit.desired.clone(),
            }),
        });
    }
    Ok(prepared)
}

fn rollback_recipe_record(
    store: &RecipeStore,
    record: &mut RecipeTransactionRecord,
) -> Result<(), RecipeTransactionError> {
    record.stage = RecipeTransactionStage::RollingBack;
    record.updated_at = chrono::Utc::now().to_rfc3339();
    store.save_transaction(record)?;
    let mut errors = Vec::new();
    for index in (0..record.members.len()).rev() {
        if let Some(config) = record.members[index].config.clone() {
            let current = match std::fs::read(&record.members[index].target) {
                Ok(bytes) => bytes,
                Err(error) => {
                    errors.push(error.to_string());
                    continue;
                }
            };
            let observed_value = match ini_key_value(&current, &config.selector) {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error.to_string());
                    continue;
                }
            };
            let (expected_current, desired_rollback) = match record.kind {
                RecipeTransactionKind::Apply => {
                    (config.owned.clone(), key_condition_value(&config.original))
                }
                RecipeTransactionKind::Remove => (
                    key_condition_value(&config.original).map(str::to_string),
                    config.owned.as_deref(),
                ),
            };
            if observed_value.as_deref() != expected_current.as_deref() {
                record.members[index].retained_external_change = true;
                errors.push(format!(
                    "externally changed configuration key retained: {}",
                    record.members[index].target.display()
                ));
            } else {
                match patch_ini_key(&current, &config.selector, desired_rollback)
                    .and_then(|bytes| replace_bytes(&bytes, &record.members[index].target))
                {
                    Ok(()) => {
                        record.members[index].retained_external_change = false;
                    }
                    Err(error) => errors.push(error.to_string()),
                }
            }
            record.updated_at = chrono::Utc::now().to_rfc3339();
            store.save_transaction(record)?;
            continue;
        }
        let observed = observed_presence(&record.members[index].target)?;
        if record.members[index].retained_external_change {
            if presence_matches(&observed, &record.members[index].before) {
                record.members[index].retained_external_change = false;
                store.save_transaction(record)?;
                continue;
            }
            errors.push(format!(
                "externally changed target retained: {}",
                record.members[index].target.display()
            ));
            continue;
        }
        if presence_matches(&observed, &record.members[index].before) {
            continue;
        }
        if matches!(observed, RecipeFilePresence::Absent)
            && matches!(
                record.members[index].before,
                RecipeFilePresence::Present { .. }
            )
        {
            if let Err(error) =
                restore_presence(&record.members[index].target, &record.members[index].before)
            {
                errors.push(error.to_string());
            }
            store.save_transaction(record)?;
            continue;
        }
        if !presence_matches(&observed, &record.members[index].after) {
            record.members[index].retained_external_change = true;
            errors.push(format!(
                "externally changed target retained: {}",
                record.members[index].target.display()
            ));
        } else if let Err(error) =
            restore_presence(&record.members[index].target, &record.members[index].before)
        {
            errors.push(error.to_string());
        }
        record.updated_at = chrono::Utc::now().to_rfc3339();
        store.save_transaction(record)?;
    }
    record.stage = if errors.is_empty() {
        RecipeTransactionStage::RolledBack
    } else {
        RecipeTransactionStage::RollbackFailed
    };
    if !errors.is_empty() {
        record.error = Some(errors.join("; "));
    }
    store.save_transaction(record)?;
    if record.stage == RecipeTransactionStage::RolledBack {
        reconcile_active_receipt_after_rollback(store, record)?;
    }
    Ok(())
}

fn reconcile_active_receipt_after_rollback(
    store: &RecipeStore,
    record: &RecipeTransactionRecord,
) -> Result<(), RecipeTransactionError> {
    match record.kind {
        RecipeTransactionKind::Remove => {
            let receipt = store.receipt(&record.receipt_id)?;
            store.set_active_receipt(&receipt)
        }
        RecipeTransactionKind::Apply => {
            let pointer: ActiveRecipeReceipt =
                match read_json(&store.active_path(&record.recipe_id)) {
                    Ok(pointer) => pointer,
                    Err(RecipeTransactionError::Io(error))
                        if error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
            if pointer.receipt_id != record.receipt_id || pointer.removed {
                return Ok(());
            }
            if let Some(previous_receipt_id) = &record.supersedes_receipt_id {
                let previous = store.receipt(previous_receipt_id)?;
                store.set_active_receipt(&previous)
            } else {
                store.clear_active_receipt(&record.recipe_id, &record.receipt_id)
            }
        }
    }
}

fn recovery_fence(
    record: &RecipeTransactionRecord,
    journal: &operation_journal::JournalStore,
) -> Result<operation_journal::RecoveryRecord, RecipeTransactionError> {
    Ok(operation_journal::RecoveryRecord {
        id: record.id.clone(),
        plan_id: record.operation_id.clone(),
        actor: dlssync_contracts::OperationActor::Gui,
        stage: dlssync_contracts::OperationStage::Applying,
        members: record
            .members
            .iter()
            .map(|member| operation_journal::RecoveryMember {
                target: member.target.clone(),
                backup: store_fence_backup(&member.target),
                previous_sha256: presence_hash(&member.before)
                    .map_or_else(String::new, ToString::to_string),
                expected_sha256: presence_hash(&member.after)
                    .map_or_else(String::new, ToString::to_string),
                stage: dlssync_contracts::OperationStage::Applying,
                game_id: None,
                component_id: Some(format!("recipe:{}", record.recipe_id)),
                backup_id: Some(record.receipt_id.clone()),
            })
            .collect(),
        error: None,
        source_store_id: journal.source_store_id().ok(),
        operation_id: Some(record.operation_id.clone()),
        game_ids: Vec::new(),
        kind: operation_journal::RecoveryKind::Unknown,
        started_at: Some(record.started_at.clone()),
        updated_at: Some(record.updated_at.clone()),
    })
}

fn complete_recovery_fence(
    journal: &operation_journal::JournalStore,
    record: &RecipeTransactionRecord,
) -> Result<(), RecipeTransactionError> {
    let mut fence = recovery_fence(record, journal)?;
    fence.stage = match record.stage {
        RecipeTransactionStage::Completed => dlssync_contracts::OperationStage::Completed,
        RecipeTransactionStage::RolledBack => dlssync_contracts::OperationStage::RolledBack,
        RecipeTransactionStage::RollbackFailed => dlssync_contracts::OperationStage::RollbackFailed,
        _ => dlssync_contracts::OperationStage::Applying,
    };
    fence.error = record.error.clone();
    fence.updated_at = Some(record.updated_at.clone());
    journal.save_recovery(&fence)?;
    Ok(())
}

fn store_fence_backup(target: &Path) -> PathBuf {
    target.with_extension("dlssync-recipe-fence")
}

fn verify_removed_member(path: &Path, expected: &RecipeFilePresence) -> RestorationVerification {
    match expected {
        RecipeFilePresence::Present { sha256, .. } => verify_restoration(path, sha256),
        RecipeFilePresence::Absent if !path.exists() => {
            RestorationVerification::Verified(VerifiedRestoration {
                path: path.to_path_buf(),
                observed_sha256: Sha256::digest(&[]),
            })
        }
        RecipeFilePresence::Absent => RestorationVerification::Failed(RemovalFailure::ReadFailed {
            path: path.to_path_buf(),
            reason: "created recipe target still exists after removal".into(),
        }),
    }
}

fn verify_config_restoration(
    path: &Path,
    ownership: &RecipeConfigOwnership,
) -> RestorationVerification {
    match std::fs::read(path)
        .map_err(RecipeTransactionError::from)
        .and_then(|bytes| ini_key_value(&bytes, &ownership.selector))
    {
        Ok(value) if key_condition_matches(&value, &ownership.original) => {
            RestorationVerification::Verified(VerifiedRestoration {
                path: path.to_path_buf(),
                observed_sha256: Sha256::digest(&std::fs::read(path).unwrap_or_default()),
            })
        }
        Ok(_) => RestorationVerification::Failed(RemovalFailure::ReadFailed {
            path: path.to_path_buf(),
            reason: "configuration key did not return to its receipt baseline".into(),
        }),
        Err(error) => RestorationVerification::Failed(RemovalFailure::ReadFailed {
            path: path.to_path_buf(),
            reason: error.to_string(),
        }),
    }
}

fn removal_presence_failure(
    path: &Path,
    expected: &RecipeFilePresence,
    observed: RecipeFilePresence,
) -> RemovalFailure {
    match (presence_hash(expected), presence_hash(&observed)) {
        (Some(expected), Some(observed)) => RemovalFailure::HashMismatch {
            path: path.to_path_buf(),
            expected: expected.clone(),
            observed: observed.clone(),
        },
        _ => RemovalFailure::ReadFailed {
            path: path.to_path_buf(),
            reason: "recipe-owned file presence changed externally".into(),
        },
    }
}

fn replace_from_staged(
    staged: &Path,
    target: &Path,
    expected: &Sha256,
) -> Result<(), RecipeTransactionError> {
    let parent = target.parent().ok_or_else(|| {
        RecipeTransactionError::Stale(format!("target has no parent: {}", target.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp = sibling_temp(target, "apply");
    std::fs::copy(staged, &temp)?;
    sync_file(&temp)?;
    atomic_replace(&temp, target)?;
    sync_file(target)?;
    let observed = Sha256::digest(&std::fs::read(target)?);
    if &observed != expected {
        return Err(RecipeTransactionError::Stale(format!(
            "installed bytes failed SHA-256 readback: {}",
            target.display()
        )));
    }
    Ok(())
}

fn replace_bytes(bytes: &[u8], target: &Path) -> Result<(), RecipeTransactionError> {
    let parent = target.parent().ok_or_else(|| {
        RecipeTransactionError::Stale(format!("target has no parent: {}", target.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp = sibling_temp(target, "config");
    std::fs::write(&temp, bytes)?;
    sync_file(&temp)?;
    atomic_replace(&temp, target)?;
    sync_file(target)?;
    Ok(())
}

fn ini_key_value(
    bytes: &[u8],
    selector: &KeySelector,
) -> Result<Option<String>, RecipeTransactionError> {
    let parsed = parse_ini(bytes, selector)?;
    Ok(parsed.matches.first().map(|entry| entry.value.clone()))
}

fn key_condition_matches(value: &Option<String>, condition: &KeyCondition) -> bool {
    match condition {
        KeyCondition::Absent => value.is_none(),
        KeyCondition::Equals { value: expected } => value.as_deref() == Some(expected.as_str()),
    }
}

fn key_condition_value(condition: &KeyCondition) -> Option<&str> {
    match condition {
        KeyCondition::Absent => None,
        KeyCondition::Equals { value } => Some(value),
    }
}

#[derive(Debug)]
struct IniMatch {
    line_index: usize,
    value: String,
}

#[derive(Debug)]
struct ParsedIni {
    bom: bool,
    newline: &'static str,
    trailing_newline: bool,
    lines: Vec<String>,
    section_line: Option<usize>,
    section_end: usize,
    matches: Vec<IniMatch>,
}

fn parse_ini(bytes: &[u8], selector: &KeySelector) -> Result<ParsedIni, RecipeTransactionError> {
    if selector.duplicates != DuplicateKeyPolicy::Reject
        || selector.case_rule != "ascii_case_insensitive"
        || selector.section.trim().is_empty()
        || selector.key.trim().is_empty()
    {
        return Err(RecipeTransactionError::Stale(
            "unsupported lossless INI selector semantics".into(),
        ));
    }
    let (bom, payload) = bytes
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .map_or((false, bytes), |payload| (true, payload));
    let text = std::str::from_utf8(payload)
        .map_err(|_| RecipeTransactionError::Stale("lossless INI requires UTF-8 bytes".into()))?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if newline == "\r\n" && text.replace("\r\n", "").contains('\r') {
        return Err(RecipeTransactionError::Stale(
            "mixed INI newline encoding is unsupported".into(),
        ));
    }
    let trailing_newline = text.ends_with(newline);
    let mut lines: Vec<String> = if text.is_empty() {
        Vec::new()
    } else {
        text.split(newline).map(str::to_string).collect()
    };
    if trailing_newline {
        lines.pop();
    }
    let wanted_section = selector.section.trim();
    let wanted_key = selector.key.trim();
    let mut current_section: Option<String> = None;
    let mut section_line = None;
    let mut section_end = lines.len();
    let mut matches = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(section) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            if section_line.is_some() && section_end == lines.len() {
                section_end = index;
            }
            current_section = Some(section.trim().to_string());
            if section.trim().eq_ignore_ascii_case(wanted_section) {
                if section_line.is_some() {
                    return Err(RecipeTransactionError::Stale(format!(
                        "duplicate INI section [{}]",
                        wanted_section
                    )));
                }
                section_line = Some(index);
                section_end = lines.len();
            }
            continue;
        }
        if current_section
            .as_deref()
            .is_some_and(|section| section.eq_ignore_ascii_case(wanted_section))
            && !trimmed.starts_with(';')
            && !trimmed.starts_with('#')
        {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case(wanted_key) {
                    matches.push(IniMatch {
                        line_index: index,
                        value: value.trim().to_string(),
                    });
                }
            }
        }
    }
    if matches.len() > 1 {
        return Err(RecipeTransactionError::Stale(format!(
            "duplicate INI key [{}] {}",
            wanted_section, wanted_key
        )));
    }
    Ok(ParsedIni {
        bom,
        newline,
        trailing_newline,
        lines,
        section_line,
        section_end,
        matches,
    })
}

fn patch_ini_key(
    bytes: &[u8],
    selector: &KeySelector,
    desired: Option<&str>,
) -> Result<Vec<u8>, RecipeTransactionError> {
    if desired.is_some_and(|value| value.contains(['\r', '\n'])) {
        return Err(RecipeTransactionError::Stale(
            "INI values cannot contain newlines".into(),
        ));
    }
    let mut parsed = parse_ini(bytes, selector)?;
    match (parsed.matches.first(), desired) {
        (Some(found), Some(value)) => {
            let line = &parsed.lines[found.line_index];
            let equals = line.find('=').expect("matched INI line contains equals");
            let prefix = &line[..=equals];
            let leading = line[equals + 1..]
                .chars()
                .take_while(|character| character.is_ascii_whitespace())
                .collect::<String>();
            parsed.lines[found.line_index] = format!("{prefix}{leading}{value}");
        }
        (Some(found), None) => {
            parsed.lines.remove(found.line_index);
        }
        (None, Some(value)) => {
            let insert_at = if let Some(section_line) = parsed.section_line {
                parsed.section_end.max(section_line + 1)
            } else {
                if !parsed.lines.is_empty() && !parsed.lines.last().is_some_and(String::is_empty) {
                    parsed.lines.push(String::new());
                }
                parsed.lines.push(format!("[{}]", selector.section.trim()));
                parsed.lines.len()
            };
            parsed
                .lines
                .insert(insert_at, format!("{}={value}", selector.key.trim()));
        }
        (None, None) => {}
    }
    let mut text = parsed.lines.join(parsed.newline);
    if parsed.trailing_newline && !text.is_empty() {
        text.push_str(parsed.newline);
    }
    let mut output = Vec::with_capacity(text.len() + usize::from(parsed.bom) * 3);
    if parsed.bom {
        output.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    output.extend_from_slice(text.as_bytes());
    Ok(output)
}

fn restore_presence(
    target: &Path,
    presence: &RecipeFilePresence,
) -> Result<(), RecipeTransactionError> {
    match presence {
        RecipeFilePresence::Absent => {
            if target.exists() {
                std::fs::remove_file(target)?;
            }
        }
        RecipeFilePresence::Present {
            sha256,
            undo_backup,
        } => {
            let backup = undo_backup.as_ref().ok_or_else(|| {
                RecipeTransactionError::Recovery(format!(
                    "missing undo backup for {}",
                    target.display()
                ))
            })?;
            replace_from_staged(backup, target, sha256)?;
        }
    }
    Ok(())
}

fn copy_verified_new(
    source: &Path,
    backup: &Path,
    expected: &Sha256,
) -> Result<(), RecipeTransactionError> {
    if backup.try_exists()? {
        let existing = Sha256::digest(&std::fs::read(backup)?);
        if &existing == expected {
            return Ok(());
        }
        return Err(RecipeTransactionError::Stale(format!(
            "undo backup already exists with different bytes: {}",
            backup.display()
        )));
    }
    let parent = backup.parent().ok_or_else(|| {
        RecipeTransactionError::Stale(format!("backup has no parent: {}", backup.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp = sibling_temp(backup, "backup");
    std::fs::copy(source, &temp)?;
    sync_file(&temp)?;
    let observed = Sha256::digest(&std::fs::read(&temp)?);
    if &observed != expected {
        return Err(RecipeTransactionError::Stale(format!(
            "source changed while creating undo backup: {}",
            source.display()
        )));
    }
    std::fs::rename(temp, backup)?;
    sync_file(backup)?;
    Ok(())
}

fn observed_presence(path: &Path) -> Result<RecipeFilePresence, RecipeTransactionError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(RecipeFilePresence::Present {
            sha256: Sha256::digest(&bytes),
            undo_backup: None,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RecipeFilePresence::Absent)
        }
        Err(error) => Err(error.into()),
    }
}

fn verify_presence(
    path: &Path,
    expected: &RecipeFilePresence,
) -> Result<(), RecipeTransactionError> {
    let observed = observed_presence(path)?;
    if presence_matches(&observed, expected) {
        Ok(())
    } else {
        Err(RecipeTransactionError::Stale(format!(
            "target changed before mutation: {}",
            path.display()
        )))
    }
}

fn presence_matches(left: &RecipeFilePresence, right: &RecipeFilePresence) -> bool {
    match (left, right) {
        (RecipeFilePresence::Absent, RecipeFilePresence::Absent) => true,
        (
            RecipeFilePresence::Present { sha256: left, .. },
            RecipeFilePresence::Present { sha256: right, .. },
        ) => left == right,
        _ => false,
    }
}

fn presence_hash(presence: &RecipeFilePresence) -> Option<&Sha256> {
    match presence {
        RecipeFilePresence::Absent => None,
        RecipeFilePresence::Present { sha256, .. } => Some(sha256),
    }
}

fn unique_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
) -> Result<Vec<PathBuf>, RecipeTransactionError> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for path in paths {
        let key = normalized_path_key(path);
        if !seen.insert(key) {
            return Err(RecipeTransactionError::Stale(format!(
                "recipe mutation target is duplicated: {}",
                path.display()
            )));
        }
        unique.push(path.clone());
    }
    Ok(unique)
}

fn normalized_path_key(path: &Path) -> String {
    path.canonicalize()
        .or_else(|error| {
            let parent = path.parent().ok_or(error)?;
            parent
                .canonicalize()
                .map(|canonical| canonical.join(path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RecipeTransactionError> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Ok(value),
            Err(error) => {
                let previous = previous_state_path(path);
                if previous.is_file() {
                    Ok(serde_json::from_slice(&std::fs::read(previous)?)?)
                } else {
                    Err(error.into())
                }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let previous = previous_state_path(path);
            if previous.is_file() {
                Ok(serde_json::from_slice(&std::fs::read(previous)?)?)
            } else {
                Err(error.into())
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn write_json_atomic_new<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), RecipeTransactionError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json_atomic_replace<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), RecipeTransactionError> {
    let parent = path.parent().ok_or_else(|| {
        RecipeTransactionError::Stale(format!("state path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp = sibling_temp(path, "state");
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    atomic_replace(&temp, path)?;
    sync_file(path)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), RecipeTransactionError> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    // std::fs supports long paths, but raw Win32 calls need extended-length paths.
    // Recipe state names contain hashes and can exceed MAX_PATH in normal profiles.
    let source = source.canonicalize()?;
    let target = target
        .parent()
        .ok_or_else(|| std::io::Error::other("replacement target has no parent"))?
        .canonicalize()?
        .join(target.file_name().unwrap_or_default());
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<(), RecipeTransactionError> {
    std::fs::rename(source, target)?;
    Ok(())
}

fn previous_state_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.previous"))
}

fn restore_interrupted_state_files(directory: &Path) -> Result<(), RecipeTransactionError> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(primary_name) = name.strip_suffix(".previous") else {
            continue;
        };
        let primary = path.with_file_name(primary_name);
        if primary.exists() {
            std::fs::remove_file(path)?;
        } else {
            std::fs::rename(path, primary)?;
        }
    }
    Ok(())
}

fn sibling_temp(path: &Path, purpose: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{purpose}.{}.tmp", uuid::Uuid::new_v4()))
}

fn sync_file(path: &Path) -> Result<(), RecipeTransactionError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    Ok(())
}

fn safe_name(value: &str) -> String {
    hex::encode(Sha256Hasher::digest(value.as_bytes()))
}

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
