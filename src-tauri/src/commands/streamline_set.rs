use crate::commands::apply::{
    apply_item_group, lookup_release, streamline_guard, ApplyOutcome, ApplyRequest,
    BatchRegistration, StateHandles,
};
use crate::error::AppResult;
use crate::state::AppState;
#[cfg(test)]
use crate::system_info::GpuInfo;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize, specta::Type)]
pub struct StreamlineSetResult {
    pub success: bool,
    pub applied: Vec<ApplyOutcome>,
    pub error: Option<String>,
    pub rolled_back: bool,
}

/// Apply an NVIDIA Streamline plugin set as one all-or-nothing transaction. The
/// `sl.*` plugins are version-locked: every member must come from the same SDK
/// release, and a partially-swapped set crashes the game on launch. So this
/// rejects a mixed-version set, refuses to start when any member is blocked
/// (DLSS Enabler / opt-in off / cross-major), and on any member failure rolls
/// every already-swapped member back to its backup.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_streamline_set(
    handle: AppHandle,
    state: State<'_, AppState>,
    items: Vec<ApplyRequest>,
) -> AppResult<StreamlineSetResult> {
    let registration = BatchRegistration::new(&handle, state.apply_registry.clone(), &items);
    let handles = state.inner().clone_handles();
    let guard_handles = state.inner().clone_handles();
    run_set(&handle, handles, items, &registration.tokens, move |item| {
        let dll_path = PathBuf::from(&item.dll_path);
        streamline_guard(&guard_handles, &dll_path, &item.target_version)
    })
    .await
}

/// Apply any coherent multi-DLL family set (FSR loader/upscaler/frame-generation,
/// XeSS libxess/libxell/libxess_fg) as one all-or-nothing transaction. Members are
/// version-locked to ONE SDK release (the spike-verified FSR SDK 2.2.0 ships
/// loader 2.2.0 + upscaler 4.1.0 + frame-gen 4.0.0 in a single zip), and 4.x FSR
/// members are refused outright when no RDNA4 GPU is present.
#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn apply_dll_set(
    handle: AppHandle,
    state: State<'_, AppState>,
    items: Vec<ApplyRequest>,
) -> AppResult<StreamlineSetResult> {
    let registration = BatchRegistration::new(&handle, state.apply_registry.clone(), &items);
    let handles = state.inner().clone_handles();
    // The common batch owner evaluates hardware after dependency expansion.
    run_set(&handle, handles, items, &registration.tokens, |_| None).await
}

async fn run_set(
    handle: &AppHandle,
    handles: StateHandles,
    items: Vec<ApplyRequest>,
    tokens: &std::collections::HashMap<String, tokio_util::sync::CancellationToken>,
    guard: impl Fn(&ApplyRequest) -> Option<String>,
) -> AppResult<StreamlineSetResult> {
    if items.is_empty() {
        return Ok(StreamlineSetResult {
            success: true,
            applied: vec![],
            error: None,
            rolled_back: false,
        });
    }

    let mut cdn_urls = Vec::with_capacity(items.len());
    for item in &items {
        let release = lookup_release(&handles, item).await?;
        cdn_urls.push(
            release
                .artifact
                .as_ref()
                .map(|artifact| artifact.package_id.clone())
                .unwrap_or(release.cdn_url),
        );
    }
    if let Some(reason) = coherence_error(&cdn_urls) {
        return Ok(set_failure(vec![], &reason, false));
    }

    for item in &items {
        if let Some(reason) = guard(item) {
            return Ok(set_failure(vec![], &reason, false));
        }
    }

    let result = apply_item_group(
        handle,
        &handles,
        &items,
        tokens,
        None,
        dlssync_contracts::OperationActor::Gui,
    )
    .await;
    let applied = result?;
    if let Some(failure) = applied.iter().find(|outcome| !outcome.success) {
        let reason = failure
            .error
            .clone()
            .unwrap_or_else(|| "transaction failed".into());
        let rolled_back = set_recovery_verified(&reason, false, true);
        return Ok(set_failure(applied, &reason, rolled_back));
    }
    Ok(StreamlineSetResult {
        success: true,
        applied,
        error: None,
        rolled_back: false,
    })
}

fn set_recovery_verified(reason: &str, had_prior_members: bool, restored_prior: bool) -> bool {
    !reason.contains("rollback_failed:")
        && restored_prior
        && (had_prior_members || reason.starts_with("rolled_back:"))
}

/// 4.x FSR binaries only run on RDNA4 silicon today; offering or applying them on
/// older AMD (or non-AMD) hardware breaks the game's upscaler outright. Fails
/// closed when the GPU inventory is empty/unknown.
#[cfg(test)]
fn fsr4_guard(gpus: &[GpuInfo], family: &str, target_version: &str) -> Option<String> {
    dlssync_application::policy::fsr4_block_reason(
        gpus.iter().any(|gpu| gpu.fsr4_capable),
        family,
        target_version,
    )
}

/// A Streamline set is version-locked: every member must resolve to the same SDK
/// release (one `cdn_url`). Returns the rejection reason when the members span
/// more than one release, else `None`. Empty/single sets are trivially coherent.
fn coherence_error(cdn_urls: &[String]) -> Option<String> {
    let first = cdn_urls.first()?;
    if cdn_urls.iter().any(|url| url != first) {
        return Some(
            "Streamline set members resolve to different SDK releases — refusing a mixed-version \
             set (the plug-ins are version-locked)."
                .to_string(),
        );
    }
    None
}

