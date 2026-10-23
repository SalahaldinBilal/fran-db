use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::AppResult;
use crate::json_store::JsonStore;

/// App preferences. The shape belongs to the frontend, Rust only stores it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsFile {
    pub settings: Option<serde_json::Value>,
}

pub type SettingsStore = JsonStore<SettingsFile>;

#[tauri::command]
pub async fn get_settings(store: State<'_, Arc<SettingsStore>>) -> AppResult<Option<serde_json::Value>> {
    store.read(|file| file.settings.clone())
}

#[tauri::command]
pub async fn set_settings(store: State<'_, Arc<SettingsStore>>, settings: serde_json::Value) -> AppResult<()> {
    store.update(move |file| {
        file.settings = Some(settings);
        Ok(())
    })
}
