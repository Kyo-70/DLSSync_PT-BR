use crate::error::{AppError, AppResult};
use crate::state::{AppState, DriverInstallCandidate, DriverInstallTarget, DriverRebootEvidence};
use crate::system_info::{self, GpuInfo, GpuVendor, SystemInfo};
use dll_catalog::DownloadProgress;
use driver_catalog::{
    sources::DEFAULT_HISTORY_LIMIT, DeviceClass, DeviceId, DriverRegistry, DriverRelease,
    DriverStatusReport, DriverUpdateAction, DriverVendor, DriverVersion, OsFamily, OsTarget,
    UpdateStatus,
};
use driver_install::state::{classify_exit, describe_exit, reboot_required, InstallStage};
use driver_install::{download_to_file, verify_signature, DownloadOpts};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};

const WINDOWS_11_MIN_BUILD: u32 = 22000;
const DRIVER_REBOOT_STATE_FILE: &str = "driver-reboot-pending.json";
const BOOT_TIME_TOLERANCE_SECS: u64 = 5;

fn map_vendor(vendor: GpuVendor) -> DriverVendor {
    match vendor {
        GpuVendor::Nvidia => DriverVendor::Nvidia,
        GpuVendor::Amd => DriverVendor::Amd,
        GpuVendor::Intel => DriverVendor::Intel,
        GpuVendor::Other => DriverVendor::Other,
    }
}

fn os_family(info: &SystemInfo) -> OsFamily {
    let build = info.os.build.parse::<u32>().unwrap_or(0);
    if build >= WINDOWS_11_MIN_BUILD {
        OsFamily::Windows11X64
    } else {
        OsFamily::Windows10X64
    }
}

/// Build the per-GPU `OsTarget`. The OS family is host-wide, but the DCH-vs-
/// Standard driver model is per-GPU (only NVIDIA distinguishes them) so the
/// flag comes from `gpu.is_dch` — Standard-driver (legacy) NVIDIA users get the
/// Standard download URL instead of always being sent the DCH build.
fn os_target_for(info: &SystemInfo, gpu: &GpuInfo) -> OsTarget {
    OsTarget {
        family: os_family(info),
        dch: gpu.is_dch,
    }
}

fn device_for(gpu: &GpuInfo) -> (DeviceId, DriverVersion) {
    let vendor = map_vendor(gpu.vendor);
    let device = DeviceId {
        class: DeviceClass::Gpu,
        vendor,
        pci_vendor_id: gpu.pci_vendor_id,
        pci_device_id: gpu.pci_device_id,
        model: gpu.model.clone(),
    };
    let installed = DriverVersion::from_installed(vendor, &gpu.driver_version);
    (device, installed)
}

fn vendor_key(vendor: DriverVendor) -> &'static str {
    match vendor {
        DriverVendor::Nvidia => "nvidia",
        DriverVendor::Amd => "amd",
        DriverVendor::Intel => "intel",
        DriverVendor::Other => "other",
        _ => "other",
    }
}

fn device_key(device: &DeviceId) -> String {
    format!(
        "{}:{:04x}:{:04x}:{}",
        vendor_key(device.vendor),
        device.pci_vendor_id,
        device.pci_device_id,
        device.model.trim().to_ascii_lowercase()
    )
}

fn reboot_state_path(state: &AppState) -> AppResult<PathBuf> {
    state
        .paths
        .read()
        .as_ref()
        .map(|paths| paths.settings_dir.join(DRIVER_REBOOT_STATE_FILE))
        .ok_or_else(|| AppError::Other("app paths not initialized".into()))
}

fn load_reboot_state(state: &AppState) -> AppResult<()> {
    let mut loaded = state.driver_reboot_state_loaded.lock();
    if *loaded {
        return Ok(());
    }
    let path = reboot_state_path(state)?;
    let pending = if path.exists() {
        let bytes = std::fs::read(&path)?;
        serde_json::from_slice::<HashMap<String, DriverRebootEvidence>>(&bytes)
            .map_err(|error| AppError::Other(format!("driver reboot state parse: {error}")))?
    } else {
        HashMap::new()
    };
    *state.driver_reboot_pending.write() = pending;
    *loaded = true;
    Ok(())
}