fn set_failure(
    mut applied: Vec<ApplyOutcome>,
    error: &str,
    rolled_back: bool,
) -> StreamlineSetResult {
    // Earlier per-file successes are no longer installed outcomes after a set fails.
    for outcome in &mut applied {
        if outcome.success {
            outcome.success = false;
            outcome.new_version = None;
            outcome.error = Some(if rolled_back {
                "rolled_back: set member failed; original bytes verified".into()
            } else {
                "rollback_failed: set member failed; recovery requires attention".into()
            });
        }
    }
    StreamlineSetResult {
        success: false,
        applied,
        error: Some(error.to_string()),
        rolled_back,
    }
}

/// A backup may only be restored from inside the backup store root. The DB-stored
/// `backup_path` is otherwise an unvalidated copy SOURCE, so a tampered row could
/// point the restore at an arbitrary file. `None`/non-`starts_with` paths are
/// rejected.
#[cfg(test)]
fn backup_path_under_root(backup_path: &std::path::Path, root: &std::path::Path) -> bool {
    crate::paths::PathGuard::assert_under_root(backup_path, root).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_current_recovery_cannot_be_hidden_by_prior_restoration() {
        assert!(!set_recovery_verified(
            "rollback_failed: current member",
            true,
            true
        ));
        assert!(!set_recovery_verified("download failed", false, true));
        assert!(set_recovery_verified("download failed", true, true));
        assert!(set_recovery_verified(
            "rolled_back: current member",
            false,
            true
        ));
        assert!(!set_recovery_verified(
            "rolled_back: current member",
            true,
            false
        ));
    }

    #[test]
    fn rolled_back_members_are_no_longer_successful_installed_outcomes() {
        let result = set_failure(
            vec![ApplyOutcome {
                apply_id: "a".into(),
                success: true,
                backup_id: Some("backup".into()),
                previous_version: Some("1".into()),
                new_version: Some("2".into()),
                error: None,
            }],
            "second member failed",
            true,
        );
        assert!(!result.success);
        assert!(result.rolled_back);
        assert!(!result.applied[0].success);
        assert!(result.applied[0].new_version.is_none());
        assert_eq!(result.applied[0].backup_id.as_deref(), Some("backup"));
    }

    fn urls(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn coherent_set_has_no_error() {
        let zip = "https://github.com/NVIDIA-RTX/Streamline/releases/download/v2.11.1/sdk.zip";
        assert!(coherence_error(&urls(&[zip, zip, zip])).is_none());
        assert!(coherence_error(&urls(&[zip])).is_none());
        assert!(coherence_error(&[]).is_none());
    }

    #[test]
    fn mixed_version_set_is_rejected() {
        let v211 = "https://github.com/NVIDIA-RTX/Streamline/releases/download/v2.11.1/sdk.zip";
        let v210 = "https://github.com/NVIDIA-RTX/Streamline/releases/download/v2.10.3/sdk.zip";
        let reason = coherence_error(&urls(&[v211, v211, v210])).unwrap();
        assert!(reason.contains("version-locked"));
    }

    fn amd_gpu(fsr4_capable: bool) -> GpuInfo {
        GpuInfo {
            vendor: crate::system_info::GpuVendor::Amd,
            pci_vendor_id: 0x1002,
            pci_device_id: if fsr4_capable { 0x7550 } else { 0x744C },
            model: if fsr4_capable {
                "AMD Radeon RX 9070 XT".into()
            } else {
                "AMD Radeon RX 7900 XTX".into()
            },
            driver_version: "Unknown".into(),
            vram_bytes: 0,
            recommended_runtimes: vec![],
            is_dch: true,
            identifiable: true,
            fsr4_capable,
        }
    }

    #[test]
    fn fsr4_guard_blocks_4x_members_without_rdna4() {
        let gpus = [amd_gpu(false)];
        let reason = fsr4_guard(&gpus, "fsr_upscaler", "4.1.0.0").unwrap();
        assert!(reason.contains("RDNA4"));
        assert!(fsr4_guard(&gpus, "fsr_fg", "4.0.0.0").is_some());
        assert!(fsr4_guard(&[], "fsr_upscaler", "4.1.0.0").is_some());
    }

    #[test]
    fn fsr4_guard_allows_rdna4_and_non_4x_members() {
        let rdna4 = [amd_gpu(true)];
        assert!(fsr4_guard(&rdna4, "fsr_upscaler", "4.1.0.0").is_none());
        let older = [amd_gpu(false)];
        assert!(fsr4_guard(&older, "fsr_upscaler", "3.1.4.0").is_none());
        assert!(fsr4_guard(&older, "fsr_loader", "2.2.0.0").is_none());
        assert!(fsr4_guard(&older, "xess_sr", "3.0.1.0").is_none());
    }

    #[test]
    fn backup_path_inside_root_is_restorable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Backups");
        let inside = root.join("Cyberpunk/2026/sl.dlss_g.dll");
        std::fs::create_dir_all(inside.parent().unwrap()).unwrap();
        std::fs::write(&inside, b"dll").unwrap();
        assert!(backup_path_under_root(&inside, &root));
    }

    #[test]
    fn backup_path_outside_root_is_rejected() {
        let root = std::path::Path::new("/data/DLSSync/Backups");
        assert!(!backup_path_under_root(
            std::path::Path::new("/etc/passwd"),
            root
        ));
        assert!(!backup_path_under_root(
            std::path::Path::new("/data/DLSSync/Other/sl.dlss_g.dll"),
            root
        ));
        assert!(!backup_path_under_root(
            std::path::Path::new("/data/DLSSync/BackupsEvil/x.dll"),
            root
        ));
    }
}
