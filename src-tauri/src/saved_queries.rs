use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::json_store::JsonStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub sql: String,
    pub connection_id: Option<String>,
    pub database: Option<String>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedQueriesFile {
    pub queries: Vec<SavedQuery>,
}

pub type SavedQueryStore = JsonStore<SavedQueriesFile>;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[tauri::command]
pub async fn list_saved_queries(store: State<'_, Arc<SavedQueryStore>>) -> AppResult<Vec<SavedQuery>> {
    store.read(|file| file.queries.clone())
}

#[tauri::command]
pub async fn save_query(store: State<'_, Arc<SavedQueryStore>>, mut query: SavedQuery) -> AppResult<SavedQuery> {
    if query.name.trim().is_empty() {
        return Err(AppError::general("A saved query needs a name"));
    }

    query.name = query.name.trim().to_string();
    if query.id.is_empty() {
        query.id = uuid::Uuid::new_v4().to_string();
    }
    query.updated_at = now_ms();

    let saved = query.clone();
    store.update(move |file| {
        match file.queries.iter_mut().find(|existing| existing.id == query.id) {
            Some(existing) => *existing = query,
            None => file.queries.push(query),
        }
        Ok(())
    })?;

    Ok(saved)
}

#[tauri::command]
pub async fn delete_saved_query(store: State<'_, Arc<SavedQueryStore>>, id: String) -> AppResult<()> {
    store.update(move |file| {
        file.queries.retain(|query| query.id != id);
        Ok(())
    })
}
