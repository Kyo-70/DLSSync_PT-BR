use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeCatalog {
    pub schema_version: u16,
    pub acquisition_requires_user_action: bool,
    pub recipes: Vec<RecipeDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeDescriptor {
    pub schema_version: u16,
    pub id: String,
    pub revision: u32,
    pub upstream_version: String,
    pub experimental: bool,
    pub manifest_sha256: String,
    pub state: RecipeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeState {
    pub ownership: RecipeOwnershipState,
    pub observation: RecipeObservationState,
    pub compatibility: RecipeCompatibilityState,
    pub support: RecipeSupportState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecipeOwnershipState {
    None,
    InstalledByDlssync { receipt_id: String },
    OwnedModified { paths: Vec<String> },
    OwnedMissing { paths: Vec<String> },
    PartiallyRemoved { retained_paths: Vec<String> },
    Removed { receipt_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecipeObservationState {
    NotObserved,
    FilesDetected {
        method: String,
        paths: Vec<String>,
        hashes: Vec<String>,
        observed_at: String,
    },
    RuntimeDetected {
        process_identity: String,
        module_paths: Vec<String>,
        observed_at: String,
    },
    UnknownOrInaccessible {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecipeCompatibilityState {
    Unknown,
    DocumentedOnly { evidence_ids: Vec<String> },
    TestedPass { evidence_id: String },
    TestedFail { evidence_id: String },
    Stale { previous_evidence_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecipeSupportState {
    Unknown,
    Experimental,
    Official { evidence_ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeValidationRequest {
    pub recipe_json: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeValidationResult {
    pub valid: bool,
    pub recipe: Option<RecipeDescriptor>,
    pub issue: Option<RecipeIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeIssue {
    pub code: String,
    pub message: String,
    pub context: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OfficialComponentClaim {
    pub component_id: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExistingRecipeFileClaim {
    pub recipe_id: String,
    pub path: String,
    pub role: RecipeFileRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipeFileRole {
    Proxy,
    Addon,
    Shader,
    Data,
    Notice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeConflictPreviewRequest {
    pub recipe_json: String,
    pub expected_sha256: String,
    pub official_components: Vec<OfficialComponentClaim>,
    pub existing_recipe_claims: Vec<ExistingRecipeFileClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeConflictPreviewResult {
    pub allowed: bool,
    pub conflicts: Vec<RecipeConflict>,
    pub issue: Option<RecipeIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeConflict {
    pub kind: RecipeConflictKind,
    pub recipe_id: String,
    pub other_party: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipeConflictKind {
    OfficialComponentPath,
    RecipePath,
    UnsupportedProxyFilename,
    UnsupportedProxyChain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeRemovalRequest {
    pub operation_id: String,
    pub recipe_id: String,
    pub receipt_id: String,
    pub restorations: Vec<RecipeRestorationCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeRestorationCheck {
    pub path: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipeRemovalStatus {
    Removed,
    Failed,
}

impl RecipeRemovalStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeRemovalResult {
    pub operation_id: String,
    pub recipe_id: String,
    pub receipt_id: String,
    pub status: RecipeRemovalStatus,
    pub verified_restorations: Vec<RecipeVerifiedRestoration>,
    pub failures: Vec<RecipeRemovalFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeVerifiedRestoration {
    pub path: String,
    pub observed_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecipeRemovalFailure {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub expected_sha256: Option<String>,
    pub observed_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipePreviewIntent {
    Apply,
    Configure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeLocalPreviewRequest {
    pub game_id: String,
    pub recipe_path: String,
    pub source_directory: Option<String>,
    pub intent: RecipePreviewIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipePreviewAction {
    Create,
    Replace,
    Configure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipePreviewFile {
    pub role: RecipeFileRole,
    pub relative_path: String,
    pub action: RecipePreviewAction,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipePreviewConfigEdit {
    pub relative_path: String,
    pub section: String,
    pub key: String,
    pub desired: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeLocalPreviewResult {
    pub preview_id: String,
    pub expires_at: String,
    pub allowed: bool,
    pub recipe: Option<RecipeDescriptor>,
    pub files: Vec<RecipePreviewFile>,
    pub config_edits: Vec<RecipePreviewConfigEdit>,
    pub issues: Vec<RecipeIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeApplyRequest {
    pub preview_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeConfigureRequest {
    pub preview_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeDurableRemovalRequest {
    pub game_id: String,
    pub recipe_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRecipeListRequest {
    pub game_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RecipeOperationStatus {
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecipeOperationResult {
    pub operation_id: String,
    pub game_id: String,
    pub recipe_id: String,
    pub receipt_id: Option<String>,
    pub status: RecipeOperationStatus,
    pub changed_paths: Vec<String>,
    pub retained_paths: Vec<String>,
    pub issues: Vec<RecipeIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRecipeMember {
    pub relative_path: String,
    pub action: RecipePreviewAction,
    pub owned_sha256: String,
    pub configuration_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRecipeReceipt {
    pub upstream_version: String,
    pub state: RecipeState,
    pub recipe_id: String,
    pub revision: u32,
    pub receipt_id: String,
    pub installation_id: String,
    pub created_at: String,
    pub members: Vec<OwnedRecipeMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRecipeListResult {
    pub game_id: String,
    pub recipes: Vec<OwnedRecipeReceipt>,
}
