use dlssync_application::product_config;
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct RuntimeMode {
    pub portable: bool,
    pub release_url: String,
}

fn detect_portable(marker_filename: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(marker_filename)))
        .map(|m| m.exists())
        .unwrap_or(false)
}

pub fn devtools_allowed() -> bool {
    cfg!(debug_assertions)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn runtime_mode() -> RuntimeMode {
    let config = product_config().expect("embedded product.toml must be valid");
    RuntimeMode {
        portable: detect_portable(&config.distribution.portable.data_marker),
        release_url: config.links.releases_latest,
    }
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn open_devtools(window: tauri::WebviewWindow) {
    if !devtools_allowed() {
        return;
    }
    // The `devtools` Tauri feature is no longer compiled into release builds, so
    // the open/close/is-open methods only exist under `debug_assertions` (Tauri
    // auto-enables devtools in debug). Gating the calls keeps release builds free
    // of the devtools surface while debug builds keep the inspector.
    #[cfg(debug_assertions)]
    {
        if window.is_devtools_open() {
            window.close_devtools();
        } else {
            window.open_devtools();
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = window;
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn state_snapshot(state: State<'_, AppState>) -> dlssync_contracts::AuthoritativeSnapshot {
    if let Err(error) = refresh_persisted_state(state.inner()) {
        tracing::warn!(%error, "failed to refresh authoritative backup and History state");
    }
    authoritative_snapshot(state.authoritative_state.as_ref())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn state_watermark(state: State<'_, AppState>) -> dlssync_contracts::StateWatermark {
    authoritative_watermark(state.authoritative_state.as_ref())
}

fn authoritative_snapshot(
    coordinator: &dlssync_application::state::StateCoordinator,
) -> dlssync_contracts::AuthoritativeSnapshot {
    coordinator.snapshot()
}

fn authoritative_watermark(
    coordinator: &dlssync_application::state::StateCoordinator,
) -> dlssync_contracts::StateWatermark {
    let (emitter_id, sequence, revision) = coordinator.watermark();
    dlssync_contracts::StateWatermark {
        emitter_id,
        sequence,
        revision,
    }
}

pub(crate) fn refresh_persisted_state(state: &AppState) -> Result<(), String> {
    let backups = state.backups.read();
    let history = state.journal.read();
    let receipt = state
        .authoritative_state
        .refresh_persisted_views(backups.as_ref(), history.as_ref())?;
    if let Some(receipt) = receipt {
        if let Some(error) = receipt.delivery_error {
            tracing::warn!(%error, "authoritative persisted-state event delivery failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{authoritative_snapshot, authoritative_watermark, devtools_allowed};
    use crate::state::AppState;

    #[test]
    fn devtools_gate_tracks_debug_assertions() {
        assert_eq!(devtools_allowed(), cfg!(debug_assertions));
    }

    #[test]
    fn phase4_snapshot_and_watermark_share_emitter_and_revision() {
        let state = AppState::new();
        let snapshot = authoritative_snapshot(state.authoritative_state.as_ref());
        let watermark = authoritative_watermark(state.authoritative_state.as_ref());
        assert_eq!(snapshot.emitter_id, watermark.emitter_id);
        assert_eq!(snapshot.sequence, watermark.sequence);
        assert_eq!(snapshot.revision, watermark.revision);
    }
}
