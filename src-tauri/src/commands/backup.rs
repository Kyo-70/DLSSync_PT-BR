use crate::error::{AppError, AppResult};
use crate::paths::PathGuard;
use crate::state::AppState;
use backup_store::{BackupEntry, DeleteOutcome};
use tauri::State;

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn list_backups(state: State<'_, AppState>) -> AppResult<Vec<BackupEntry>> {
    let guard = state.backups.read();
    let store = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("backup store not initialized".into()))?;
    Ok(store.list()?)
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn restore_backup(state: State<'_, AppState>, backup_id: String) -> AppResult<()> {
    let (entry, root_dir) = {
        let guard = state.backups.read();
        let store = guard
            .as_ref()
            .ok_or_else(|| AppError::Other("backup store not initialized".into()))?;
        (store.get(&backup_id)?, store.root_dir.clone())
    };

    PathGuard::assert_under_root(&entry.backup_path, &root_dir)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    PathGuard::assert_dll_ext(&entry.original_path)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    PathGuard::deny_system_dir(&entry.original_path)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    PathGuard::assert_not_symlink(&entry.original_path)
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let store = state
        .backups
        .read()
        .as_ref()
        .cloned()
        .ok_or_else(|| AppError::Other("backup store not initialized".into()))?;
    tokio::task::spawn_blocking(move || {
        dlssync_application::transaction::restore_entries(
            &store,
            &[entry],
            dlssync_contracts::OperationActor::Gui,
        )
    })
    .await
    .map_err(|error| AppError::Other(error.to_string()))?
    .map_err(|error| AppError::Other(error.to_string()))?;

    if let Err(error) = crate::commands::runtime::refresh_persisted_state(state.inner()) {
        tracing::warn!(%error, "persisted-state refresh after restore failed");
    }

    Ok(())
}

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub async fn delete_backup(
    state: State<'_, AppState>,
    backup_id: String,
) -> AppResult<DeleteOutcome> {
    let guard = state.backups.read();
    let store = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("backup store not initialized".into()))?;
    let outcome = store.delete(&backup_id, true)?;
    drop(guard);
    if let Err(error) = crate::commands::runtime::refresh_persisted_state(state.inner()) {
        tracing::warn!(%error, "persisted-state refresh after backup deletion failed");
    }
    Ok(outcome)
}
