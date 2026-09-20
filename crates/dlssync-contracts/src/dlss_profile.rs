use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

pub const FG_PRESET_DEFAULT: u32 = 0x00FF_FFFE;
pub const PRESET_LATEST: u32 = 0x00FF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DlssFeature {
    Sr,
    Rr,
    Fg,
    Nr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PresetValueError {
    pub feature: DlssFeature,
    pub raw: u32,
}

impl fmt::Display for PresetValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "0x{:08X} is not valid for {:?}",
            self.raw, self.feature
        )
    }
}

impl std::error::Error for PresetValueError {}

macro_rules! preset_enum {
    ($name:ident, $feature:expr, { $($variant:ident = $raw:expr),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn raw_value(self) -> u32 {
                match self {
                    $(Self::$variant => $raw),+
                }
            }
        }

        impl TryFrom<u32> for $name {
            type Error = PresetValueError;

            fn try_from(raw: u32) -> Result<Self, Self::Error> {
                $(if raw == $raw {
                    return Ok(Self::$variant);
                })+
                Err(PresetValueError {
                    feature: $feature,
                    raw,
                })
            }
        }
    };
}

preset_enum!(SrPreset, DlssFeature::Sr, {
    Off = 0,
    A = 1,
    B = 2,
    C = 3,
    D = 4,
    E = 5,
    F = 6,
    G = 7,
    H = 8,
    I = 9,
    J = 10,
    K = 11,
    L = 12,
    M = 13,
    N = 14,
    O = 15,
    Latest = PRESET_LATEST,
});

preset_enum!(RrPreset, DlssFeature::Rr, {
    Off = 0,
    A = 1,
    B = 2,
    C = 3,
    D = 4,
    E = 5,
    F = 6,
    G = 7,
    H = 8,
    I = 9,
    J = 10,
    K = 11,
    L = 12,
    M = 13,
    N = 14,
    O = 15,
    Latest = PRESET_LATEST,
});

preset_enum!(FgPreset, DlssFeature::Fg, {
    Off = 0,
    A = 1,
    B = 2,
    C = 3,
    D = 4,
    E = 5,
    F = 6,
    G = 7,
    H = 8,
    I = 9,
    J = 10,
    K = 11,
    L = 12,
    M = 13,
    N = 14,
    O = 15,
    P = 16,
    Q = 17,
    R = 18,
    S = 19,
    T = 20,
    U = 21,
    V = 22,
    W = 23,
    X = 24,
    Y = 25,
    Z = 26,
    Default = FG_PRESET_DEFAULT,
    Latest = PRESET_LATEST,
});

