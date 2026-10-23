use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::AppResult;
use crate::json_store::JsonStore;

/// Open tabs per connection. The shape belongs to the frontend, Rust only stores it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TabStateFile {
    pub connections: HashMap<String, serde_json::Value>,
}

pub type TabStateStore = JsonStore<TabStateFile>;

#[tauri::command]
pub async fn get_tab_state(
    store: State<'_, Arc<TabStateStore>>,
    connection_id: String,
) -> AppResult<Option<serde_json::Value>> {
    store.read(|file| file.connections.get(&connection_id).cloned())
}

#[tauri::command]
pub async fn set_tab_state(
    store: State<'_, Arc<TabStateStore>>,
    connection_id: String,
    state: serde_json::Value,
) -> AppResult<()> {
    store.update(move |file| {
        file.connections.insert(connection_id, state);
        Ok(())
    })
}
