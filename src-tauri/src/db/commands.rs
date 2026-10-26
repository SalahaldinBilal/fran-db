use std::{collections::HashMap, sync::Arc};

use tauri::State;

use super::{
    DatabaseKind,
    session::ContextId,
    sessions::{OpenSession, SessionInfo, SessionManager},
    split_statements,
    types::{
        BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, DatabaseInfo, DesignOptions, ObjectInfo, ObjectKind,
        ObjectStructure, RowChange, ServerInfo, SqlTemplate, StatementOutcome, StatementRange, TableDesign,
    },
};
use crate::connections::{ConnectionProfile, ConnectionStore, resolve_secrets};
use crate::error::AppResult;
use crate::ssh::known_hosts::KnownHosts;
use crate::vault::Vault;

#[tauri::command]
pub async fn open_session(
    store: State<'_, Arc<ConnectionStore>>,
    vault: State<'_, Arc<Vault>>,
    known_hosts: State<'_, Arc<KnownHosts>>,
    sessions: State<'_, SessionManager>,
    connection_id: String,
    secrets: HashMap<String, String>,
) -> AppResult<SessionInfo> {
    let profile = store.find(&connection_id)?;
    let bag = resolve_secrets(
        &vault,
        Some(&profile.id),
        secrets,
        &profile.driver.required_secrets(),
        &profile.driver.optional_secrets(),
    )?;

    let session = profile.driver.connect(&bag, &known_hosts).await?;
    sessions.insert(OpenSession {
        info: SessionInfo {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: profile.id.clone(),
            kind: profile.driver.kind(),
            server: session.server_info(),
            catalog: session.catalog(),
        },
        session,
    })
}

#[tauri::command]
pub async fn list_catalogs(sessions: State<'_, SessionManager>, session_id: String) -> AppResult<Vec<String>> {
    sessions.get(&session_id)?.session.list_catalogs().await
}

#[tauri::command]
pub async fn switch_catalog(
    sessions: State<'_, SessionManager>,
    session_id: String,
    catalog: String,
) -> AppResult<SessionInfo> {
    let open = sessions.get(&session_id)?;
    open.session.switch_catalog(&catalog).await?;
    Ok(SessionInfo {
        catalog: open.session.catalog(),
        ..open.info.clone()
    })
}

/// Works on unsaved profiles too. Stored secrets are used when the profile already has an id.
#[tauri::command]
pub async fn test_connection(
    vault: State<'_, Arc<Vault>>,
    known_hosts: State<'_, Arc<KnownHosts>>,
    profile: ConnectionProfile,
    secrets: HashMap<String, String>,
) -> AppResult<ServerInfo> {
    let connection_id = (!profile.id.is_empty()).then_some(profile.id.as_str());
    let bag = resolve_secrets(
        &vault,
        connection_id,
        secrets,
        &profile.driver.required_secrets(),
        &profile.driver.optional_secrets(),
    )?;

    let session = profile.driver.connect(&bag, &known_hosts).await?;
    let info = session.server_info();
    session.close().await;
    Ok(info)
}

#[tauri::command]
pub async fn close_session(sessions: State<'_, SessionManager>, session_id: String) -> AppResult<()> {
    if let Some(open) = sessions.remove(&session_id) {
        open.session.close().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn list_databases(
    sessions: State<'_, SessionManager>,
    session_id: String,
) -> AppResult<Vec<DatabaseInfo>> {
    sessions.get(&session_id)?.session.list_databases().await
}

#[tauri::command]
pub async fn list_objects(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
) -> AppResult<Vec<ObjectInfo>> {
    sessions.get(&session_id)?.session.list_objects(&database).await
}

#[tauri::command]
pub async fn get_object_structure(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    name: String,
    kind: ObjectKind,
    signature: Option<String>,
) -> AppResult<ObjectStructure> {
    sessions
        .get(&session_id)?
        .session
        .object_structure(&database, &name, kind, signature.as_deref())
        .await
}

#[tauri::command]
pub async fn get_column_choices(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    table: String,
    column: String,
) -> AppResult<Option<ColumnChoices>> {
    sessions
        .get(&session_id)?
        .session
        .column_choices(&database, &table, &column)
        .await
}

#[tauri::command]
pub async fn get_primary_key(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    table: String,
) -> AppResult<Vec<String>> {
    sessions.get(&session_id)?.session.primary_key(&database, &table).await
}

#[tauri::command]
pub async fn open_query_context(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: Option<String>,
) -> AppResult<ContextId> {
    sessions.get(&session_id)?.session.open_context(database).await
}

#[tauri::command]
pub async fn close_query_context(
    sessions: State<'_, SessionManager>,
    session_id: String,
    context_id: ContextId,
) -> AppResult<()> {
    if let Ok(open) = sessions.get(&session_id) {
        open.session.close_context(context_id).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn execute_query(
    sessions: State<'_, SessionManager>,
    session_id: String,
    context_id: ContextId,
    sql: String,
    row_limit: usize,
) -> AppResult<Vec<StatementOutcome>> {
    sessions
        .get(&session_id)?
        .session
        .execute(context_id, &sql, row_limit)
        .await
}

#[tauri::command]
pub async fn cancel_query(
    sessions: State<'_, SessionManager>,
    session_id: String,
    context_id: ContextId,
) -> AppResult<()> {
    sessions.get(&session_id)?.session.cancel(context_id).await
}

#[tauri::command]
pub async fn split_sql(kind: DatabaseKind, sql: String) -> AppResult<Vec<StatementRange>> {
    Ok(split_statements(kind, &sql))
}

#[tauri::command]
pub async fn list_columns(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
) -> AppResult<Vec<CatalogColumn>> {
    sessions.get(&session_id)?.session.list_columns(&database).await
}

#[tauri::command]
pub async fn get_object_sql(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    name: String,
    kind: ObjectKind,
    signature: Option<String>,
    template: SqlTemplate,
) -> AppResult<String> {
    sessions
        .get(&session_id)?
        .session
        .object_sql(&database, &name, kind, signature.as_deref(), template)
        .await
}

#[tauri::command]
pub async fn get_database_sql(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    template: SqlTemplate,
) -> AppResult<String> {
    sessions.get(&session_id)?.session.database_sql(&database, template).await
}

#[tauri::command]
pub async fn preview_row_changes(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    table: String,
    columns: Vec<ColumnMeta>,
    changes: Vec<RowChange>,
) -> AppResult<Vec<String>> {
    sessions
        .get(&session_id)?
        .session
        .row_changes_sql(&database, &table, &columns, &changes)
}

#[tauri::command]
pub async fn execute_batch(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: Option<String>,
    statements: Vec<String>,
    transactional: bool,
) -> AppResult<BatchOutcome> {
    sessions
        .get(&session_id)?
        .session
        .execute_batch(database.as_deref(), &statements, transactional)
        .await
}

#[tauri::command]
pub async fn get_table_design(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    table: String,
) -> AppResult<TableDesign> {
    sessions.get(&session_id)?.session.table_design(&database, &table).await
}

#[tauri::command]
pub async fn get_design_options(
    sessions: State<'_, SessionManager>,
    session_id: String,
) -> AppResult<DesignOptions> {
    sessions.get(&session_id)?.session.design_options().await
}

#[tauri::command]
pub async fn preview_table_design(
    sessions: State<'_, SessionManager>,
    session_id: String,
    database: String,
    original: Option<TableDesign>,
    design: TableDesign,
) -> AppResult<Vec<String>> {
    sessions
        .get(&session_id)?
        .session
        .table_design_sql(&database, original.as_ref(), &design)
}