preset_enum!(NrPreset, DlssFeature::Nr, {
    Off = 0,
    A = 1,
    B = 2,
    C = 3,
    D = 4,
    Latest = PRESET_LATEST,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PresetReadOnlyReason {
    ProviderRuntimeWriteSupportUnverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum PresetWriteMapping {
    Writable,
    ReadOnly { reason: PresetReadOnlyReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DlssFrameGenMode {
    AppControlled,
    Fixed,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DlssFrameGenCount {
    AppControlled,
    X2,
    X3,
    X4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Type)]
pub struct DlssPresetWriteConfig {
    pub enable_sr_dll_override: bool,
    pub sr_preset: Option<SrPreset>,
    pub enable_rr_dll_override: bool,
    pub rr_preset: Option<RrPreset>,
    pub enable_fg_dll_override: bool,
    pub fg_preset: Option<FgPreset>,
    pub fg_mode: Option<DlssFrameGenMode>,
    pub fg_fixed_count: Option<DlssFrameGenCount>,
    pub fg_dynamic_target_fps: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PresetNamespaceIds {
    pub override_id: u32,
    pub preset_id: u32,
}

macro_rules! preset_definition {
    ($name:ident, $preset:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
        pub struct $name {
            pub preset: $preset,
            pub raw_value: u32,
            pub description: String,
            pub write_mapping: PresetWriteMapping,
        }
    };
}

preset_definition!(SrPresetDefinition, SrPreset);
preset_definition!(RrPresetDefinition, RrPreset);
preset_definition!(FgPresetDefinition, FgPreset);
preset_definition!(NrPresetDefinition, NrPreset);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SrPresetRegistry {
    pub setting_ids: PresetNamespaceIds,
    pub options: Vec<SrPresetDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RrPresetRegistry {
    pub setting_ids: PresetNamespaceIds,
    pub options: Vec<RrPresetDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FgPresetRegistry {
    pub setting_ids: PresetNamespaceIds,
    pub options: Vec<FgPresetDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NrPresetRegistry {
    pub setting_ids: PresetNamespaceIds,
    pub options: Vec<NrPresetDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DlssPresetRegistry {
    pub sr: SrPresetRegistry,
    pub rr: RrPresetRegistry,
    pub fg: FgPresetRegistry,
    pub nr: NrPresetRegistry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "platform")]
pub enum HostPlatform {
    Windows,
    Other { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum NvapiRuntime {
    Available { version: String },
    Unavailable { reason: String },
    VersionUnknown { detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AdapterClass {
    Dedicated,
    Integrated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NvidiaDriverVersion {
    pub branch: u16,
    pub revision: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AmdDriverVersion {
    pub vendor_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct IntelDriverVersion {
    pub vendor_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "provider")]
pub enum AdapterProvider {
    Nvidia {
        driver_version: Option<NvidiaDriverVersion>,
    },
    Amd {
        driver_version: Option<AmdDriverVersion>,
    },
    Intel {
        driver_version: Option<IntelDriverVersion>,
    },
    Unknown {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AdapterObservation {
    pub class: AdapterClass,
    pub provider: AdapterProvider,
    pub hardware_recognized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Confirmed,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CapabilityEvidence {
    pub host: HostPlatform,
    pub nvapi: NvapiRuntime,
    pub adapter: AdapterObservation,
    pub provider_applicability: SupportState,
    pub game_integration: SupportState,
    pub runtime_stack: SupportState,
    pub preset_runtime_mapping: SupportState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityBlockReason {
    HostNotWindows,
    NvapiUnavailable,
    NvapiVersionUnknown,
    AdapterNotNvidia,
    NvidiaDriverVersionUnknown,
    AdapterClassUnknown,
    HardwareUnrecognized,
    ProviderApplicabilityUnknown,
    ProviderIncompatible,
    GameIntegrationUnknown,
    GameIntegrationIncompatible,
    RuntimeStackUnknown,
    RuntimeStackIncompatible,
    PresetRuntimeMappingUnknown,
    PresetRuntimeMappingIncompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CapabilityAssessment {
    pub provider_documented_namespace: bool,
    pub write_eligible: bool,
    pub block_reasons: Vec<CapabilityBlockReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DlssCapabilityReport {
    pub adapter_model: String,
    pub pci_vendor_id: u16,
    pub pci_device_id: u16,
    pub evidence: CapabilityEvidence,
    pub assessment: CapabilityAssessment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DlssCapabilitySnapshot {
    pub presets: DlssPresetRegistry,
    pub adapters: Vec<DlssCapabilityReport>,
    /// Installed-driver profile persistence. This does not establish in-game support or effects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_access: Option<DrsProfileAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DrsProfileAccess {
    pub interface_version: String,
    pub driver_version: u32,
    pub driver_branch: String,
    pub setting_ids: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsWriteState {
    NotAttempted,
    Accepted,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsReadbackState {
    NotAttempted,
    Matched,
    Mismatched,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDocumentationState {
    Unknown,
    DocumentedNumericDefinition,
    DocumentedRuntimeApplicability,
    NotDocumented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InGameBehaviorState {
    Unknown,
    ObservedEffective,
    ObservedIneffective,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PresetEvidence {
    pub write: DrsWriteState,
    pub readback: DrsReadbackState,
    pub provider_documentation: ProviderDocumentationState,
    pub in_game_behavior: InGameBehaviorState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "value_type", content = "value")]
pub enum RawDrsValue {
    Dword(u32),
    Binary(Vec<u8>),
    Unicode(Vec<u16>),
    Unknown { setting_type: u32, bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "presence")]
pub enum LocalSettingState {
    Absent,
    Present { value: RawDrsValue },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DrsSettingLocation {
    CurrentApplication,
    CurrentGlobal,
    BaseProfile,
    DriverDefault,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub enum SettingPatch {
    Keep,
    Set { value: RawDrsValue },
    RemoveLocal,
}
