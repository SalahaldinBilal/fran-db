use std::sync::Arc;

use tauri::State;

use super::{ConnectionProfile, ConnectionStore, ConnectionsFile, secret_id, secret_prefix};
use crate::agent::AgentHost;
use crate::error::{AppError, AppResult};
use crate::tab_state::TabStateStore;
use crate::vault::Vault;

#[tauri::command]
pub async fn list_connections(
    store: State<'_, Arc<ConnectionStore>>,
) -> AppResult<ConnectionsFile> {
    store.read(|file| file.clone())
}

#[tauri::command]
pub async fn save_connection_order(
    store: State<'_, Arc<ConnectionStore>>,
    ids: Vec<String>,
    groups: Vec<String>,
) -> AppResult<()> {
    store.update(move |file| {
        file.connections.sort_by_key(|profile| {
            ids.iter().position(|id| *id == profile.id).unwrap_or(usize::MAX)
        });
        file.group_order = groups;
        Ok(())
    })
}

#[tauri::command]
pub async fn save_connection(
    store: State<'_, Arc<ConnectionStore>>,
    agent: State<'_, Arc<AgentHost>>,
    mut profile: ConnectionProfile,
) -> AppResult<ConnectionProfile> {
    if profile.name.trim().is_empty() {
        return Err(AppError::general("A connection needs a name"));
    }

    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }

    let saved = profile.clone();
    store.update(move |file| {
        match file.connections.iter_mut().find(|existing| existing.id == profile.id) {
            Some(existing) => *existing = profile,
            None => file.connections.push(profile),
        }
        Ok(())
    })?;

    agent.close_connection_sessions(&saved.id).await;
    Ok(saved)
}

#[tauri::command]
pub async fn rename_group(
    store: State<'_, Arc<ConnectionStore>>,
    from: String,
    to: String,
) -> AppResult<()> {
    if to.trim().is_empty() {
        return Err(AppError::general("A group needs a name"));
    }

    store.update(move |file| {
        for profile in &mut file.connections {
            if profile.group.as_deref() == Some(&from) {
                profile.group = Some(to.clone());
            }
        }
        if file.group_order.contains(&to) {
            file.group_order.retain(|name| *name != from);
        } else {
            for name in &mut file.group_order {
                if *name == from {
                    *name = to.clone();
                }
            }
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn duplicate_connection(
    store: State<'_, Arc<ConnectionStore>>,
    vault: State<'_, Arc<Vault>>,
    id: String,
) -> AppResult<ConnectionProfile> {
    let mut copy = store.find(&id)?;
    copy.id = uuid::Uuid::new_v4().to_string();
    copy.name = format!("{} (copy)", copy.name);

    let saved = copy.clone();
    let source_id = id.clone();
    store.update(move |file| {
        let position = file
            .connections
            .iter()
            .position(|profile| profile.id == source_id)
            .map(|index| index + 1)
            .unwrap_or(file.connections.len());
        file.connections.insert(position, copy);
        Ok(())
    })?;

    let source_prefix = secret_prefix(&id);
    if vault.ids_with_prefix(&source_prefix).is_ok_and(|ids| !ids.is_empty()) {
        vault.copy_prefix(&source_prefix, &secret_prefix(&saved.id))?;
    }
    Ok(saved)
}

#[tauri::command]
pub async fn delete_connection(
    store: State<'_, Arc<ConnectionStore>>,
    vault: State<'_, Arc<Vault>>,
    tab_state: State<'_, Arc<TabStateStore>>,
    agent: State<'_, Arc<AgentHost>>,
    id: String,
) -> AppResult<()> {
    let target = id.clone();
    store.update(move |file| {
        file.connections.retain(|profile| profile.id != target);
        Ok(())
    })?;
    agent.close_connection_sessions(&id).await;

    let target = id.clone();
    tab_state.update(move |file| {
        file.connections.remove(&target);
        Ok(())
    })?;

    vault.remove_prefix(&secret_prefix(&id))
}

#[tauri::command]
pub async fn get_stored_secrets(
    vault: State<'_, Arc<Vault>>,
    connection_id: String,
) -> AppResult<Vec<String>> {
    let prefix = secret_prefix(&connection_id);
    Ok(vault
        .ids_with_prefix(&prefix)?
        .into_iter()
        .filter_map(|id| id.strip_prefix(&prefix).map(str::to_string))
        .collect())
}

#[tauri::command]
pub async fn set_connection_secret(
    vault: State<'_, Arc<Vault>>,
    connection_id: String,
    slot: String,
    value: String,
) -> AppResult<()> {
    let value = zeroize::Zeroizing::new(value);
    vault.set(&secret_id(&connection_id, &slot), &value)
}

#[tauri::command]
pub async fn clear_connection_secret(
    vault: State<'_, Arc<Vault>>,
    connection_id: String,
    slot: String,
) -> AppResult<()> {
    vault.remove(&secret_id(&connection_id, &slot))
}
