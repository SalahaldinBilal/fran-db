use std::sync::Arc;

use tauri::State;

use super::{Vault, VaultStatus};
use crate::error::AppResult;

#[tauri::command]
pub async fn get_vault_status(vault: State<'_, Arc<Vault>>) -> AppResult<VaultStatus> {
    Ok(vault.status())
}

#[tauri::command]
pub async fn reset_vault(vault: State<'_, Arc<Vault>>) -> AppResult<VaultStatus> {
    vault.reset()?;
    Ok(vault.status())
}
