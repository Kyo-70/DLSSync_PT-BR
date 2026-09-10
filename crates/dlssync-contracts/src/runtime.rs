//! Runtime contracts. Observations and compatibility decisions belong to Rust.
use crate::{ApiError, OperationActor};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ComponentState {
    pub identity: ComponentIdentity,
    pub observed_version: Option<String>,
    pub observed_hash: Option<ContentHash>,
    pub candidate: Option<ArtifactDescriptor>,
    pub status: ComponentStatus,
    pub compatibility: CompatibilityDecision,
    pub owner: Option<String>,
    pub checked_at: String,
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
    pub files_total: u32,
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
    pub sequence: u32,
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
    pub sequence: u32,
    pub component: Option<ComponentIdentity>,
    pub snapshot: OperationSnapshot,
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
}
