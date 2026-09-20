//! General PC driver engine commands (audio/network/Bluetooth/input/storage/
//! chipset/USB/firmware/camera/printer) backed by `system-drivers`
//! (WMI inventory + Windows Update Agent / Microsoft Update Catalog, with the
//! anti-downgrade guard). GPUs keep their dedicated vendor sources elsewhere.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use system_drivers::{DeviceGroup, InstallProgress, InstallReport, InstallStage};
use tauri::{AppHandle, Emitter, State};

const SYSTEM_DRIVER_INSTALL_EVENT: &str = "system_driver_install_progress";
const MAX_UPDATE_ID_LEN: usize = 128;

/// A WUA `UpdateID:RevisionNumber` is a GUID (hex + dashes, optionally braced)
/// followed by `:` and an integer. Restricting to that character set blocks the
/// argument-injection vector: with no space or quote permitted, a crafted value
/// cannot break out of its quoted token to inject extra flags into the elevated
/// child. Defence-in-depth on top of [`driver_install::launch::build_command_line`].
fn is_valid_update_id(id: &str) -> bool {
    if id.len() > MAX_UPDATE_ID_LEN {
        return false;
    }
    let Some((guid, revision)) = id.rsplit_once(':') else {
        return false;
    };
    uuid::Uuid::parse_str(guid).is_ok()
        && !revision.is_empty()
        && revision.bytes().all(|byte| byte.is_ascii_digit())
        && revision.parse::<u32>().is_ok()
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SystemDriverOutcome {
    pub success: bool,
    pub reboot_required: bool,
    pub result_code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<SystemDriverVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SystemDriverVerification {
    ActiveVersionVerified,
    RebootPending,
    Unverified,
}

#[cfg(windows)]
fn inventory_blocking() -> Result<Vec<system_drivers::SystemDevice>, String> {
    use system_drivers::DeviceCatalog;
    system_drivers::WmiInventory
        .inventory()
        .map_err(|error| error.to_string())
}
#[cfg(not(windows))]
fn inventory_blocking() -> Result<Vec<system_drivers::SystemDevice>, String> {
    Err("Device inventory requires Windows".into())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn get_system_devices() -> AppResult<Vec<system_drivers::SystemDevice>> {
    tokio::task::spawn_blocking(inventory_blocking)
        .await
        .map_err(|error| AppError::Other(error.to_string()))?
        .map_err(AppError::Other)
}

fn verify_active_version(
    outcome: &mut SystemDriverOutcome,
    devices: &[system_drivers::SystemDevice],
    instance_id: Option<&str>,
    expected: Option<&str>,
) {
    if !outcome.success {
        return;
    }
    let observed = instance_id
        .and_then(|id| {
            devices
                .iter()
                .find(|device| device.present && device.hardware_id.eq_ignore_ascii_case(id))
        })
        .and_then(|device| device.driver_version.clone());
    outcome.observed_version = observed.clone();
    if expected.is_some() && observed.as_deref() == expected {
        outcome.verification = Some(SystemDriverVerification::ActiveVersionVerified);
        outcome.message = format!(
            "Windows reports the requested active driver version {}.",
            expected.unwrap_or_default()
        );
    } else if outcome.reboot_required {
        outcome.verification = Some(SystemDriverVerification::RebootPending);
        outcome.message = "Windows accepted the package. Restart is required; the requested active version is not yet verified.".into();
    } else {
        outcome.verification = Some(SystemDriverVerification::Unverified);
        outcome.message = "Windows accepted the package, but the requested active driver version could not be verified. Rescan the device before retrying.".into();
    }
}

/// Installed-device context the install carries so it can snapshot the current
/// driver (`pnputil /export-driver`) and record a rollback-able backup before
/// applying the update. Sourced from the matched `DriverUpdate` fields.
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DriverInstallContext {
    pub inf_name: Option<String>,
    pub hardware_id: Option<String>,
    pub device_class: Option<String>,
    pub provider: Option<String>,
    pub current_version: Option<String>,
}

/// One DriverStore version of a driver package (current or superseded), for the
/// "old / latest versions" display in System & Components.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DriverStoreVersion {
    pub published_name: String,
    pub version: String,
    pub date: Option<String>,
    pub provider: String,
    pub current: bool,
}

#[cfg(windows)]
fn scan_blocking() -> Result<Vec<DeviceGroup>, String> {
    use system_drivers::{
        dedup_updates, filter_safe_updates, group_by_class, DeviceCatalog, UpdateSource,
        WmiInventory, WuaSource,
    };
    let devices = WmiInventory.inventory().map_err(|e| e.to_string())?;
    let updates = WuaSource.search().map_err(|e| e.to_string())?;
    let safe = filter_safe_updates(&devices, updates);
    let deduped = dedup_updates(&devices, safe);
    Ok(group_by_class(deduped))
}

#[cfg(not(windows))]
fn scan_blocking() -> Result<Vec<DeviceGroup>, String> {
    Err("system driver scan requires Windows".to_string())
}

/// WUA download+install are Administrator-only (they fail with E_ACCESSDENIED
/// when the app runs unelevated). Run the install in an elevated child of
/// ourselves via UAC — the same per-action elevation model as the GPU installer
/// — and read back the `InstallReport` JSON it writes. The app stays unelevated.
/// Elevated WUA install can legitimately take a while (download + driver
/// service work); cap the wait so a stuck UAC dialog or hung WUA service can
/// never freeze the UI forever — the child is terminated and a Failed terminal
/// state is surfaced.
#[cfg(windows)]
const SYSTEM_DRIVER_INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[cfg(windows)]
fn install_blocking(
    app: &AppHandle,
    update_id: &str,
    context: &DriverInstallContext,
    expected_version: Option<&str>,
) -> Result<SystemDriverOutcome, String> {
    let emit = |stage: InstallStage, message: &str, fraction: Option<f64>| {
        let _ = app.emit(
            SYSTEM_DRIVER_INSTALL_EVENT,
            &InstallProgress {
                stage,
                message: message.to_string(),
                fraction,
            },
        );
    };

    emit(
        InstallStage::Installing,
        "Waiting for administrator approval…",
        Some(0.05),
    );

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exchange = tempfile::Builder::new()
        .prefix("dlssync-wua-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let result_path = exchange.path().join("result.json");
    let progress_path = exchange.path().join("progress.json");

    let snapshot = plan_driver_snapshot(app, context);

    let mut parts: Vec<String> = vec![
        "--wua-install".into(),
        update_id.to_string(),
        "--result".into(),
        result_path.display().to_string(),
        "--progress-file".into(),
        progress_path.display().to_string(),
    ];
    if let Some((inf, dest, _)) = snapshot.as_ref() {
        parts.push("--snapshot-inf".into());
        parts.push(inf.clone());
        parts.push("--snapshot-dest".into());
        parts.push(dest.display().to_string());
    }
    let args = driver_install::launch::build_command_line(&parts);

    let mut last_emitted = String::new();
    let on_tick = || {
        if let Ok(json) = std::fs::read_to_string(&progress_path) {
            if json != last_emitted {
                if let Ok(p) = serde_json::from_str::<InstallProgress>(&json) {
                    let _ = app.emit(SYSTEM_DRIVER_INSTALL_EVENT, &p);
                    last_emitted = json;
                }
            }
        }
    };

    let code = driver_install::launch::launch_elevated(
        &exe,
        &args,
        None,
        Some(SYSTEM_DRIVER_INSTALL_TIMEOUT),
        on_tick,
    )
    .map_err(|e| e.to_string())?;

    if code == driver_install::launch::UAC_DECLINED_EXIT {
        let msg = "Administrator approval was declined — the update was not installed.";
        emit(InstallStage::Failed, msg, Some(1.0));
        let _ = std::fs::remove_file(&progress_path);
        return Ok(SystemDriverOutcome {
            verification: None,
            observed_version: None,
            success: false,
            reboot_required: false,
            result_code: code,
            message: msg.to_string(),
        });
    }

    let mut outcome = match std::fs::read_to_string(&result_path) {
        Ok(json) => match serde_json::from_str::<InstallReport>(&json) {
            Ok(r) => SystemDriverOutcome {
                verification: None,
                observed_version: None,
                success: r.success,
                reboot_required: r.reboot_required,
                result_code: r.result_code,
                message: r.message,
            },
            Err(e) => SystemDriverOutcome {
                verification: None,
                observed_version: None,
                success: false,
                reboot_required: false,
                result_code: code,
                message: format!(
                    "The update finished (exit {code}) but its result could not be parsed: {e}"
                ),
            },
        },
        Err(e) => {
            let message = if code == 3 {
                "Windows Update refused the install — per-machine access denied (0x80240044). \
                 Try again, or install this driver from Windows Update directly."
                    .to_string()
            } else {
                format!(
                    "The elevated installer exited with code {code} without reporting a result ({e})."
                )
            };
            SystemDriverOutcome {
                verification: None,
                observed_version: None,
                success: false,
                reboot_required: false,
                result_code: code,
                message,
            }
        }
    };
    let _ = std::fs::remove_file(&result_path);
    let _ = std::fs::remove_file(&progress_path);

    // Keep a valid pre-update snapshot even when installation failed.
    if let Some((inf, dest, stamp)) = snapshot {
        record_driver_backup(app, context, &inf, &dest, stamp);
    }
    match inventory_blocking() {
        Ok(devices) => verify_active_version(
            &mut outcome,
            &devices,
            context.hardware_id.as_deref(),
            expected_version,
        ),
        Err(error) if outcome.success => {
            outcome.verification = Some(SystemDriverVerification::Unverified);
            outcome.message =
                format!("Windows accepted the package; device readback failed: {error}");
        }
        Err(_) => {}
    }

    emit(
        if outcome.success {
            InstallStage::Completed
        } else {
            InstallStage::Failed
        },
        &outcome.message,
        Some(1.0),
    );
    Ok(outcome)
}

/// Decide where the elevated child should export the current driver before the
/// update, returning `(published_inf, dest_dir, timestamp)`. Only fires when the
/// device reports a DriverStore `oemNN.inf` and the backup store is open. The
/// child creates `dest_dir`; we only compute the path here so the parent (which
/// owns the DB) can record the backup row afterwards.
#[cfg(windows)]
fn plan_driver_snapshot(
    app: &AppHandle,
    context: &DriverInstallContext,
) -> Option<(String, std::path::PathBuf, chrono::DateTime<chrono::Utc>)> {
    use tauri::Manager;
    let inf = context.inf_name.as_deref()?;
    if !system_drivers::is_published_oem_inf(inf) {
        return None;
    }
    let state = app.state::<AppState>();
    let guard = state.backups.read();
    let root = guard.as_ref()?.root_dir.clone();
    let key = context
        .hardware_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(inf);
    let stamp = chrono::Utc::now();
    let dest = root
        .join("driver-backups")
        .join(backup_store::sanitize_folder_name(key))
        .join(format!(
            "{}-{}",
            stamp.format("%Y-%m-%d %H-%M-%S"),
            uuid::Uuid::new_v4()
        ));
    Some((inf.to_string(), dest, stamp))
}

/// Record a `driver_package` backup row once the elevated child has exported a
/// non-empty snapshot. A no-op when the export produced nothing (driver not in
/// the DriverStore as an `oemNN.inf`).
#[cfg(windows)]
fn record_driver_backup(
    app: &AppHandle,
    context: &DriverInstallContext,
    inf: &str,
    dest: &std::path::Path,
    stamp: chrono::DateTime<chrono::Utc>,
) {
    use tauri::Manager;
    let Ok(size_bytes) = system_drivers::verify_driver_snapshot(dest) else {
        return;
    };
    let hardware_id = context
        .hardware_id
        .clone()
        .unwrap_or_else(|| "system-driver".to_string());
    let device_class = context
        .device_class
        .clone()
        .unwrap_or_else(|| "Driver".to_string());
    let entry = backup_store::BackupEntry {
        id: uuid::Uuid::new_v4().to_string(),
        game_id: hardware_id.clone(),
        dll_family: device_class.clone(),
        dll_filename: inf.to_string(),
        original_path: std::path::PathBuf::from(&hardware_id),
        backup_path: dest.to_path_buf(),
        previous_version: context.current_version.clone(),
        previous_sha256: None,
        created_at: stamp,
        restored_at: None,
        size_bytes: Some(size_bytes),
        backup_type: "driver_package".to_string(),
        device_class: Some(device_class),
        hardware_id: Some(hardware_id),
        driver_provider: context.provider.clone(),
    };
    let state = app.state::<AppState>();
    let guard = state.backups.read();
    if let Some(store) = guard.as_ref() {
        if let Err(e) = store.insert(&entry) {
            tracing::warn!(error = %e, "failed to record driver-package backup");
        }
    }
}

#[cfg(not(windows))]
fn install_blocking(
    _app: &AppHandle,
    _update_id: &str,
    _context: &DriverInstallContext,
    _expected_version: Option<&str>,
) -> Result<SystemDriverOutcome, String> {
    Err("driver install requires Windows".to_string())
}

/// Scan every non-GPU device for a newer signed driver on Windows Update /
/// the Microsoft Update Catalog, grouped by device class. Honors the
/// anti-downgrade guard: an entry only appears when it is provably newer than
/// the installed driver.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn scan_system_drivers() -> AppResult<Vec<DeviceGroup>> {
    tokio::task::spawn_blocking(scan_blocking)
        .await
        .map_err(|e| AppError::Other(format!("system driver scan task: {e}")))?
        .map_err(AppError::Other)
}

/// Download + install one driver update by its `UpdateID:RevisionNumber`,
/// emitting `system_driver_install_progress` events along the way. `context`
/// carries the matched device's DriverStore INF so the elevated child can
/// snapshot the current driver (and lay a System Restore checkpoint) before
/// applying the update — recorded as a rollback-able `driver_package` backup.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn install_system_driver(
    app: AppHandle,
    update_id: String,
    context: Option<DriverInstallContext>,
) -> AppResult<SystemDriverOutcome> {
    if !is_valid_update_id(&update_id) {
        return Err(AppError::Validation(format!(
            "rejected malformed WUA update id: {update_id:?}"
        )));
    }
    // The renderer's old context is a hint only. Re-resolve the offered update and device before elevation.
    let _ = context;
    tokio::task::spawn_blocking(move || {
        let update = scan_blocking()?
            .into_iter()
            .flat_map(|group| group.updates)
            .find(|update| update.update_id == update_id)
            .ok_or_else(|| {
                "This driver update is no longer offered for this PC. Rescan to refresh the list."
                    .to_string()
            })?;
        let context = DriverInstallContext {
            inf_name: update.target_inf,
            hardware_id: update.target_hardware_id,
            device_class: Some(format!("{:?}", update.class)),
            provider: Some(update.provider),
            current_version: update.current_version,
        };
        install_blocking(&app, &update_id, &context, update.driver_version.as_deref())
    })
    .await
    .map_err(|e| AppError::Other(format!("system driver install task: {e}")))?
    .map_err(AppError::Other)
}

/// Roll a System & Components driver back to a previously-snapshotted version by
/// re-installing its exported DriverStore package (`pnputil /add-driver
/// /install`) via an elevated child. Marks the backup restored on success.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn restore_system_driver(
    state: State<'_, AppState>,
    backup_id: String,
) -> AppResult<SystemDriverOutcome> {
    let (entry, root_dir) = {
        let guard = state.backups.read();
        let store = guard
            .as_ref()
            .ok_or_else(|| AppError::Other("backup store not initialized".into()))?;
        (store.get(&backup_id)?, store.root_dir.clone())
    };
    if entry.backup_type != "driver_package" {
        return Err(AppError::Other(
            "this backup is a game DLL, not a system driver".into(),
        ));
    }
    crate::paths::PathGuard::assert_under_root(&entry.backup_path, &root_dir)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    crate::paths::PathGuard::assert_not_symlink(&entry.backup_path)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    system_drivers::verify_driver_snapshot(&entry.backup_path).map_err(AppError::Validation)?;
    let mut outcome = restore_blocking(entry.backup_path.clone()).await?;
    let devices = tokio::task::spawn_blocking(inventory_blocking)
        .await
        .map_err(|error| AppError::Other(error.to_string()))?
        .map_err(AppError::Other)?;
    verify_active_version(
        &mut outcome,
        &devices,
        entry.hardware_id.as_deref(),
        entry.previous_version.as_deref(),
    );
    if outcome.success
        && matches!(
            outcome.verification,
            Some(SystemDriverVerification::ActiveVersionVerified)
        )
    {
        let guard = state.backups.read();
        if let Some(store) = guard.as_ref() {
            store.mark_restored(&backup_id, chrono::Utc::now())?;
        }
    }
    Ok(outcome)
}

