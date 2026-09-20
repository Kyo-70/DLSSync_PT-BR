use tauri::AppHandle;

#[tauri::command]
#[cfg_attr(feature = "bindings", specta::specta)]
pub fn tray_set_pending(handle: AppHandle, count: u32) {
    crate::tray::update_pending_count(&handle, count);
}