fn persist_reboot_state(state: &AppState) -> AppResult<()> {
    let path = reboot_state_path(state)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(&*state.driver_reboot_pending.read())
        .map_err(|error| AppError::Other(format!("driver reboot state encode: {error}")))?;
    std::fs::write(path, body)?;
    Ok(())
}

fn gpu_for_device<'a>(info: &'a SystemInfo, device: &DeviceId) -> Option<&'a GpuInfo> {
    info.gpus.iter().find(|gpu| {
        map_vendor(gpu.vendor) == device.vendor
            && gpu.pci_vendor_id == device.pci_vendor_id
            && gpu.pci_device_id == device.pci_device_id
            && gpu.model == device.model
    })
}

fn observed_version(info: &SystemInfo, device: &DeviceId) -> Option<DriverVersion> {
    let gpu = gpu_for_device(info, device)?;
    let version = DriverVersion::from_installed(device.vendor, &gpu.driver_version);
    (version.packed != 0).then_some(version)
}

fn boot_changed(recorded: u64, current: u64) -> bool {
    recorded != 0 && current != 0 && recorded.abs_diff(current) > BOOT_TIME_TOLERANCE_SECS
}

fn reconcile_reboot_pending(
    pending: &mut HashMap<String, DriverRebootEvidence>,
    info: &SystemInfo,
    current_boot_time: u64,
) -> bool {
    let before = pending.len();
    pending.retain(|_, evidence| {
        if !boot_changed(evidence.boot_time_secs, current_boot_time) {
            return true;
        }
        let Some(observed) = observed_version(info, &evidence.device) else {
            return true;
        };
        observed.packed != evidence.expected_version.packed
    });
    before != pending.len()
}

const SYSTEM_INFO_CACHE_TTL_SECS: i64 = 60;

