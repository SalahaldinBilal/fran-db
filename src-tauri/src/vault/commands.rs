use std::sync::Arc;

use tauri::State;

use super::{Vault, VaultStatus};
use crate::agent::AgentHost;
use crate::error::AppResult;

#[tauri::command]
pub async fn get_vault_status(vault: State<'_, Arc<Vault>>) -> AppResult<VaultStatus> {
    Ok(vault.status())
}

/// Agent history is encrypted with a key kept in the vault, so it goes too.
#[tauri::command]
pub async fn reset_vault(vault: State<'_, Arc<Vault>>, agent: State<'_, Arc<AgentHost>>) -> AppResult<VaultStatus> {
    vault.reset()?;
    agent.history.forget()?;
    agent.inner().apply().await;
    Ok(vault.status())
}
