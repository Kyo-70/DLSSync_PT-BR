pub mod capabilities;
pub mod evidence;
pub mod presets;
pub mod settings;

#[cfg(windows)]
pub mod ffi;

pub use settings::{
    DlssOverrideConfig, DlssPreset, DrsSetting, FeatureSettingIds, FrameGenCount, FrameGenMode,
    OverrideScope, TypedDlssOverrideConfig, FEATURE_SETTING_IDS,
};

pub use capabilities::{
    assess_override_write, AdapterClass, AdapterObservation, AdapterProvider, AmdDriverVersion,
    CapabilityAssessment, CapabilityBlockReason, CapabilityEvidence, HostPlatform,
    IntelDriverVersion, NvapiRuntime, NvidiaDriverVersion, SupportState,
};
pub use evidence::{
    DrsReadbackState, DrsSettingLocation, DrsSettingObservation, DrsWriteState,
    InGameBehaviorState, LocalSettingState, PresetEvidence, ProviderDocumentationState,
    RawDrsValue, SettingPatch,
};
pub use presets::{
    DlssFeature, FgPreset, NrPreset, PresetValueError, RrPreset, SrPreset, FG_PRESET_DEFAULT,
    PRESET_LATEST,
};
