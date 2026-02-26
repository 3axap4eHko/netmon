use std::sync::Arc;
use tauri::State;

use crate::auth::{AuthManager, AuthState};
use crate::sync::{SyncEngine, SyncStatus};

type AuthState_ = Arc<AuthManager>;
type SyncState = Arc<SyncEngine>;

#[tauri::command]
pub fn get_auth_state(auth: State<'_, AuthState_>) -> Result<AuthState, String> {
    Ok(auth.get_auth_state())
}

#[tauri::command]
pub fn start_oauth(auth: State<'_, AuthState_>, provider: String) -> Result<String, String> {
    auth.start_oauth(&provider)
}

#[tauri::command]
pub fn login_email(
    auth: State<'_, AuthState_>,
    sync: State<'_, SyncState>,
    email: String,
    password: String,
) -> Result<AuthState, String> {
    let state = auth.login_email(&email, &password)?;
    if state.authenticated {
        sync.start();
    }
    Ok(state)
}

#[tauri::command]
pub fn register_email(
    auth: State<'_, AuthState_>,
    sync: State<'_, SyncState>,
    email: String,
    password: String,
) -> Result<AuthState, String> {
    let state = auth.register_email(&email, &password)?;
    if state.authenticated {
        sync.start();
    }
    Ok(state)
}

#[tauri::command]
pub fn logout(
    auth: State<'_, AuthState_>,
    sync: State<'_, SyncState>,
) -> Result<(), String> {
    sync.stop();
    auth.logout()
}

#[tauri::command]
pub fn get_sync_status(sync: State<'_, SyncState>) -> Result<SyncStatus, String> {
    Ok(sync.status())
}

#[tauri::command]
pub fn get_account_info(auth: State<'_, AuthState_>) -> Result<AuthState, String> {
    Ok(auth.get_auth_state())
}
