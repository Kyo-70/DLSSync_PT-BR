use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "platform")]
pub enum HostPlatform {
    Windows,
    Other { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum NvapiRuntime {
    Available { version: String },
    Unavailable { reason: String },
    VersionUnknown { detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AdapterClass {
    Dedicated,
    Integrated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NvidiaDriverVersion {
    pub branch: u16,
    pub revision: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AmdDriverVersion {
    pub vendor_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct IntelDriverVersion {
    pub vendor_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AdapterObservation {
    pub class: AdapterClass,
    pub provider: AdapterProvider,
    pub hardware_recognized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    Confirmed,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CapabilityEvidence {
    pub host: HostPlatform,
    pub nvapi: NvapiRuntime,
    pub adapter: AdapterObservation,
    pub provider_applicability: SupportState,
    pub game_integration: SupportState,
    pub runtime_stack: SupportState,
    pub preset_runtime_mapping: SupportState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CapabilityAssessment {
    /// The pinned NVIDIA header documents the requested setting namespace.
    /// This field does not establish adapter, game, or runtime support.
    pub provider_documented_namespace: bool,
    pub write_eligible: bool,
    pub block_reasons: Vec<CapabilityBlockReason>,
}

pub fn assess_override_write(evidence: &CapabilityEvidence) -> CapabilityAssessment {
    let mut block_reasons = Vec::new();

    if !matches!(evidence.host, HostPlatform::Windows) {
        block_reasons.push(CapabilityBlockReason::HostNotWindows);
    }

    match &evidence.nvapi {
        NvapiRuntime::Available { version } if !version.trim().is_empty() => {}
        NvapiRuntime::Available { .. } | NvapiRuntime::VersionUnknown { .. } => {
            block_reasons.push(CapabilityBlockReason::NvapiVersionUnknown);
        }
        NvapiRuntime::Unavailable { .. } => {
            block_reasons.push(CapabilityBlockReason::NvapiUnavailable);
        }
    }

    match &evidence.adapter.provider {
        AdapterProvider::Nvidia {
            driver_version: Some(_),
        } => {}
        AdapterProvider::Nvidia {
            driver_version: None,
        } => block_reasons.push(CapabilityBlockReason::NvidiaDriverVersionUnknown),
        AdapterProvider::Amd { .. }
        | AdapterProvider::Intel { .. }
        | AdapterProvider::Unknown { .. } => {
            block_reasons.push(CapabilityBlockReason::AdapterNotNvidia);
        }
    }

    if matches!(evidence.adapter.class, AdapterClass::Unknown) {
        block_reasons.push(CapabilityBlockReason::AdapterClassUnknown);
    }
    if !evidence.adapter.hardware_recognized {
        block_reasons.push(CapabilityBlockReason::HardwareUnrecognized);
    }

    push_support_reason(
        evidence.provider_applicability,
        CapabilityBlockReason::ProviderApplicabilityUnknown,
        CapabilityBlockReason::ProviderIncompatible,
        &mut block_reasons,
    );
    push_support_reason(
        evidence.game_integration,
        CapabilityBlockReason::GameIntegrationUnknown,
        CapabilityBlockReason::GameIntegrationIncompatible,
        &mut block_reasons,
    );
    push_support_reason(
        evidence.runtime_stack,
        CapabilityBlockReason::RuntimeStackUnknown,
        CapabilityBlockReason::RuntimeStackIncompatible,
        &mut block_reasons,
    );
    push_support_reason(
        evidence.preset_runtime_mapping,
        CapabilityBlockReason::PresetRuntimeMappingUnknown,
        CapabilityBlockReason::PresetRuntimeMappingIncompatible,
        &mut block_reasons,
    );

    CapabilityAssessment {
        provider_documented_namespace: true,
        write_eligible: block_reasons.is_empty(),
        block_reasons,
    }
}

fn push_support_reason(
    state: SupportState,
    unknown: CapabilityBlockReason,
    incompatible: CapabilityBlockReason,
    reasons: &mut Vec<CapabilityBlockReason>,
) {
    match state {
        SupportState::Confirmed => {}
        SupportState::Unknown => reasons.push(unknown),
        SupportState::Incompatible => reasons.push(incompatible),
    }
}
