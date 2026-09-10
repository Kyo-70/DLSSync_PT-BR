use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::system_info::GpuVendor;
use dlssync_contracts::{DlssCapability, DlssGeneration, NvidiaGpuArchitecture};
use nvapi_drs::settings::RESETTABLE_IDS;
use nvapi_drs::{DlssOverrideConfig, DrsSetting, OverrideScope};
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
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct DlssApplyOutcome {
    pub needs_elevation: bool,
    pub denied_settings: Vec<u32>,
}

#[cfg(windows)]
fn run_apply(scope: &OverrideScope, settings: &[DrsSetting]) -> Result<Vec<u32>, String> {
    nvapi_drs::ffi::apply_overrides(scope, settings)
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
fn run_apply(_scope: &OverrideScope, _settings: &[DrsSetting]) -> Result<Vec<u32>, String> {
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

fn nvidia_architecture(device_id: u16, model: &str) -> NvidiaGpuArchitecture {
    let model = model.to_ascii_lowercase();
    if model.contains("rtx 20") || matches!(device_id, 0x1E00..=0x1FFF | 0x2180..=0x21FF) {
        NvidiaGpuArchitecture::Turing
    } else if model.contains("rtx 30") || matches!(device_id, 0x2200..=0x25FF) {
        NvidiaGpuArchitecture::Ampere
    } else if model.contains("rtx 40") || matches!(device_id, 0x2600..=0x28FF) {
        NvidiaGpuArchitecture::Ada
    } else if model.contains("rtx 50") || matches!(device_id, 0x2B00..=0x2FFF) {
        NvidiaGpuArchitecture::Blackwell
    } else if model.contains("rtx") {
        NvidiaGpuArchitecture::Future
    } else if !model.is_empty() {
        NvidiaGpuArchitecture::PreRtx
    } else {
        NvidiaGpuArchitecture::Unknown
    }
}

fn dlss_capability_for_gpu(
    architecture: NvidiaGpuArchitecture,
    installed_driver: Option<String>,
) -> DlssCapability {
    let has_rtx = !matches!(
        architecture,
        NvidiaGpuArchitecture::PreRtx | NvidiaGpuArchitecture::Unknown
    );
    let has_frame_generation = matches!(
        architecture,
        NvidiaGpuArchitecture::Ada
            | NvidiaGpuArchitecture::Blackwell
            | NvidiaGpuArchitecture::Future
    );
    let has_multi_frame_generation = matches!(
        architecture,
        NvidiaGpuArchitecture::Blackwell | NvidiaGpuArchitecture::Future
    );
    let dlss5_driver = "610.47";
    let neural_rendering = matches!(architecture, NvidiaGpuArchitecture::Blackwell)
        && installed_driver
            .as_deref()
            .is_some_and(|driver| driver_version_at_least(driver, dlss5_driver));
    let mut generations = if has_rtx {
        vec![DlssGeneration::Dlss2, DlssGeneration::Dlss4]
    } else {
        Vec::new()
    };
    if has_frame_generation {
        generations.push(DlssGeneration::Dlss3);
    }
    if neural_rendering {
        generations.push(DlssGeneration::Dlss5);
    }
    DlssCapability {
        gpu_architecture: architecture,
        generations,
        valid_presets: if has_rtx {
            ('A'..='O').map(|letter| letter.to_string()).collect()
        } else {
            Vec::new()
        },
        frame_generation_multipliers: if has_multi_frame_generation {
            vec![2, 3, 4]
        } else if has_frame_generation {
            vec![2]
        } else {
            Vec::new()
        },
        installed_driver,
        minimum_driver: if neural_rendering {
            dlss5_driver.into()
        } else {
            "512.15".into()
        },
        ray_reconstruction: has_rtx,
        neural_rendering,
    }
}

fn driver_version_at_least(installed: &str, minimum: &str) -> bool {
    let parse = |raw: &str| -> Vec<u32> {
        raw.split(|ch: char| !ch.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect()
    };
    let mut installed = parse(installed);
    // WMI reports NVIDIA 591.74 as 32.0.15.9174; normalize that Windows package form.
    if installed.len() >= 4 && installed[0] >= 30 && installed[2] >= 10 {
        installed = vec![
            (installed[2] - 10) * 100 + installed[3] / 100,
            installed[3] % 100,
        ];
    }
    let minimum = parse(minimum);
    installed.as_slice() >= minimum.as_slice()
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn dlss_overrides_supported(state: State<'_, AppState>) -> AppResult<bool> {
    let info = crate::commands::drivers::ensure_system_info(&state).await?;
    Ok(info.gpus.iter().any(|gpu| gpu.vendor == GpuVendor::Nvidia))
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn dlss_capabilities(state: State<'_, AppState>) -> AppResult<Vec<DlssCapability>> {
    let info = crate::commands::drivers::ensure_system_info(&state).await?;
    Ok(info
        .gpus
        .iter()
        .filter(|gpu| gpu.vendor == GpuVendor::Nvidia)
        .map(|gpu| {
            dlss_capability_for_gpu(
                nvidia_architecture(gpu.pci_device_id, &gpu.model),
                (gpu.driver_version != "Unknown").then(|| gpu.driver_version.clone()),
            )
        })
        .collect())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_dlss_override(
    scope: OverrideScope,
    config: DlssOverrideConfig,
) -> AppResult<DlssApplyOutcome> {
    let settings = config.to_drs_settings();
    let denied = tokio::task::spawn_blocking(move || run_apply(&scope, &settings))
        .await
        .map_err(|e| AppError::Other(format!("dlss apply task: {e}")))?
        .map_err(AppError::Other)?;
    Ok(DlssApplyOutcome {
        needs_elevation: !denied.is_empty(),
        denied_settings: denied,
    })
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn reset_dlss_override(scope: OverrideScope) -> AppResult<()> {
    tokio::task::spawn_blocking(move || run_reset(&scope, RESETTABLE_IDS))
        .await
        .map_err(|e| AppError::Other(format!("dlss reset task: {e}")))?
        .map_err(AppError::Other)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn read_dlss_override_config(scope: OverrideScope) -> AppResult<DlssOverrideReadback> {
    let ids = RESETTABLE_IDS.to_vec();
    tokio::task::spawn_blocking(move || -> Result<DlssOverrideReadback, String> {
        let config = DlssOverrideConfig::from_drs_settings(&run_read(&scope, &ids)?);
        if matches!(scope, OverrideScope::PerGame { .. }) && config.is_empty() {
            let inherited =
                DlssOverrideConfig::from_drs_settings(&run_read(&OverrideScope::Global, &ids)?);
            if !inherited.is_empty() {
                return Ok(DlssOverrideReadback {
                    active_count: inherited.active_override_count() as u32,
                    config: inherited,
                    source: DlssOverrideSource::Global,
                });
            }
        }
        let source = match (&scope, config.is_empty()) {
            (_, true) => DlssOverrideSource::None,
            (OverrideScope::Global, false) => DlssOverrideSource::Global,
            (OverrideScope::PerGame { .. }, false) => DlssOverrideSource::PerGame,
        };
        Ok(DlssOverrideReadback {
            active_count: config.active_override_count() as u32,
            config,
            source,
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
    fn capability_models_dlss5_and_blackwell_mfg_explicitly() {
        let capability =
            dlss_capability_for_gpu(NvidiaGpuArchitecture::Blackwell, Some("610.47".into()));
        assert!(capability.generations.contains(&DlssGeneration::Dlss5));
        assert_eq!(capability.frame_generation_multipliers, vec![2, 3, 4]);
        assert!(capability.neural_rendering);
        assert_eq!(
            capability.valid_presets.first().map(String::as_str),
            Some("A")
        );
        assert_eq!(
            capability.valid_presets.last().map(String::as_str),
            Some("O")
        );
    }

    #[test]
    fn older_architecture_and_driver_are_conservatively_gated() {
        let ada = dlss_capability_for_gpu(NvidiaGpuArchitecture::Ada, Some("616.92".into()));
        assert!(!ada.neural_rendering);
        assert!(!ada.generations.contains(&DlssGeneration::Dlss5));
        let turing =
            dlss_capability_for_gpu(NvidiaGpuArchitecture::Turing, Some("32.0.15.9174".into()));
        assert!(!turing.generations.contains(&DlssGeneration::Dlss5));
        assert!(turing.frame_generation_multipliers.is_empty());
        assert!(turing.ray_reconstruction);
        assert!(driver_version_at_least("32.0.15.9174", "591.74"));
        assert!(!driver_version_at_least("591.74", "610.47"));
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
