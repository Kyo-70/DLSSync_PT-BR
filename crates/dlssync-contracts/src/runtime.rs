//! Runtime contracts. Observations and compatibility decisions belong to Rust.
use crate::{ApiError, OperationActor, OperationKind, OperationRecord};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86,
    X64,
    Arm64,
    Arm64Ec,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha256,
    Md5,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    pub digest: String,
}

impl ContentHash {
    pub fn is_valid(&self) -> bool {
        let length = match self.algorithm {
            HashAlgorithm::Sha256 => 64,
            HashAlgorithm::Md5 => 32,
        };
        self.digest.len() == length && self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

/// Decimal bytes preserve the full u64 range across JSON and JavaScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ByteCount(pub String);

impl From<u64> for ByteCount {
    fn from(value: u64) -> Self {
        Self(value.to_string())
    }
}

/// Canonical unsigned decimal counter safe for JavaScript consumers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Counter(pub String);

impl Counter {
    pub fn new(value: u64) -> Self {
        Self(value.to_string())
    }

    pub fn parse(&self) -> Option<u64> {
        if self.0.is_empty()
            || (self.0.len() > 1 && self.0.starts_with('0'))
            || !self.0.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        self.0.parse().ok()
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl From<u64> for Counter {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ComponentIdentity {
    pub game_id: String,
    pub relative_path: String,
    pub family: String,
    pub filename: String,
    pub architecture: Architecture,
    pub graphics_api: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SignatureStatus {
    NotChecked,
    Missing,
    Verified,
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ArtifactDescriptor {
    pub id: String,
    pub family: String,
    pub filename: String,
    pub file_version: Option<String>,
    pub package_version: String,
    pub package_id: String,
    pub compatibility_line: String,
    pub architecture: Architecture,
    pub hash: ContentHash,
    pub size_bytes: ByteCount,
    pub source_url: String,
    pub archive_entry: Option<String>,
    pub expected_publisher: Option<String>,
    pub observed_publisher: Option<String>,
    pub signature_status: SignatureStatus,
    pub dependencies: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Compatible,
    Incompatible,
    Unknown,
    Experimental,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Evidence {
    pub source: String,
    pub observed_at: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CompatibilityDecision {
    pub status: CompatibilityStatus,
    pub reason_code: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Unchecked,
    Current,
    UpdateAvailable,
    Newer,
    Disabled,
    ExternallyManaged,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    Official,
    Experimental,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ApplicabilityStatus {
    Applicable,
    NotApplicable,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationErrorCode {
    ReadDenied,
    Missing,
    ParseFailed,
    ChangedDuringRead,
    Unmeasured,
    InaccessibleDirectory,
    WatcherLost,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ObservationError {
    pub code: ObservationErrorCode,
    pub path: Option<String>,
    pub detail: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ComponentState {
    #[serde(default)]
    pub component_id: String,
    pub identity: ComponentIdentity,
    pub observed_version: Option<String>,
    pub observed_hash: Option<ContentHash>,
    pub candidate: Option<ArtifactDescriptor>,
    pub status: ComponentStatus,
    pub compatibility: CompatibilityDecision,
    #[serde(default)]
    pub support: SupportStatus,
    #[serde(default)]
    pub applicability: ApplicabilityStatus,
    pub owner: Option<String>,
    pub checked_at: Option<String>,
    #[serde(default)]
    pub observation_complete: bool,
    #[serde(default)]
    pub observation_errors: Vec<ObservationError>,
    #[serde(default)]
    pub catalog_revision: Option<String>,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    Planned,
    Blocked,
    Downloading,
    Verifying,
    BackingUp,
    Applying,
    VerifyingInstalled,
    Completed,
    Cancelled,
    RollingBack,
    RolledBack,
    RollbackFailed,
}

impl OperationStage {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Blocked
                | Self::Completed
                | Self::Cancelled
                | Self::RolledBack
                | Self::RollbackFailed
        )
    }
    pub const fn can_cancel_immediately(self) -> bool {
        matches!(
            self,
            Self::Planned | Self::Downloading | Self::Verifying | Self::BackingUp
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MeasuredProgress {
    pub bytes_received: ByteCount,
    pub bytes_total: Option<ByteCount>,
    pub files_verified: u32,
    pub files_total: Option<u32>,
    #[serde(default)]
    pub measurement_basis: MeasurementBasis,
    #[serde(default)]
    pub estimated_seconds_remaining: Option<u64>,
    #[serde(default)]
    pub transfer_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementBasis {
    Bytes,
    VerifiedFiles,
    #[default]
    Activity,
}

impl Default for MeasuredProgress {
    fn default() -> Self {
        Self {
            bytes_received: ByteCount::from(0),
            bytes_total: None,
            files_verified: 0,
            files_total: None,
            measurement_basis: MeasurementBasis::Activity,
            estimated_seconds_remaining: None,
            transfer_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ComponentResult {
    pub identity: ComponentIdentity,
    pub stage: OperationStage,
    pub installed_hash: Option<ContentHash>,
    pub installed_version: Option<String>,
    pub backup_id: Option<String>,
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationSnapshot {
    pub id: String,
    pub plan_id: String,
    pub actor: OperationActor,
    #[serde(default)]
    pub kind: OperationKind,
    #[serde(default)]
    pub game_ids: Vec<String>,
    #[serde(default)]
    pub parent_operation_id: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    pub sequence: Counter,
    pub stage: OperationStage,
    pub progress: MeasuredProgress,
    pub results: Vec<ComponentResult>,
    pub cancel_requested: bool,
    pub error: Option<ApiError>,
    pub state_revision: String,
    pub updated_at: String,
}

/// Each event includes the authoritative snapshot so reconnection does not
/// require replaying every byte-level progress increment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationEvent {
    pub operation_id: String,
    pub sequence: Counter,
    pub component: Option<ComponentIdentity>,
    pub snapshot: OperationSnapshot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GameStateStatus {
    #[default]
    Unchecked,
    Current,
    UpdateAvailable,
    Unknown,
    NoComponents,
    NonActionable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct GameSnapshot {
    pub id: String,
    pub name: String,
    pub install_dir: String,
    #[serde(default)]
    pub launcher: Option<String>,
    #[serde(default)]
    pub art_url: Option<String>,
    #[serde(default)]
    pub revision: Counter,
    #[serde(default)]
    pub checked_at: Option<String>,
    #[serde(default)]
    pub observation_complete: bool,
    #[serde(default)]
    pub observation_errors: Vec<ObservationError>,
    #[serde(default)]
    pub status: GameStateStatus,
    #[serde(default)]
    pub components: Vec<ComponentState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackupKind {
    #[default]
    GameDll,
    DriverPackage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackupAvailability {
    VerifiedPresent,
    VerifiedAbsent,
    #[default]
    Unverified,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RestoreEligibilityView {
    pub eligible: bool,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RestoreVerification {
    pub verified: bool,
    pub at: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BackupView {
    pub id: String,
    pub game_id: Option<String>,
    pub component_id: Option<String>,
    pub operation_id: Option<String>,
    pub original_path: String,
    pub backup_path: String,
    pub sha256: Option<String>,
    pub kind: BackupKind,
    pub availability: BackupAvailability,
    pub restore_eligibility: RestoreEligibilityView,
    pub last_restore: Option<RestoreVerification>,
    /// Compatibility projection. New consumers must use `availability`.
    #[serde(default)]
    pub verified_available: bool,
    /// Compatibility projection. New consumers must use `last_restore`.
    pub restored_at: Option<String>,
    #[serde(default)]
    pub revision: Counter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HistoryView {
    pub id: String,
    pub source_store_id: String,
    pub source_record_id: String,
    pub historical_at: Option<String>,
    pub operation_id: Option<String>,
    pub game_id: Option<String>,
    pub component_id: Option<String>,
    pub recovery_outcome: Option<OperationStage>,
    pub record: OperationRecord,
    #[serde(default)]
    pub revision: Counter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    Cache,
    Embedded,
    Remote,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRemoteResult {
    #[default]
    Never,
    Modified,
    NotModified,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogState {
    pub content_revision: Option<String>,
    pub generated_at: Option<String>,
    #[serde(default)]
    pub source: CatalogSource,
    pub signature_verified: bool,
    pub public_key_fingerprint: Option<String>,
    pub automatic_refresh_enabled: bool,
    pub manual_refresh_enabled: bool,
    pub app_updates_enabled: bool,
    pub last_remote_attempt_at: Option<String>,
    pub last_remote_success_at: Option<String>,
    #[serde(default)]
    pub last_remote_result: CatalogRemoteResult,
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StateCounts {
    pub actionable_games: u32,
    pub actionable_components: u32,
    pub protected_games: u32,
    pub eligible_restorable_backups: u32,
    pub active_operations: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StateDelta {
    #[serde(default)]
    pub base_revision: Counter,
    #[serde(default)]
    pub affected_game_ids: Vec<String>,
    #[serde(default)]
    pub games: Vec<GameSnapshot>,
    #[serde(default)]
    pub removed_game_ids: Vec<String>,
    #[serde(default)]
    pub operations: Vec<OperationSnapshot>,
    #[serde(default)]
    pub removed_operation_ids: Vec<String>,
    #[serde(default)]
    pub backups: Vec<BackupView>,
    #[serde(default)]
    pub removed_backup_ids: Vec<String>,
    #[serde(default)]
    pub history: Vec<HistoryView>,
    #[serde(default)]
    pub removed_history_ids: Vec<String>,
    pub catalog: Option<CatalogState>,
    #[serde(default)]
    pub counts: StateCounts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuthoritativeSnapshot {
    pub schema_version: u16,
    pub emitter_id: String,
    #[serde(default)]
    pub sequence: Counter,
    #[serde(default)]
    pub revision: Counter,
    pub captured_at: String,
    #[serde(default)]
    pub games: Vec<GameSnapshot>,
    #[serde(default)]
    pub operations: Vec<OperationSnapshot>,
    #[serde(default)]
    pub backups: Vec<BackupView>,
    #[serde(default)]
    pub history: Vec<HistoryView>,
    #[serde(default)]
    pub history_sync_pending: bool,
    #[serde(default)]
    pub catalog: CatalogState,
    #[serde(default)]
    pub counts: StateCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StateEvent {
    pub schema_version: u16,
    pub id: String,
    pub emitter_id: String,
    pub sequence: Counter,
    pub operation_id: Option<String>,
    pub game_id: Option<String>,
    pub revision: Counter,
    pub emitted_at: String,
    pub delta: StateDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StateWatermark {
    pub emitter_id: String,
    pub sequence: Counter,
    pub revision: Counter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FilePrecondition {
    pub identity: ComponentIdentity,
    pub absolute_path: String,
    pub observed_hash: ContentHash,
    pub observed_version: Option<String>,
    pub size_bytes: ByteCount,
    pub checked_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PlannedChange {
    pub precondition: FilePrecondition,
    pub artifact: ArtifactDescriptor,
    pub compatibility: CompatibilityDecision,
    pub added_as_dependency: bool,
    pub set_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DriverInstallPlan {
    pub id: String,
    pub device_instance_id: String,
    pub hardware_ids: Vec<String>,
    pub vendor: String,
    pub channel: String,
    pub installed_version: Option<String>,
    pub package: ArtifactDescriptor,
    pub compatibility: CompatibilityDecision,
    pub recovery_backup_id: Option<String>,
    pub requires_elevation: bool,
    pub reboot_may_be_required: bool,
    pub checked_at: String,
    pub fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn md5_cannot_be_presented_as_sha256() {
        let mut hash = ContentHash {
            algorithm: HashAlgorithm::Md5,
            digest: "a".repeat(32),
        };
        assert!(hash.is_valid());
        hash.algorithm = HashAlgorithm::Sha256;
        assert!(!hash.is_valid());
    }
    #[test]
    fn recovery_and_cancellation_have_distinct_boundaries() {
        assert!(!OperationStage::RollingBack.is_terminal());
        assert!(OperationStage::RollbackFailed.is_terminal());
        assert!(!OperationStage::Applying.can_cancel_immediately());
        assert!(OperationStage::Downloading.can_cancel_immediately());
        assert_ne!(
            serde_json::to_string(&OperationStage::RolledBack).unwrap(),
            serde_json::to_string(&OperationStage::RollbackFailed).unwrap()
        );
    }
    #[test]
    fn byte_counts_round_trip_without_javascript_precision_loss() {
        let count = ByteCount::from(u64::MAX);
        let json = serde_json::to_string(&count).unwrap();
        assert_eq!(json, "\"18446744073709551615\"");
        assert_eq!(serde_json::from_str::<ByteCount>(&json).unwrap(), count);
    }

    #[test]
    fn phase4_support_and_applicability_are_independent_states() {
        let official = SupportStatus::Official;
        let experimental = SupportStatus::Experimental;
        let unknown = SupportStatus::Unknown;
        let applicable = ApplicabilityStatus::Applicable;

        assert_ne!(official, experimental);
        assert_ne!(experimental, unknown);
        assert_eq!(applicable, ApplicabilityStatus::Applicable);
        assert_eq!(serde_json::to_string(&official).unwrap(), "\"official\"");
        assert_eq!(
            serde_json::to_string(&experimental).unwrap(),
            "\"experimental\""
        );
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"unknown\"");
        assert_eq!(
            serde_json::to_string(&applicable).unwrap(),
            "\"applicable\""
        );
    }

    #[test]
    fn phase4_counter_rejects_noncanonical_values() {
        assert_eq!(Counter::from(u64::MAX).parse(), Some(u64::MAX));
        assert_eq!(Counter("0".into()).parse(), Some(0));
        assert_eq!(Counter("01".into()).parse(), None);
        assert_eq!(Counter("-1".into()).parse(), None);
        assert_eq!(Counter(String::new()).parse(), None);
    }

    #[test]
    fn phase10_backup_contract_exposes_verified_restore_semantics() {
        let backup = BackupView::default();
        assert_eq!(backup.kind, BackupKind::GameDll);
        assert_eq!(backup.availability, BackupAvailability::Unverified);
        assert!(!backup.restore_eligibility.eligible);
        assert!(backup.last_restore.is_none());
        assert_eq!(StateCounts::default().protected_games, 0);
    }
}