#[cfg(windows)]
async fn restore_blocking(dir: std::path::PathBuf) -> AppResult<SystemDriverOutcome> {
    const RESTORE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);
    tokio::task::spawn_blocking(move || -> Result<SystemDriverOutcome, String> {
        if !dir.is_dir() {
            return Err(format!("snapshot folder is missing: {}", dir.display()));
        }
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let exchange = tempfile::Builder::new()
            .prefix("dlssync-driver-restore-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let result_path = exchange.path().join("result.json");
        let args = driver_install::launch::build_command_line([
            "--restore-driver",
            &dir.display().to_string(),
            "--result",
            &result_path.display().to_string(),
        ]);
        let code = driver_install::launch::launch_elevated(
            &exe,
            &args,
            None,
            Some(RESTORE_TIMEOUT),
            || {},
        )
        .map_err(|e| e.to_string())?;
        if code == driver_install::launch::UAC_DECLINED_EXIT {
            return Ok(SystemDriverOutcome {
                verification: None,
                observed_version: None,
                success: false,
                reboot_required: false,
                result_code: code,
                message: "Administrator approval was declined — the rollback did not run."
                    .to_string(),
            });
        }
        let outcome = match std::fs::read_to_string(&result_path) {
            Ok(json) => match serde_json::from_str::<InstallReport>(&json) {
                Ok(r) => SystemDriverOutcome {
                    verification: None,
                    observed_version: None,
                    success: r.success,
                    reboot_required: r.reboot_required,
                    result_code: r.result_code,
                    message: r.message,
                },
                Err(e) => SystemDriverOutcome {
                    verification: None,
                    observed_version: None,
                    success: false,
                    reboot_required: false,
                    result_code: code,
                    message: format!(
                        "Rollback finished (exit {code}) but its result was unreadable: {e}"
                    ),
                },
            },
            Err(e) => SystemDriverOutcome {
                verification: None,
                observed_version: None,
                success: false,
                reboot_required: false,
                result_code: code,
                message: format!(
                    "The rollback helper exited with code {code} without a result ({e})."
                ),
            },
        };
        let _ = std::fs::remove_file(&result_path);
        Ok(outcome)
    })
    .await
    .map_err(|e| AppError::Other(format!("driver rollback task: {e}")))?
    .map_err(AppError::Other)
}

