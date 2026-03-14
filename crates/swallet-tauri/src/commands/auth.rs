use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub fn has_data_file(state: tauri::State<'_, AppState>) -> bool {
    let service = state.service.lock().unwrap();
    service.has_data_file()
}

#[tauri::command]
pub fn create_store(state: tauri::State<'_, AppState>, password: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.create_new_store(password.as_bytes())?;
    Ok(())
}

#[tauri::command]
pub fn unlock(state: tauri::State<'_, AppState>, password: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.unlock(password.as_bytes())?;
    // migrate if needed
    if let Some(ref mut store) = service.store {
        store.migrate();
    }
    let _ = service.save_store();
    Ok(())
}

#[tauri::command]
pub fn verify_password(state: tauri::State<'_, AppState>, password: String) -> bool {
    let service = state.service.lock().unwrap();
    service.verify_password(password.as_bytes())
}

#[tauri::command]
pub fn is_unlocked(state: tauri::State<'_, AppState>) -> bool {
    let service = state.service.lock().unwrap();
    service.store.is_some()
}
