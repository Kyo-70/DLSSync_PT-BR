use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsWriteState {
    NotAttempted,
    Accepted,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsReadbackState {
    NotAttempted,
    Matched,
    Mismatched,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDocumentationState {
    Unknown,
    DocumentedNumericDefinition,
    DocumentedRuntimeApplicability,
    NotDocumented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InGameBehaviorState {
    Unknown,
    ObservedEffective,
    ObservedIneffective,
}

impl InGameBehaviorState {
    pub const fn was_observed_effective(self) -> bool {
        matches!(self, Self::ObservedEffective)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PresetEvidence {
    pub write: DrsWriteState,
    pub readback: DrsReadbackState,
    pub provider_documentation: ProviderDocumentationState,
    pub in_game_behavior: InGameBehaviorState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "value_type", content = "value")]
pub enum RawDrsValue {
    Dword(u32),
    Binary(Vec<u8>),
    Unicode(Vec<u16>),
    Unknown { setting_type: u32, bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "presence")]
pub enum LocalSettingState {
    Absent,
    Present { value: RawDrsValue },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsSettingLocation {
    CurrentApplication,
    CurrentGlobal,
    BaseProfile,
    DriverDefault,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DrsSettingObservation {
    pub setting_id: u32,
    pub local: LocalSettingState,
    pub effective_value: Option<RawDrsValue>,
    pub predefined: bool,
    pub predefined_value: Option<RawDrsValue>,
    pub location: DrsSettingLocation,
    pub resolved_profile: Option<String>,
    pub matched_application: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum SettingPatch {
    Keep,
    Set { value: RawDrsValue },
    RemoveLocal,
}