#[cfg(not(windows))]
async fn restore_blocking(_dir: std::path::PathBuf) -> AppResult<SystemDriverOutcome> {
    Err(AppError::Other("driver rollback requires Windows".into()))
}

/// List the DriverStore versions (current + superseded) of the driver package
/// published as `inf_name` (`oemNN.inf`), newest-first, so the UI can show the
/// installed version alongside the older ones still cached locally. Reads
/// `pnputil /enum-drivers` (works unelevated); returns an empty list off Windows
/// or when the package isn't found.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn system_driver_versions(inf_name: String) -> AppResult<Vec<DriverStoreVersion>> {
    if !system_drivers::is_published_oem_inf(&inf_name) {
        return Ok(Vec::new());
    }
    #[cfg(windows)]
    {
        tokio::task::spawn_blocking(move || enum_driver_versions(&inf_name))
            .await
            .map_err(|e| AppError::Other(format!("driver versions task: {e}")))?
            .map_err(AppError::Other)
    }
    #[cfg(not(windows))]
    {
        let _ = inf_name;
        Ok(Vec::new())
    }
}

#[cfg(windows)]
fn enum_driver_versions(inf_name: &str) -> Result<Vec<DriverStoreVersion>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let inf = inf_name.to_ascii_lowercase();
    let output = std::process::Command::new("pnputil.exe")
        .args(["/enum-drivers"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("pnputil /enum-drivers: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let packages = system_drivers::parse_enum_drivers(&text);
    let Some(original) = packages
        .iter()
        .find(|p| p.published_name.eq_ignore_ascii_case(&inf))
        .map(|p| p.original_name.to_ascii_lowercase())
    else {
        return Ok(Vec::new());
    };
    let groups = system_drivers::versions_by_original_name(&packages);
    let list = groups.get(&original).cloned().unwrap_or_default();
    Ok(list
        .into_iter()
        .map(|p| DriverStoreVersion {
            current: p.published_name.eq_ignore_ascii_case(&inf),
            published_name: p.published_name,
            version: p.version,
            date: p.date,
            provider: p.provider,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_success_does_not_establish_the_active_driver_version() {
        let mut outcome = SystemDriverOutcome {
            success: true,
            reboot_required: false,
            result_code: 0,
            message: String::new(),
            verification: None,
            observed_version: None,
        };
        verify_active_version(&mut outcome, &[], Some("DEVICE"), Some("2.0"));
        assert!(matches!(
            outcome.verification,
            Some(SystemDriverVerification::Unverified)
        ));
        outcome.reboot_required = true;
        verify_active_version(&mut outcome, &[], Some("DEVICE"), Some("2.0"));
        assert!(matches!(
            outcome.verification,
            Some(SystemDriverVerification::RebootPending)
        ));
        let device = system_drivers::SystemDevice {
            name: "Test".into(),
            class: system_drivers::DeviceClass::Audio,
            manufacturer: "Vendor".into(),
            driver_version: Some("2.0".into()),
            driver_date: None,
            hardware_id: "DEVICE".into(),
            hardware_ids: Vec::new(),
            problem_code: Some(0),
            inf_name: None,
            present: true,
        };
        verify_active_version(&mut outcome, &[device], Some("DEVICE"), Some("2.0"));
        assert!(matches!(
            outcome.verification,
            Some(SystemDriverVerification::ActiveVersionVerified)
        ));
    }

    #[test]
    fn accepts_real_wua_update_ids() {
        assert!(is_valid_update_id(
            "12345678-1234-1234-1234-123456789abc:200"
        ));
        assert!(is_valid_update_id(
            "{12345678-1234-1234-1234-123456789abc}:7"
        ));
    }

    #[test]
    fn rejects_injection_payloads() {
        // Space + quote are the levers for breaking out of the quoted token.
        assert!(!is_valid_update_id("abc\" --restore-driver \"C:\\evil:1"));
        assert!(!is_valid_update_id("id:1 --snapshot-inf x"));
        assert!(!is_valid_update_id("../../etc/passwd:1"));
    }

    #[cfg(windows)]
    #[test]
    fn restore_args_neutralize_quoted_backup_path() {
        let hostile_dir = r#"C:\backups\evil" --snapshot-inf "C:\x"#;
        let cmd = driver_install::launch::build_command_line([
            "--restore-driver",
            hostile_dir,
            "--result",
            r"C:\t\r.json",
        ]);
        assert!(!cmd.contains(r#"evil" --snapshot-inf"#));
        assert!(cmd.contains(r#"evil\""#));
    }

    #[test]
    fn rejects_missing_or_nonnumeric_revision() {
        assert!(!is_valid_update_id("12345678-1234:abc"));
        assert!(!is_valid_update_id("12345678-1234"));
        assert!(!is_valid_update_id(""));
    }

    #[test]
    fn rejects_overlong_input() {
        let long = format!("{}:1", "a".repeat(200));
        assert!(!is_valid_update_id(&long));
    }
}