pub(crate) async fn ensure_system_info(state: &State<'_, AppState>) -> AppResult<SystemInfo> {
    let stale = state.system_info.read().as_ref().is_some_and(|info| {
        chrono::Utc::now()
            .signed_duration_since(info.collected_at)
            .num_seconds()
            >= SYSTEM_INFO_CACHE_TTL_SECS
    });
    if stale {
        *state.system_info.write() = None;
    }
    crate::state::coordinate_singleton(
        &state.system_info,
        &state.collect_system_info_lock,
        || async {
            tokio::task::spawn_blocking(system_info::collect)
                .await
                .map_err(|e| crate::error::AppError::Other(format!("system_info collect: {e}")))
        },
    )
    .await
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn check_driver_updates(
    state: State<'_, AppState>,
) -> AppResult<Vec<DriverStatusReport>> {
    load_reboot_state(state.inner())?;
    let info = ensure_system_info(&state).await?;
    let current_boot_time = sysinfo::System::boot_time();
    let reboot_state_changed = {
        let mut pending = state.driver_reboot_pending.write();
        reconcile_reboot_pending(&mut pending, &info, current_boot_time)
    };
    if reboot_state_changed {
        persist_reboot_state(state.inner())?;
    }
    let registry = DriverRegistry::with_default_gpu_sources();
    let client = state.http_catalog.clone();
    let mut reports = Vec::with_capacity(info.gpus.len());
    let mut candidates: HashMap<String, DriverInstallCandidate> = HashMap::new();
    for gpu in &info.gpus {
        let os = os_target_for(&info, gpu);
        let (device, installed) = device_for(gpu);
        let reboot_pending = state
            .driver_reboot_pending
            .read()
            .get(&device_key(&device))
            .map(|evidence| evidence.expected_version.display.clone());
        let report = match registry
            .resolve_with_reboot_pending(
                &client,
                &device,
                &os,
                installed.clone(),
                reboot_pending.clone(),
            )
            .await
        {
            Ok(report) => report,
            Err(error) => {
                tracing::warn!(model = %gpu.model, %error, "driver lookup failed");
                DriverStatusReport::new(
                    device,
                    installed,
                    None,
                    UpdateStatus::Unknown,
                    reboot_pending,
                )
            }
        };
        if let DriverUpdateAction::Install { download_url, .. } = &report.action {
            if let Some(release) = report.latest.as_ref() {
                let candidate = candidates.entry(download_url.clone()).or_insert_with(|| {
                    DriverInstallCandidate {
                        release: release.clone(),
                        targets: Vec::new(),
                    }
                });
                candidate.targets.push(DriverInstallTarget {
                    device: report.device.clone(),
                    installed: report.installed.clone(),
                });
            }
        }
        reports.push(report);
    }
    *state.driver_install_candidates.write() = candidates;
    Ok(reports)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn list_driver_history(
    state: State<'_, AppState>,
    model: String,
    vendor: String,
) -> AppResult<Vec<DriverRelease>> {
    let target_vendor = match vendor.to_ascii_lowercase().as_str() {
        "nvidia" => DriverVendor::Nvidia,
        "amd" => DriverVendor::Amd,
        "intel" => DriverVendor::Intel,
        other => {
            return Err(AppError::Other(format!(
                "unsupported driver vendor: {other}"
            )))
        }
    };
    let info = ensure_system_info(&state).await?;
    let gpu = info
        .gpus
        .iter()
        .find(|g| g.model == model && map_vendor(g.vendor) == target_vendor)
        .cloned()
        .ok_or_else(|| AppError::Other(format!("no detected GPU matches model '{model}'")))?;
    let os = os_target_for(&info, &gpu);
    let (device, _) = device_for(&gpu);
    let registry = DriverRegistry::with_default_gpu_sources();
    let client = state.http_catalog.clone();
    let releases = registry
        .history(&client, &device, &os, DEFAULT_HISTORY_LIMIT)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    let target = DriverInstallTarget {
        device,
        installed: DriverVersion::from_installed(target_vendor, &gpu.driver_version),
    };
    let mut candidates = state.driver_install_candidates.write();
    for release in &releases {
        if let Some(download_url) = release.download_url.as_ref() {
            candidates.insert(
                download_url.clone(),
                DriverInstallCandidate {
                    release: release.clone(),
                    targets: vec![target.clone()],
                },
            );
        }
    }
    Ok(releases)
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InstallProgress {
    pub stage: InstallStage,
    pub message: String,
    pub progress: Option<f64>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InstallOutcome {
    pub stage: InstallStage,
    pub exit_code: i32,
    pub message: String,
    pub reboot_required: bool,
}

const DRIVER_INSTALL_EVENT: &str = "driver_install_progress";

/// Launch the downloaded vendor installer, preferring an unattended (silent)
/// run where the vendor supports it (NVIDIA `/s /n`, Intel `/s`) so a routine
/// driver update no longer pops the full vendor GUI. AMD's self-extractor cannot
/// be driven silently, so it falls through to its normal GUI (empty args).
#[cfg(windows)]
fn launch_installer(path: &Path, vendor: &str) -> Result<i32, String> {
    let args = driver_install::launch::silent_install_args(vendor);
    driver_install::launch::launch_and_wait_with_args(path, &args, None).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn launch_installer(_path: &Path, _vendor: &str) -> Result<i32, String> {
    Err("driver install requires Windows".to_string())
}

/// Derive the on-disk installer filename from the download URL. The result is
/// joined under the driver cache dir, so it must be a single path component:
/// strip any query/fragment, take the last `/` segment, and reject anything
/// carrying a path separator, a drive/ADS colon, or a `..` traversal so a
/// crafted URL cannot escape the cache directory.
fn installer_filename(url: &str) -> String {
    const FALLBACK: &str = "driver-setup.exe";
    let candidate = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or("");
    let safe = candidate.to_ascii_lowercase().ends_with(".exe")
        && !candidate.contains(['/', '\\', ':'])
        && !candidate.contains("..");
    if safe {
        candidate.to_string()
    } else {
        FALLBACK.to_string()
    }
}

fn emit_stage(app: &AppHandle, stage: InstallStage, message: &str, progress: Option<f64>) {
    let _ = app.emit(
        DRIVER_INSTALL_EVENT,
        InstallProgress {
            stage,
            message: message.to_string(),
            progress,
        },
    );
}

fn clear_system_info_cache(state: &AppState) {
    *state.system_info.write() = None;
}

async fn collect_fresh_system_info() -> AppResult<SystemInfo> {
    tokio::task::spawn_blocking(system_info::collect)
        .await
        .map_err(|error| AppError::Other(format!("system_info collect: {error}")))
}

fn validate_candidate_baselines(
    candidate: &DriverInstallCandidate,
    info: &SystemInfo,
) -> AppResult<()> {
    for target in &candidate.targets {
        let observed = observed_version(info, &target.device).ok_or_else(|| {
            AppError::Validation(format!(
                "The active driver for '{}' could not be read before installation.",
                target.device.model
            ))
        })?;
        if observed.packed != target.installed.packed {
            return Err(AppError::Validation(format!(
                "The active driver for '{}' changed after the update check. Check again before installing.",
                target.device.model
            )));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn record_driver_outcome(
    state: &AppState,
    candidate: &DriverInstallCandidate,
    observed: Option<&SystemInfo>,
    downloaded_bytes: u64,
    installer_sha256: &str,
    signer: &pe_version::AuthenticodeInfo,
    exit_code: i32,
    reboot_required: bool,
) -> bool {
    let mut changed = false;
    let mut pending = state.driver_reboot_pending.write();
    for target in &candidate.targets {
        let key = device_key(&target.device);
        let observed_after_install =
            observed.and_then(|info| observed_version(info, &target.device));
        if reboot_required {
            pending.insert(
                key,
                DriverRebootEvidence {
                    device: target.device.clone(),
                    expected_version: candidate.release.version.clone(),
                    baseline_version: target.installed.clone(),
                    observed_after_install,
                    expected_size_bytes: candidate.release.size_bytes,
                    downloaded_bytes,
                    installer_sha256: installer_sha256.to_string(),
                    signer_subject: signer.subject_cn.clone(),
                    signer_status: signer.status.clone(),
                    revocation_bypassed: signer.revocation_bypassed,
                    installer_exit_code: exit_code,
                    recorded_at: chrono::Utc::now().to_rfc3339(),
                    boot_time_secs: sysinfo::System::boot_time(),
                },
            );
            changed = true;
        } else if observed_after_install
            .as_ref()
            .is_some_and(|version| version.packed == candidate.release.version.packed)
        {
            changed |= pending.remove(&key).is_some();
        }
    }
    changed
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn install_driver(
    app: AppHandle,
    state: State<'_, AppState>,
    vendor: String,
    download_url: String,
) -> AppResult<InstallOutcome> {
    let download_url = download_url.trim().to_string();
    if download_url.is_empty() {
        return Err(AppError::Validation(
            "No one-click installer is available for this driver branch — open the release notes to download it manually.".into(),
        ));
    }
    load_reboot_state(state.inner())?;
    let candidate = state
        .driver_install_candidates
        .read()
        .get(&download_url)
        .cloned()
        .ok_or_else(|| {
            AppError::Validation(
                "This driver package was not returned by the latest update check. Check again before installing."
                    .into(),
            )
        })?;
    if !vendor_key(candidate.release.vendor).eq_ignore_ascii_case(&vendor) {
        return Err(AppError::Validation(
            "The requested vendor does not match the checked driver package.".into(),
        ));
    }
    let baseline = collect_fresh_system_info().await?;
    validate_candidate_baselines(&candidate, &baseline)?;
    let vendor_kind = crate::netpolicy::validate_driver_url(&vendor, &download_url)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let cache_dir = {
        let guard = state.paths.read();
        guard
            .as_ref()
            .map(|p| p.cache_dir.clone())
            .ok_or_else(|| AppError::Other("app paths not initialized".into()))?
    };
    let staging = {
        let drivers_dir = cache_dir.join("drivers");
        std::fs::create_dir_all(&drivers_dir)
            .map_err(|e| AppError::Other(format!("create drivers cache dir: {e}")))?;
        tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(&drivers_dir)
            .map_err(|e| AppError::Other(format!("create install staging dir: {e}")))?
    };
    let dest = staging.path().join(installer_filename(&download_url));
    let client = state.http_downloads.read().clone();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DownloadProgress>();
    let app_pump = app.clone();
    let pump = tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            let fraction = progress
                .bytes_total
                .filter(|total| *total > 0)
                .map(|total| progress.bytes_downloaded as f64 / total as f64);
            emit_stage(
                &app_pump,
                InstallStage::Downloading,
                "Downloading driver",
                fraction,
            );
        }
    });

    let opts = DownloadOpts {
        progress_tx: Some(tx),
        referer: matches!(vendor_kind, crate::netpolicy::DriverVendor::Amd)
            .then(|| "https://www.amd.com/".to_string()),
        ..Default::default()
    };
    let downloaded = match download_to_file(&client, &download_url, &dest, opts).await {
        Ok(d) => {
            let _ = pump.await;
            d
        }
        Err(e) => {
            let _ = pump.await;
            emit_stage(
                &app,
                InstallStage::Failed,
                &format!("Download failed: {e}"),
                None,
            );
            return Err(AppError::Other(e.to_string()));
        }
    };
    if candidate.release.size_bytes > 0 && downloaded.bytes != candidate.release.size_bytes {
        emit_stage(
            &app,
            InstallStage::Failed,
            "Downloaded driver size does not match the checked package metadata",
            None,
        );
        return Err(AppError::Validation(format!(
            "Driver package size mismatch: expected {} bytes, received {} bytes.",
            candidate.release.size_bytes, downloaded.bytes
        )));
    }
    let installer_sha256 = dll_catalog::hex_sha256_file(&downloaded.path)
        .map_err(|error| AppError::Other(format!("hash downloaded driver: {error}")))?;

    emit_stage(
        &app,
        InstallStage::Verifying,
        "Verifying vendor signature",
        None,
    );
    let verify_path = downloaded.path.clone();
    let verify_vendor = vendor.clone();
    let signer =
        match tokio::task::spawn_blocking(move || verify_signature(&verify_path, &verify_vendor))
            .await
            .map_err(|e| AppError::Other(format!("verify task: {e}")))?
        {
            Ok(signer) => signer,
            Err(e) => {
                emit_stage(
                    &app,
                    InstallStage::Failed,
                    &format!("Signature verification failed: {e}"),
                    None,
                );
                return Err(AppError::Other(e.to_string()));
            }
        };

    emit_stage(
        &app,
        InstallStage::Launching,
        "Launching installer — accept the Windows UAC prompt",
        None,
    );
    emit_stage(
        &app,
        InstallStage::Installing,
        "Vendor installer is running",
        None,
    );
    let launch_path = downloaded.path.clone();
    let launch_vendor = vendor.clone();
    let launch =
        tokio::task::spawn_blocking(move || launch_installer(&launch_path, &launch_vendor)).await;
    let exit_code = match launch {
        Ok(Ok(code)) => code,
        Ok(Err(e)) => {
            emit_stage(
                &app,
                InstallStage::Failed,
                &format!("Launch failed: {e}"),
                None,
            );
            return Err(AppError::Other(e));
        }
        Err(e) => {
            let msg = format!("launch task: {e}");
            emit_stage(&app, InstallStage::Failed, &msg, None);
            return Err(AppError::Other(msg));
        }
    };

    let stage = classify_exit(exit_code);
    let message = describe_exit(exit_code, &vendor);
    emit_stage(&app, stage, &message, None);
    let requires_reboot = reboot_required(exit_code);
    if matches!(stage, InstallStage::Completed) {
        let observed = match collect_fresh_system_info().await {
            Ok(info) => {
                *state.system_info.write() = Some(info.clone());
                Some(info)
            }
            Err(error) => {
                tracing::warn!(%error, "fresh driver readback failed after installer exit");
                clear_system_info_cache(state.inner());
                None
            }
        };
        if record_driver_outcome(
            state.inner(),
            &candidate,
            observed.as_ref(),
            downloaded.bytes,
            &installer_sha256,
            &signer,
            exit_code,
            requires_reboot,
        ) {
            if let Err(error) = persist_reboot_state(state.inner()) {
                tracing::warn!(%error, "driver reboot evidence could not be persisted");
            }
        }
        state
            .driver_install_candidates
            .write()
            .remove(&download_url);
    }
    Ok(InstallOutcome {
        stage,
        exit_code,
        message,
        reboot_required: requires_reboot,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_info::OsInfo;

    fn sysinfo_with_build(build: &str) -> SystemInfo {
        SystemInfo {
            os: OsInfo {
                name: "Windows".into(),
                version: "10.0".into(),
                build: build.into(),
                edition: "Windows 11 Pro".into(),
            },
            cpu: Default::default(),
            ram: Default::default(),
            gpus: vec![],
            collected_at: chrono::Utc::now(),
        }
    }

    fn nvidia_gpu(is_dch: bool) -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Nvidia,
            pci_vendor_id: 0x10DE,
            pci_device_id: 0x2705,
            model: "NVIDIA GeForce RTX 4070 Ti SUPER".into(),
            driver_version: "32.0.15.9174".into(),
            vram_bytes: 0,
            recommended_runtimes: vec![],
            is_dch,
            identifiable: true,
            fsr4_capable: false,
        }
    }

    #[test]
    fn os_target_picks_windows_11_at_build_threshold() {
        let gpu = nvidia_gpu(true);
        assert_eq!(
            os_target_for(&sysinfo_with_build("22631"), &gpu).family,
            OsFamily::Windows11X64
        );
        assert_eq!(
            os_target_for(&sysinfo_with_build("19045"), &gpu).family,
            OsFamily::Windows10X64
        );
        assert_eq!(
            os_target_for(&sysinfo_with_build(""), &gpu).family,
            OsFamily::Windows10X64
        );
    }

    #[test]
    fn os_target_propagates_per_gpu_dch_flag() {
        let info = sysinfo_with_build("22631");
        assert!(os_target_for(&info, &nvidia_gpu(true)).dch);
        assert!(!os_target_for(&info, &nvidia_gpu(false)).dch);
    }

    #[test]
    fn map_vendor_maps_each_gpu_vendor() {
        assert_eq!(map_vendor(GpuVendor::Nvidia), DriverVendor::Nvidia);
        assert_eq!(map_vendor(GpuVendor::Amd), DriverVendor::Amd);
        assert_eq!(map_vendor(GpuVendor::Intel), DriverVendor::Intel);
        assert_eq!(map_vendor(GpuVendor::Other), DriverVendor::Other);
    }

    #[test]
    fn device_for_nvidia_normalizes_installed_version() {
        let gpu = GpuInfo {
            vendor: GpuVendor::Nvidia,
            pci_vendor_id: driver_catalog::consts::pci::NVIDIA,
            pci_device_id: 0x2705,
            model: "NVIDIA GeForce RTX 4070 Ti SUPER".into(),
            driver_version: "32.0.15.9174".into(),
            vram_bytes: 0,
            recommended_runtimes: vec![],
            is_dch: true,
            identifiable: true,
            fsr4_capable: false,
        };
        let (device, installed) = device_for(&gpu);
        assert_eq!(device.vendor, DriverVendor::Nvidia);
        assert_eq!(device.pci_vendor_id, driver_catalog::consts::pci::NVIDIA);
        assert_eq!(device.pci_device_id, 0x2705);
        assert_eq!(installed.display, "591.74");
    }

    #[test]
    fn installer_filename_keeps_a_clean_exe_segment() {
        assert_eq!(
            installer_filename(
                "https://us.download.nvidia.com/Windows/610.47/610.47-desktop-win10-win11-64bit-international-dch-whql.exe"
            ),
            "610.47-desktop-win10-win11-64bit-international-dch-whql.exe"
        );
    }

    #[test]
    fn installer_filename_strips_query_and_fragment() {
        assert_eq!(
            installer_filename("https://host/setup.exe?token=abc"),
            "setup.exe"
        );
        assert_eq!(
            installer_filename("https://host/setup.exe#frag"),
            "setup.exe"
        );
    }

    #[test]
    fn installer_filename_rejects_path_traversal_and_separators() {
        assert_eq!(
            installer_filename("https://host/x/..\\..\\..\\Windows\\System32\\evil.exe"),
            "driver-setup.exe"
        );
        assert_eq!(
            installer_filename("https://host/C:evil.exe"),
            "driver-setup.exe"
        );
        assert_eq!(
            installer_filename("https://host/..%2fevil.exe"),
            "driver-setup.exe"
        );
    }

    #[test]
    fn installer_filename_falls_back_when_not_an_exe() {
        assert_eq!(installer_filename("https://host/page"), "driver-setup.exe");
        assert_eq!(
            installer_filename("https://host/archive.zip"),
            "driver-setup.exe"
        );
    }

    #[test]
    fn clear_system_info_cache_resets_to_none() {
        let state = AppState::new();
        *state.system_info.write() = Some(sysinfo_with_build("22631"));
        assert!(state.system_info.read().is_some());
        clear_system_info_cache(&state);
        assert!(state.system_info.read().is_none());
    }
}
