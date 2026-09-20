use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::system_info::{GpuInfo, GpuVendor, SystemInfo};
use dlssync_contracts as ipc;
use nvapi_drs::settings::RESETTABLE_IDS;
use nvapi_drs::{
    self as drs, DlssOverrideConfig, DrsSetting, OverrideScope, TypedDlssOverrideConfig,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

const EXE_WALK_MAX_DEPTH: usize = 4;
const EXE_SKIP_TOKENS: &[&str] = &[
    "redist",
    "vcredist",
    "directx",
    "crashpad",
    "crashreport",
    "unins",
    "setup",
    "dotnet",
    "commonredist",
    "easyanticheat",
    "battleye",
    "launcher",
    "touchup",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DlssOverrideSource {
    PerGame,
    Global,
    None,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DlssOverrideReadback {
    pub config: DlssOverrideConfig,
    pub source: DlssOverrideSource,
    pub active_count: u32,
    pub observations: Vec<ipc::DrsSettingObservation>,
    pub observation_complete: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DlssApplyOutcome {
    pub needs_elevation: bool,
    pub denied_settings: Vec<u32>,
    pub patches: Vec<ipc::SettingPatch>,
    pub preset_evidence: ipc::PresetEvidence,
}

fn project_fg_mode(mode: ipc::DlssFrameGenMode) -> drs::FrameGenMode {
    match mode {
        ipc::DlssFrameGenMode::AppControlled => drs::FrameGenMode::AppControlled,
        ipc::DlssFrameGenMode::Fixed => drs::FrameGenMode::Fixed,
        ipc::DlssFrameGenMode::Dynamic => drs::FrameGenMode::Dynamic,
    }
}

fn project_fg_count(count: ipc::DlssFrameGenCount) -> drs::FrameGenCount {
    match count {
        ipc::DlssFrameGenCount::AppControlled => drs::FrameGenCount::AppControlled,
        ipc::DlssFrameGenCount::X2 => drs::FrameGenCount::X2,
        ipc::DlssFrameGenCount::X3 => drs::FrameGenCount::X3,
        ipc::DlssFrameGenCount::X4 => drs::FrameGenCount::X4,
    }
}

fn typed_write_config(
    config: ipc::DlssPresetWriteConfig,
) -> Result<TypedDlssOverrideConfig, AppError> {
    Ok(TypedDlssOverrideConfig {
        enable_sr_dll_override: config.enable_sr_dll_override,
        sr_preset: config
            .sr_preset
            .map(|preset| drs::SrPreset::try_from(preset.raw_value()))
            .transpose()
            .map_err(|error| AppError::Validation(error.to_string()))?,
        enable_rr_dll_override: config.enable_rr_dll_override,
        rr_preset: config
            .rr_preset
            .map(|preset| drs::RrPreset::try_from(preset.raw_value()))
            .transpose()
            .map_err(|error| AppError::Validation(error.to_string()))?,
        enable_fg_dll_override: config.enable_fg_dll_override,
        fg_preset: config
            .fg_preset
            .map(|preset| drs::FgPreset::try_from(preset.raw_value()))
            .transpose()
            .map_err(|error| AppError::Validation(error.to_string()))?,
        fg_mode: config.fg_mode.map(project_fg_mode),
        fg_fixed_count: config.fg_fixed_count.map(project_fg_count),
        fg_dynamic_target_fps: config.fg_dynamic_target_fps,
    })
}

#[cfg(windows)]
fn run_apply(
    scope: &OverrideScope,
    settings: &[DrsSetting],
    remove: &[u32],
) -> Result<Vec<u32>, String> {
    nvapi_drs::ffi::apply_profile_patch(scope, settings, remove)
}

#[cfg(windows)]
fn run_reset(scope: &OverrideScope, ids: &[u32]) -> Result<(), String> {
    nvapi_drs::ffi::reset_overrides(scope, ids)
}

#[cfg(windows)]
fn run_read(scope: &OverrideScope, ids: &[u32]) -> Result<Vec<(u32, Option<u32>)>, String> {
    nvapi_drs::ffi::read_overrides(scope, ids)
}

#[cfg(not(windows))]
fn run_apply(
    _scope: &OverrideScope,
    _settings: &[DrsSetting],
    _remove: &[u32],
) -> Result<Vec<u32>, String> {
    Err("DLSS overrides require Windows with an NVIDIA driver".to_string())
}

#[cfg(not(windows))]
fn run_reset(_scope: &OverrideScope, _ids: &[u32]) -> Result<(), String> {
    Err("DLSS overrides require Windows with an NVIDIA driver".to_string())
}

#[cfg(not(windows))]
fn run_read(_scope: &OverrideScope, _ids: &[u32]) -> Result<Vec<(u32, Option<u32>)>, String> {
    Err("DLSS overrides require Windows with an NVIDIA driver".to_string())
}

#[cfg(windows)]
fn read_profile_observations(
    scope: &OverrideScope,
    ids: &[u32],
) -> Result<Vec<ipc::DrsSettingObservation>, String> {
    let observations = drs::ffi::read_observations(scope, ids)?;
    let wire = serde_json::to_value(observations).map_err(|error| error.to_string())?;
    serde_json::from_value(wire).map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn read_profile_observations(
    _scope: &OverrideScope,
    _ids: &[u32],
) -> Result<Vec<ipc::DrsSettingObservation>, String> {
    Err("NVIDIA profile observations require Windows".into())
}

fn parse_nvidia_driver_version(raw: &str) -> Option<drs::NvidiaDriverVersion> {
    if raw.eq_ignore_ascii_case("unknown") {
        return None;
    }
    let parts: Vec<u32> = raw
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect();
    let (branch, revision) = if parts.len() >= 4 && parts[0] >= 30 && parts[2] >= 10 {
        ((parts[2] - 10) * 100 + parts[3] / 100, parts[3] % 100)
    } else {
        (*parts.first()?, *parts.get(1)?)
    };
    Some(drs::NvidiaDriverVersion {
        branch: u16::try_from(branch).ok()?,
        revision: u16::try_from(revision).ok()?,
    })
}

fn observed_driver_text(raw: &str) -> Option<String> {
    (!raw.trim().is_empty() && !raw.eq_ignore_ascii_case("unknown")).then(|| raw.to_string())
}

fn adapter_observation(gpu: &GpuInfo) -> drs::AdapterObservation {
    let provider = match gpu.vendor {
        GpuVendor::Nvidia => drs::AdapterProvider::Nvidia {
            driver_version: parse_nvidia_driver_version(&gpu.driver_version),
        },
        GpuVendor::Amd => drs::AdapterProvider::Amd {
            driver_version: observed_driver_text(&gpu.driver_version)
                .map(|vendor_text| drs::AmdDriverVersion { vendor_text }),
        },
        GpuVendor::Intel => drs::AdapterProvider::Intel {
            driver_version: observed_driver_text(&gpu.driver_version)
                .map(|vendor_text| drs::IntelDriverVersion { vendor_text }),
        },
        GpuVendor::Other => drs::AdapterProvider::Unknown {
            detail: format!("unrecognized PCI vendor 0x{:04X}", gpu.pci_vendor_id),
        },
    };
    drs::AdapterObservation {
        class: drs::AdapterClass::Unknown,
        provider,
        hardware_recognized: gpu.identifiable && gpu.pci_vendor_id != 0 && gpu.pci_device_id != 0,
    }
}

#[cfg(windows)]
fn host_platform() -> drs::HostPlatform {
    drs::HostPlatform::Windows
}

#[cfg(not(windows))]
fn host_platform() -> drs::HostPlatform {
    drs::HostPlatform::Other {
        name: std::env::consts::OS.to_string(),
    }
}

#[cfg(windows)]
fn profile_access() -> Result<ipc::DrsProfileAccess, String> {
    let runtime = drs::ffi::profile_runtime()?;
    Ok(ipc::DrsProfileAccess {
        interface_version: runtime.interface_version,
        driver_version: runtime.driver_version,
        driver_branch: runtime.driver_branch,
        setting_ids: RESETTABLE_IDS
            .iter()
            .copied()
            .filter(|id| runtime.supported_setting_ids.contains(id))
            .collect(),
    })
}

#[cfg(not(windows))]
fn profile_access() -> Result<ipc::DrsProfileAccess, String> {
    Err("NVAPI DRS requires Windows".into())
}

fn capability_evidence(
    gpu: &GpuInfo,
    host: &drs::HostPlatform,
    nvapi: &drs::NvapiRuntime,
) -> drs::CapabilityEvidence {
    drs::CapabilityEvidence {
        host: host.clone(),
        nvapi: nvapi.clone(),
        adapter: adapter_observation(gpu),
        provider_applicability: drs::SupportState::Unknown,
        game_integration: drs::SupportState::Unknown,
        runtime_stack: drs::SupportState::Unknown,
        preset_runtime_mapping: drs::SupportState::Unknown,
    }
}

fn project_host(value: &drs::HostPlatform) -> ipc::HostPlatform {
    match value {
        drs::HostPlatform::Windows => ipc::HostPlatform::Windows,
        drs::HostPlatform::Other { name } => ipc::HostPlatform::Other { name: name.clone() },
    }
}

fn project_nvapi(value: &drs::NvapiRuntime) -> ipc::NvapiRuntime {
    match value {
        drs::NvapiRuntime::Available { version } => ipc::NvapiRuntime::Available {
            version: version.clone(),
        },
        drs::NvapiRuntime::Unavailable { reason } => ipc::NvapiRuntime::Unavailable {
            reason: reason.clone(),
        },
        drs::NvapiRuntime::VersionUnknown { detail } => ipc::NvapiRuntime::VersionUnknown {
            detail: detail.clone(),
        },
    }
}

fn project_adapter(value: &drs::AdapterObservation) -> ipc::AdapterObservation {
    let class = match value.class {
        drs::AdapterClass::Dedicated => ipc::AdapterClass::Dedicated,
        drs::AdapterClass::Integrated => ipc::AdapterClass::Integrated,
        drs::AdapterClass::Unknown => ipc::AdapterClass::Unknown,
    };
    let provider = match &value.provider {
        drs::AdapterProvider::Nvidia { driver_version } => ipc::AdapterProvider::Nvidia {
            driver_version: driver_version.map(|version| ipc::NvidiaDriverVersion {
                branch: version.branch,
                revision: version.revision,
            }),
        },
        drs::AdapterProvider::Amd { driver_version } => ipc::AdapterProvider::Amd {
            driver_version: driver_version
                .as_ref()
                .map(|version| ipc::AmdDriverVersion {
                    vendor_text: version.vendor_text.clone(),
                }),
        },
        drs::AdapterProvider::Intel { driver_version } => ipc::AdapterProvider::Intel {
            driver_version: driver_version
                .as_ref()
                .map(|version| ipc::IntelDriverVersion {
                    vendor_text: version.vendor_text.clone(),
                }),
        },
        drs::AdapterProvider::Unknown { detail } => ipc::AdapterProvider::Unknown {
            detail: detail.clone(),
        },
    };
    ipc::AdapterObservation {
        class,
        provider,
        hardware_recognized: value.hardware_recognized,
    }
}

fn project_support(value: drs::SupportState) -> ipc::SupportState {
    match value {
        drs::SupportState::Confirmed => ipc::SupportState::Confirmed,
        drs::SupportState::Incompatible => ipc::SupportState::Incompatible,
        drs::SupportState::Unknown => ipc::SupportState::Unknown,
    }
}

fn project_evidence(value: &drs::CapabilityEvidence) -> ipc::CapabilityEvidence {
    ipc::CapabilityEvidence {
        host: project_host(&value.host),
        nvapi: project_nvapi(&value.nvapi),
        adapter: project_adapter(&value.adapter),
        provider_applicability: project_support(value.provider_applicability),
        game_integration: project_support(value.game_integration),
        runtime_stack: project_support(value.runtime_stack),
        preset_runtime_mapping: project_support(value.preset_runtime_mapping),
    }
}

fn project_block_reason(value: drs::CapabilityBlockReason) -> ipc::CapabilityBlockReason {
    match value {
        drs::CapabilityBlockReason::HostNotWindows => ipc::CapabilityBlockReason::HostNotWindows,
        drs::CapabilityBlockReason::NvapiUnavailable => {
            ipc::CapabilityBlockReason::NvapiUnavailable
        }
        drs::CapabilityBlockReason::NvapiVersionUnknown => {
            ipc::CapabilityBlockReason::NvapiVersionUnknown
        }
        drs::CapabilityBlockReason::AdapterNotNvidia => {
            ipc::CapabilityBlockReason::AdapterNotNvidia
        }
        drs::CapabilityBlockReason::NvidiaDriverVersionUnknown => {
            ipc::CapabilityBlockReason::NvidiaDriverVersionUnknown
        }
        drs::CapabilityBlockReason::AdapterClassUnknown => {
            ipc::CapabilityBlockReason::AdapterClassUnknown
        }
        drs::CapabilityBlockReason::HardwareUnrecognized => {
            ipc::CapabilityBlockReason::HardwareUnrecognized
        }
        drs::CapabilityBlockReason::ProviderApplicabilityUnknown => {
            ipc::CapabilityBlockReason::ProviderApplicabilityUnknown
        }
        drs::CapabilityBlockReason::ProviderIncompatible => {
            ipc::CapabilityBlockReason::ProviderIncompatible
        }
        drs::CapabilityBlockReason::GameIntegrationUnknown => {
            ipc::CapabilityBlockReason::GameIntegrationUnknown
        }
        drs::CapabilityBlockReason::GameIntegrationIncompatible => {
            ipc::CapabilityBlockReason::GameIntegrationIncompatible
        }
        drs::CapabilityBlockReason::RuntimeStackUnknown => {
            ipc::CapabilityBlockReason::RuntimeStackUnknown
        }
        drs::CapabilityBlockReason::RuntimeStackIncompatible => {
            ipc::CapabilityBlockReason::RuntimeStackIncompatible
        }
        drs::CapabilityBlockReason::PresetRuntimeMappingUnknown => {
            ipc::CapabilityBlockReason::PresetRuntimeMappingUnknown
        }
        drs::CapabilityBlockReason::PresetRuntimeMappingIncompatible => {
            ipc::CapabilityBlockReason::PresetRuntimeMappingIncompatible
        }
    }
}

fn project_assessment(value: drs::CapabilityAssessment) -> ipc::CapabilityAssessment {
    ipc::CapabilityAssessment {
        provider_documented_namespace: value.provider_documented_namespace,
        write_eligible: value.write_eligible,
        block_reasons: value
            .block_reasons
            .into_iter()
            .map(project_block_reason)
            .collect(),
    }
}

fn preset_registry() -> ipc::DlssPresetRegistry {
    let sr = (0..=15)
        .chain(std::iter::once(drs::PRESET_LATEST))
        .map(|raw| {
            let source = drs::SrPreset::try_from(raw).expect("SR registry value");
            ipc::SrPresetDefinition {
                preset: ipc::SrPreset::try_from(raw).expect("SR IPC value"),
                raw_value: raw,
                description: source.description().into(),
                write_mapping: ipc::PresetWriteMapping::Writable,
            }
        })
        .collect();
    let rr = (0..=15)
        .chain(std::iter::once(drs::PRESET_LATEST))
        .map(|raw| {
            let source = drs::RrPreset::try_from(raw).expect("RR registry value");
            ipc::RrPresetDefinition {
                preset: ipc::RrPreset::try_from(raw).expect("RR IPC value"),
                raw_value: raw,
                description: source.description().into(),
                write_mapping: ipc::PresetWriteMapping::Writable,
            }
        })
        .collect();
    let fg = (0..=26)
        .chain([drs::FG_PRESET_DEFAULT, drs::PRESET_LATEST])
        .map(|raw| {
            let source = drs::FgPreset::try_from(raw).expect("FG registry value");
            ipc::FgPresetDefinition {
                preset: ipc::FgPreset::try_from(raw).expect("FG IPC value"),
                raw_value: raw,
                description: source.description().into(),
                write_mapping: ipc::PresetWriteMapping::Writable,
            }
        })
        .collect();
    let nr = (0..=4)
        .chain(std::iter::once(drs::PRESET_LATEST))
        .map(|raw| {
            let source = drs::NrPreset::try_from(raw).expect("NR registry value");
            ipc::NrPresetDefinition {
                preset: ipc::NrPreset::try_from(raw).expect("NR IPC value"),
                raw_value: raw,
                description: source.description().into(),
                write_mapping: ipc::PresetWriteMapping::ReadOnly {
                    reason: ipc::PresetReadOnlyReason::ProviderRuntimeWriteSupportUnverified,
                },
            }
        })
        .collect();
    ipc::DlssPresetRegistry {
        sr: ipc::SrPresetRegistry {
            setting_ids: ipc::PresetNamespaceIds {
                override_id: drs::settings::ids::DLSS_SR_ENABLE_OVERRIDE,
                preset_id: drs::settings::ids::DLSS_SR_FORCED_PRESET,
            },
            options: sr,
        },
        rr: ipc::RrPresetRegistry {
            setting_ids: ipc::PresetNamespaceIds {
                override_id: drs::settings::ids::DLSS_RR_ENABLE_OVERRIDE,
                preset_id: drs::settings::ids::DLSS_RR_FORCED_PRESET,
            },
            options: rr,
        },
        fg: ipc::FgPresetRegistry {
            setting_ids: ipc::PresetNamespaceIds {
                override_id: drs::settings::ids::DLSS_FG_ENABLE_OVERRIDE,
                preset_id: drs::settings::ids::DLSS_FG_FORCED_PRESET,
            },
            options: fg,
        },
        nr: ipc::NrPresetRegistry {
            setting_ids: ipc::PresetNamespaceIds {
                override_id: drs::settings::ids::DLSS_NR_ENABLE_OVERRIDE,
                preset_id: drs::settings::ids::DLSS_NR_FORCED_PRESET,
            },
            options: nr,
        },
    }
}

fn capability_snapshot_from(
    info: &SystemInfo,
    host: drs::HostPlatform,
    nvapi: drs::NvapiRuntime,
) -> ipc::DlssCapabilitySnapshot {
    let adapters = info
        .gpus
        .iter()
        .map(|gpu| {
            let evidence = capability_evidence(gpu, &host, &nvapi);
            let assessment = drs::assess_override_write(&evidence);
            ipc::DlssCapabilityReport {
                adapter_model: gpu.model.clone(),
                pci_vendor_id: gpu.pci_vendor_id,
                pci_device_id: gpu.pci_device_id,
                evidence: project_evidence(&evidence),
                assessment: project_assessment(assessment),
            }
        })
        .collect();
    ipc::DlssCapabilitySnapshot {
        presets: preset_registry(),
        adapters,
        profile_access: None,
    }
}

async fn capability_snapshot(
    state: &State<'_, AppState>,
) -> AppResult<ipc::DlssCapabilitySnapshot> {
    let info = crate::commands::drivers::ensure_system_info(state).await?;
    let access = tokio::task::spawn_blocking(profile_access)
        .await
        .map_err(|error| AppError::Other(format!("NVAPI capability probe: {error}")))?;
    let runtime = match &access {
        Ok(value) => drs::NvapiRuntime::Available {
            version: value.interface_version.clone(),
        },
        Err(reason) => drs::NvapiRuntime::Unavailable {
            reason: reason.clone(),
        },
    };
    let mut snapshot = capability_snapshot_from(&info, host_platform(), runtime);
    if info
        .gpus
        .iter()
        .any(|gpu| gpu.vendor == GpuVendor::Nvidia && gpu.identifiable)
    {
        snapshot.profile_access = access.ok();
    }
    Ok(snapshot)
}

async fn ensure_write_eligible(state: &State<'_, AppState>, requested: &[u32]) -> AppResult<()> {
    let snapshot = capability_snapshot(state).await?;
    let access = snapshot.profile_access.ok_or_else(|| {
        AppError::Validation("The installed NVIDIA driver profile API is unavailable".into())
    })?;
    if let Some(id) = requested.iter().find(|id| !access.setting_ids.contains(id)) {
        return Err(AppError::Validation(format!(
            "The installed driver does not expose profile setting 0x{id:08X}"
        )));
    }
    Ok(())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn dlss_overrides_supported(state: State<'_, AppState>) -> AppResult<bool> {
    Ok(capability_snapshot(&state)
        .await?
        .profile_access
        .is_some_and(|access| !access.setting_ids.is_empty()))
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn dlss_capabilities(
    state: State<'_, AppState>,
) -> AppResult<ipc::DlssCapabilitySnapshot> {
    capability_snapshot(&state).await
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_dlss_override(
    state: State<'_, AppState>,
    scope: OverrideScope,
    config: ipc::DlssPresetWriteConfig,
    changed_setting_ids: Option<Vec<u32>>,
) -> AppResult<DlssApplyOutcome> {
    let mut settings = typed_write_config(config)?.to_drs_settings();
    let requested =
        changed_setting_ids.unwrap_or_else(|| settings.iter().map(|setting| setting.id).collect());
    if requested.iter().any(|id| !RESETTABLE_IDS.contains(id)) {
        return Err(AppError::Validation("Unknown DLSS profile setting".into()));
    }
    ensure_write_eligible(&state, &requested).await?;
    settings.retain(|setting| requested.contains(&setting.id));
    for id in [
        drs::settings::ids::DLSS_SR_ENABLE_OVERRIDE,
        drs::settings::ids::DLSS_RR_ENABLE_OVERRIDE,
        drs::settings::ids::DLSS_FG_ENABLE_OVERRIDE,
    ] {
        if requested.contains(&id) && !settings.iter().any(|setting| setting.id == id) {
            settings.push(DrsSetting { id, value: 0 });
        }
    }
    let remove: Vec<_> = requested
        .iter()
        .copied()
        .filter(|id| !settings.iter().any(|setting| setting.id == *id))
        .collect();
    let patches = settings
        .iter()
        .map(|setting| ipc::SettingPatch::Set {
            value: ipc::RawDrsValue::Dword(setting.value),
        })
        .chain(remove.iter().map(|_| ipc::SettingPatch::RemoveLocal))
        .collect();
    let intended = settings.clone();
    let removed = remove.clone();
    let (denied, readback, removed_ok) = tokio::task::spawn_blocking(move || {
        let denied = run_apply(&scope, &settings, &remove)?;
        let ids = settings
            .iter()
            .map(|setting| setting.id)
            .collect::<Vec<_>>();
        let readback = run_read(&scope, &ids);
        let removed_ok = read_profile_observations(&scope, &remove).map(|values| {
            values
                .iter()
                .all(|value| matches!(value.local, ipc::LocalSettingState::Absent))
        });
        Ok::<_, String>((denied, readback, removed_ok))
    })
    .await
    .map_err(|e| AppError::Other(format!("dlss apply task: {e}")))?
    .map_err(AppError::Other)?;
    let readback_state = match readback {
        Ok(values) => {
            let matched = denied.is_empty()
                && intended.iter().all(|setting| {
                    values
                        .iter()
                        .any(|(id, value)| *id == setting.id && *value == Some(setting.value))
                });
            if matched && removed_ok.unwrap_or(false) {
                ipc::DrsReadbackState::Matched
            } else {
                ipc::DrsReadbackState::Mismatched
            }
        }
        Err(_) => ipc::DrsReadbackState::Failed,
    };
    let write_state = if intended.is_empty() && removed.is_empty() {
        ipc::DrsWriteState::NotAttempted
    } else if denied.is_empty() {
        ipc::DrsWriteState::Accepted
    } else {
        ipc::DrsWriteState::Rejected
    };
    Ok(DlssApplyOutcome {
        needs_elevation: !denied.is_empty(),
        denied_settings: denied,
        patches,
        preset_evidence: ipc::PresetEvidence {
            write: write_state,
            readback: readback_state,
            provider_documentation: ipc::ProviderDocumentationState::DocumentedNumericDefinition,
            in_game_behavior: ipc::InGameBehaviorState::Unknown,
        },
    })
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn reset_dlss_override(
    state: State<'_, AppState>,
    scope: OverrideScope,
) -> AppResult<()> {
    let snapshot = capability_snapshot(&state).await?;
    let ids = snapshot
        .profile_access
        .ok_or_else(|| AppError::Validation("NVIDIA profile access unavailable".into()))?
        .setting_ids;
    tokio::task::spawn_blocking(move || {
        run_reset(&scope, &ids)?;
        let readback = read_profile_observations(&scope, &ids)?;
        if readback
            .iter()
            .any(|value| !matches!(value.local, ipc::LocalSettingState::Absent))
        {
            return Err("NVIDIA profile reset did not match readback".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(format!("dlss reset task: {e}")))?
    .map_err(AppError::Other)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn read_dlss_override_config(scope: OverrideScope) -> AppResult<DlssOverrideReadback> {
    let ids = RESETTABLE_IDS.to_vec();
    tokio::task::spawn_blocking(move || -> Result<DlssOverrideReadback, String> {
        let observations = read_profile_observations(&scope, &ids)?;
        let values: Vec<_> = observations
            .iter()
            .map(|item| {
                (
                    item.setting_id,
                    match item.effective_value {
                        Some(ipc::RawDrsValue::Dword(value)) => Some(value),
                        _ => None,
                    },
                )
            })
            .collect();
        let config = DlssOverrideConfig::from_drs_settings(&values);
        let has_local = observations
            .iter()
            .any(|item| matches!(item.local, ipc::LocalSettingState::Present { .. }));
        let source = if config.is_empty() {
            DlssOverrideSource::None
        } else if matches!(scope, OverrideScope::Global) || !has_local {
            DlssOverrideSource::Global
        } else {
            DlssOverrideSource::PerGame
        };
        Ok(DlssOverrideReadback {
            active_count: config.active_override_count() as u32,
            config,
            source,
            observations,
            observation_complete: true,
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("dlss read config task: {e}")))?
    .map_err(AppError::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase5_ipc_registry_keeps_feature_specific_ranges() {
        let registry = preset_registry();
        assert_eq!(registry.sr.options.len(), 17);
        assert_eq!(registry.rr.options.len(), 17);
        assert_eq!(registry.fg.options.len(), 29);
        assert_eq!(registry.nr.options.len(), 6);
        assert!(registry
            .fg
            .options
            .iter()
            .any(|option| option.preset == ipc::FgPreset::Z));
        assert!(!registry
            .nr
            .options
            .iter()
            .any(|option| option.raw_value == 5));
    }

    #[test]
    fn phase5_ipc_fg_default_remains_distinct_from_latest() {
        let registry = preset_registry();
        let default = registry
            .fg
            .options
            .iter()
            .find(|option| option.raw_value == drs::FG_PRESET_DEFAULT)
            .unwrap();
        let latest = registry
            .fg
            .options
            .iter()
            .find(|option| option.raw_value == drs::PRESET_LATEST)
            .unwrap();
        assert_eq!(default.preset, ipc::FgPreset::Default);
        assert_eq!(latest.preset, ipc::FgPreset::Latest);
        assert_ne!(default.preset, latest.preset);
        assert!(!registry
            .sr
            .options
            .iter()
            .any(|option| option.raw_value == drs::FG_PRESET_DEFAULT));
    }

    #[test]
    fn phase5_ipc_capabilities_fail_closed_without_nvapi_or_known_version() {
        let gpu = test_gpu(GpuVendor::Nvidia, "610.47");
        for nvapi in [
            drs::NvapiRuntime::Unavailable {
                reason: "not installed".into(),
            },
            drs::NvapiRuntime::VersionUnknown {
                detail: "version unavailable".into(),
            },
        ] {
            let evidence = capability_evidence(&gpu, &drs::HostPlatform::Windows, &nvapi);
            let assessment = drs::assess_override_write(&evidence);
            assert!(!assessment.write_eligible);
            assert!(assessment.block_reasons.iter().any(|reason| matches!(
                reason,
                drs::CapabilityBlockReason::NvapiUnavailable
                    | drs::CapabilityBlockReason::NvapiVersionUnknown
            )));
        }
    }

    #[test]
    fn phase5_ipc_provider_versions_do_not_share_a_comparison_space() {
        let nvidia = adapter_observation(&test_gpu(GpuVendor::Nvidia, "32.0.15.9174"));
        let amd = adapter_observation(&test_gpu(GpuVendor::Amd, "25.6.1"));
        let intel = adapter_observation(&test_gpu(GpuVendor::Intel, "32.0.101.6734"));

        assert!(matches!(
            nvidia.provider,
            drs::AdapterProvider::Nvidia {
                driver_version: Some(drs::NvidiaDriverVersion {
                    branch: 591,
                    revision: 74
                })
            }
        ));
        assert!(matches!(amd.provider, drs::AdapterProvider::Amd { .. }));
        assert!(matches!(intel.provider, drs::AdapterProvider::Intel { .. }));
        assert_eq!(nvidia.class, drs::AdapterClass::Unknown);
        assert_eq!(amd.class, drs::AdapterClass::Unknown);
        assert_eq!(intel.class, drs::AdapterClass::Unknown);
    }

    fn test_gpu(vendor: GpuVendor, driver_version: &str) -> GpuInfo {
        let pci_vendor_id = match vendor {
            GpuVendor::Nvidia => 0x10DE,
            GpuVendor::Amd => 0x1002,
            GpuVendor::Intel => 0x8086,
            GpuVendor::Other => 0xFFFF,
        };
        GpuInfo {
            vendor,
            pci_vendor_id,
            pci_device_id: 1,
            model: "test adapter".into(),
            driver_version: driver_version.into(),
            vram_bytes: 0,
            recommended_runtimes: Vec::new(),
            is_dch: true,
            identifiable: true,
            fsr4_capable: false,
        }
    }
}

fn collect_executables(dir: &Path, depth: usize, out: &mut Vec<(u64, PathBuf)>) {
    if depth > EXE_WALK_MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if EXE_SKIP_TOKENS.iter().any(|token| name.contains(token)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let path = entry.path();
        if meta.is_dir() {
            collect_executables(&path, depth + 1, out);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            out.push((meta.len(), path));
        }
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn find_game_executable(install_dir: String) -> AppResult<Option<String>> {
    crate::paths::PathGuard::assert_safe_scan_dir(Path::new(&install_dir))
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let resolved = tokio::task::spawn_blocking(move || {
        let mut executables = Vec::new();
        collect_executables(Path::new(&install_dir), 0, &mut executables);
        executables
            .into_iter()
            .max_by_key(|(size, _)| *size)
            .map(|(_, path)| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| AppError::Other(format!("executable scan: {e}")))?;
    Ok(resolved)
}
