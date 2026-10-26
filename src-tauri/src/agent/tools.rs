use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::access::{AgentAccess, AgentMode};
use super::approvals::{KeyValue, PreviewCell, PreviewRow, RowAction, RowPreview};
use super::args::{
    AlterTableArgs, ColumnValuesArgs, CountRowsArgs, CreateTableArgs, DefinitionArgs, DeleteRowsArgs, DescribeTableArgs,
    DropObjectArgs, FilterArg, InsertRowsArgs, ListObjectsArgs, OpenSessionArgs, ReadRowsArgs, RelationshipsArgs,
    RowUpdateArg, RunQueryArgs, SearchSchemaArgs, SessionArgs, SqlArgs, TableArgs, UpdateRowsArgs,
};
use super::call::CallLog;
use super::format;
use super::host::{AgentHost, CloseReason, Live, refused};
use super::sessions::AgentSession;
use crate::db::classify::{ClassifiedStatement, StatementAccess};
use crate::db::session::DatabaseSession;
use crate::db::types::{
    CellEdit, ColumnMeta, FilterOp, FilterValue, ObjectKind, ReadQuery, ResultSet, RowChange, RowFilter, RowOrder,
    SqlTemplate, TableDesign,
};
use crate::error::{AppError, AppResult};

const DEFAULT_ROWS: u64 = 100;
const MAX_ROWS: u64 = 1000;
const DEFAULT_VALUES: u64 = 50;
const SEARCH_LIMIT: usize = 200;
const PREVIEW_ROWS: usize = 50;
const PREVIEW_CELL_LIMIT: usize = 300;

type CurrentRow = Vec<(String, Option<String>)>;

fn preview_text(value: Option<String>) -> Option<String> {
    value.map(|text| match text.char_indices().nth(PREVIEW_CELL_LIMIT) {
        Some((cut, _)) => format!("{}...", &text[..cut]),
        None => text,
    })
}

fn key_values(key: &Map<String, Value>) -> Vec<KeyValue> {
    key.iter()
        .map(|(column, value)| KeyValue { column: column.clone(), value: preview_text(cell_text(value)) })
        .collect()
}

fn filter_value(value: &Value) -> AppResult<FilterValue> {
    match value {
        Value::Null => Ok(FilterValue::Null),
        Value::Bool(value) => Ok(FilterValue::Bool(*value)),
        Value::Number(value) => Ok(FilterValue::Number(value.to_string())),
        Value::String(value) => Ok(FilterValue::Text(value.clone())),
        _ => Err(AppError::general("Filter values must be text, numbers, true/false or null")),
    }
}

fn filters(given: Option<Vec<FilterArg>>) -> AppResult<Vec<RowFilter>> {
    given
        .unwrap_or_default()
        .into_iter()
        .map(|filter| {
            let values = filter
                .value
                .iter()
                .chain(filter.values.iter().flatten())
                .map(filter_value)
                .collect::<AppResult<Vec<_>>>()?;
            Ok(RowFilter { column: filter.column, op: filter.op.into(), values })
        })
        .collect()
}

/// A cell value as the row editor writes it. Booleans become 1 and 0, which every server reads as true and false.
fn cell_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(if *value { "1" } else { "0" }.to_string()),
        Value::String(value) => Some(value.clone()),
        other => Some(other.to_string()),
    }
}

fn column_index(columns: &[ColumnMeta], name: &str, table: &str) -> AppResult<usize> {
    columns
        .iter()
        .position(|column| column.name == name)
        .ok_or_else(|| AppError::general(format!("{table} has no column {name}")))
}

fn edits(columns: &[ColumnMeta], values: &Map<String, Value>, table: &str) -> AppResult<Vec<CellEdit>> {
    values
        .iter()
        .map(|(name, value)| Ok(CellEdit { column: column_index(columns, name, table)?, value: cell_text(value) }))
        .collect()
}

/// The full row key at the positions `row_changes_sql` reads, with every primary key column given.
fn key_row(columns: &[ColumnMeta], key: &Map<String, Value>, table: &str) -> AppResult<Vec<Option<String>>> {
    let mut row = vec![None; columns.len()];
    for (index, column) in columns.iter().enumerate().filter(|(_, column)| column.primary_key) {
        let value = key.get(&column.name).and_then(cell_text).ok_or_else(|| {
            AppError::general(format!("The key for {table} needs {}, part of its primary key", column.name))
        })?;
        row[index] = Some(value);
    }
    if let Some(extra) = key.keys().find(|name| !columns.iter().any(|column| column.primary_key && &column.name == *name)) {
        return Err(AppError::general(format!("{extra} isn't part of the primary key of {table}")));
    }
    Ok(row)
}

fn first_result(outcomes: Vec<crate::db::types::StatementOutcome>) -> AppResult<ResultSet> {
    let outcome = outcomes.into_iter().next().ok_or_else(|| AppError::general("The server returned nothing"))?;
    if let Some(error) = outcome.error {
        return Err(error);
    }
    outcome.result_sets.into_iter().next().ok_or_else(|| AppError::general("The server returned no rows"))
}

fn refused_statements(statements: &[ClassifiedStatement]) -> Vec<String> {
    statements
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| match &statement.access {
            StatementAccess::Read => None,
            StatementAccess::Change { reason } => Some(format!("Statement {}: {reason}", index + 1)),
        })
        .collect()
}

async fn choose_database(
    session: &dyn DatabaseSession,
    access: &AgentAccess,
    wanted: Option<String>,
    log: &CallLog,
) -> AppResult<Option<String>> {
    let shared = || access.databases.join(", ");
    match session.catalog() {
        Some(current) => {
            let target = wanted.unwrap_or_else(|| current.clone());
            if !access.allows(&target) {
                return Err(refused(log, format!("{target} isn't shared with agents. Pass database, one of: {}", shared())));
            }
            if target != current {
                session.switch_catalog(&target).await?;
            }
            Ok(None)
        }
        None => match wanted {
            Some(database) if !access.allows(&database) => {
                Err(refused(log, format!("{database} isn't shared with agents. Shared: {}", shared())))
            }
            wanted => Ok(wanted),
        },
    }
}

impl AgentHost {
    async fn visible_databases(&self, session: &AgentSession, access: &AgentAccess) -> AppResult<Vec<String>> {
        Ok(session
            .session
            .list_databases()
            .await?
            .into_iter()
            .filter(|database| !database.system && (session.catalog.is_some() || access.allows(&database.name)))
            .map(|database| database.name)
            .collect())
    }

    /// Column details of a table, read from an empty SELECT so they match what row edits expect.
    async fn table_columns(&self, live: &Live, database: &str, table: &str, log: &CallLog) -> AppResult<Vec<ColumnMeta>> {
        let probe = ReadQuery::Rows {
            table: table.to_string(),
            columns: Vec::new(),
            filters: Vec::new(),
            order: Vec::new(),
            limit: 0,
            offset: 0,
        };
        let sql = live.session.session.read_sql(database, &probe)?;
        let set = first_result(self.run_read(live, Some(database), &sql, 1, log).await?)?;
        Ok(set.columns)
    }

    // Kept out of the call's statements, since the agent didn't ask for this read.
    async fn current_row(
        &self,
        live: &Live,
        database: &str,
        table: &str,
        key: &Map<String, Value>,
        columns: Vec<String>,
        log: &CallLog,
    ) -> AppResult<Option<CurrentRow>> {
        let filters = key
            .iter()
            .map(|(column, value)| {
                Ok(RowFilter { column: column.clone(), op: FilterOp::Eq, values: vec![filter_value(value)?] })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let query = ReadQuery::Rows { table: table.to_string(), columns, filters, order: Vec::new(), limit: 1, offset: 0 };
        let sql = live.session.session.read_sql(database, &query)?;
        let set = first_result(self.run_read(live, Some(database), &sql, 1, log).await?)?;
        Ok(set.rows.into_iter().next().map(|row| {
            set.columns.iter().map(|column| column.name.clone()).zip(row).collect()
        }))
    }

    async fn update_preview(
        &self,
        live: &Live,
        database: &str,
        table: &str,
        updates: &[RowUpdateArg],
        log: &CallLog,
    ) -> RowPreview {
        let mut rows = Vec::new();
        for row in updates.iter().take(PREVIEW_ROWS) {
            let mut columns: Vec<String> = row.key.keys().cloned().collect();
            columns.extend(row.values.keys().filter(|name| !row.key.contains_key(*name)).cloned());
            let current = self.current_row(live, database, table, &row.key, columns, log).await.ok();
            let cells = row
                .values
                .iter()
                .map(|(column, value)| PreviewCell {
                    column: column.clone(),
                    before: preview_text(
                        current
                            .as_ref()
                            .and_then(Option::as_ref)
                            .and_then(|found| found.iter().find(|(name, _)| name == column))
                            .and_then(|(_, value)| value.clone()),
                    ),
                    after: preview_text(cell_text(value)),
                })
                .collect();
            rows.push(PreviewRow {
                action: RowAction::Update,
                key: key_values(&row.key),
                cells,
                found: current.map(|found| found.is_some()),
            });
        }
        RowPreview { table: table.to_string(), rows, more: updates.len().saturating_sub(PREVIEW_ROWS) }
    }

    async fn delete_preview(
        &self,
        live: &Live,
        database: &str,
        table: &str,
        keys: &[Map<String, Value>],
        log: &CallLog,
    ) -> RowPreview {
        let mut rows = Vec::new();
        for key in keys.iter().take(PREVIEW_ROWS) {
            let current = self.current_row(live, database, table, key, Vec::new(), log).await.ok();
            let cells: Vec<PreviewCell> = current
                .as_ref()
                .and_then(Option::as_ref)
                .map(|found| {
                    found
                        .iter()
                        .map(|(column, value)| PreviewCell {
                            column: column.clone(),
                            before: preview_text(value.clone()),
                            after: None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            rows.push(PreviewRow {
                action: RowAction::Delete,
                key: key_values(key),
                cells,
                found: current.map(|found| found.is_some()),
            });
        }
        RowPreview { table: table.to_string(), rows, more: keys.len().saturating_sub(PREVIEW_ROWS) }
    }

    async fn read_query(&self, live: &Live, database: &str, query: &ReadQuery, row_limit: usize, log: &CallLog) -> AppResult<ResultSet> {
        let sql = live.session.session.read_sql(database, query)?;
        log.statements(std::slice::from_ref(&sql));
        first_result(self.run_read(live, Some(database), &sql, row_limit, log).await?)
    }

    pub fn list_connections(&self) -> AppResult<Value> {
        let config = self.config()?;
        let profiles = self.store.read(|file| file.connections.clone())?;
        let shared: Vec<Value> = profiles
            .iter()
            .filter_map(|profile| {
                let mode = profile.agent_access.effective_mode(&config);
                (mode != AgentMode::Off).then(|| {
                    json!({
                        "id": profile.id,
                        "name": profile.name,
                        "kind": profile.driver.kind(),
                        "mode": mode,
                        "databases": profile.agent_access.databases,
                    })
                })
            })
            .collect();
        Ok(json!({ "connections": shared }))
    }

    pub async fn open_session(&self, args: OpenSessionArgs, log: &CallLog) -> AppResult<Value> {
        let config = self.config()?;
        let profile = self.find_profile(&args.connection)?;
        let access = profile.agent_access.clone();
        let mode = access.effective_mode(&config);
        if mode == AgentMode::Off {
            return Err(refused(
                log,
                format!("{} isn't shared with agents. Turn on agent access for it in Fran DB.", profile.name),
            ));
        }

        let session = self.connect(&profile).await?;
        let default_database = match choose_database(session.as_ref(), &access, args.database, log).await {
            Ok(database) => database,
            Err(error) => {
                session.close().await;
                return Err(error);
            }
        };

        let live = self.sessions.insert(AgentSession::new(
            profile.id.clone(),
            profile.name.clone(),
            profile.driver.kind(),
            default_database.clone(),
            log.agent.clone(),
            session,
        ));
        log.session(&live);
        self.emit_sessions_changed();

        let databases = self.visible_databases(&live, &access).await.unwrap_or_default();
        let others: Vec<String> = match &live.catalog {
            Some(current) => live
                .session
                .list_catalogs()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|name| name != current && access.allows(name))
                .collect(),
            None => Vec::new(),
        };
        let note = match mode {
            AgentMode::Ask => "Changes wait for the user to approve them in Fran DB.",
            _ => "This connection is read only for agents.",
        };

        let mut reply = json!({
            "session": live.id,
            "connection": live.connection_name,
            "kind": live.kind,
            "server": format!("{} {}", live.server.product, live.server.version).trim(),
            "mode": mode,
            "database": live.catalog,
            "defaultDatabase": default_database,
            "otherDatabases": others,
            "note": note,
        });
        reply[if live.catalog.is_some() { "schemas" } else { "databases" }] = json!(databases);
        Ok(reply)
    }

    pub async fn close_agent_session(&self, args: SessionArgs, log: &CallLog) -> AppResult<Value> {
        self.live(&args.session, log).await?;
        self.close_session(&args.session, CloseReason::Agent).await;
        Ok(json!({ "closed": true }))
    }

    pub async fn list_databases(&self, args: SessionArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let session = &live.session.session;
        let sizes = session.database_sizes().await.unwrap_or_default();
        let databases: Vec<Value> = session
            .list_databases()
            .await?
            .into_iter()
            .filter(|database| live.session.catalog.is_some() || live.access.allows(&database.name))
            .map(|database| {
                let size = sizes.iter().find(|size| size.name == database.name).map(|size| size.size_bytes);
                json!({ "name": database.name, "system": database.system, "sizeBytes": size })
            })
            .collect();
        Ok(json!({ "databases": databases }))
    }

    pub async fn list_objects(&self, args: ListObjectsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let kind = args.kind.map(ObjectKind::from);
        let needle = args.name_contains.map(|text| text.to_lowercase());
        let objects: Vec<_> = live
            .session
            .session
            .list_objects(&database)
            .await?
            .into_iter()
            .filter(|object| kind.is_none_or(|kind| object.kind == kind))
            .filter(|object| needle.as_ref().is_none_or(|needle| object.name.to_lowercase().contains(needle)))
            .collect();
        Ok(json!({ "database": database, "objects": objects }))
    }

    pub async fn describe_table(&self, args: DescribeTableArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let kind = args.kind.map_or(ObjectKind::Table, ObjectKind::from);
        let session = &live.session.session;
        let structure = session.object_structure(&database, &args.table, kind, None).await?;

        let referenced_by: Vec<Value> = if kind == ObjectKind::Table {
            session
                .foreign_keys(&database)
                .await?
                .into_iter()
                .filter(|key| key.key.referenced_table == args.table && key.key.referenced_database == database)
                .map(|key| {
                    json!({
                        "name": key.key.name,
                        "database": key.database,
                        "table": key.table,
                        "columns": key.key.columns,
                        "referencedColumns": key.key.referenced_columns,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let primary_key: Vec<&str> = structure
            .columns
            .iter()
            .filter(|column| column.primary_key)
            .map(|column| column.name.as_str())
            .collect();

        Ok(json!({
            "database": database,
            "table": args.table,
            "columns": structure.columns,
            "primaryKey": primary_key,
            "indexes": structure.indexes,
            "foreignKeys": structure.foreign_keys,
            "referencedBy": referenced_by,
        }))
    }

    pub async fn get_definition(&self, args: DefinitionArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let sql = live
            .session
            .session
            .object_sql(&database, &args.name, args.kind.into(), args.signature.as_deref(), SqlTemplate::Create)
            .await?;
        Ok(json!({ "sql": sql }))
    }

    pub async fn search_schema(&self, args: SearchSchemaArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let databases = match args.database {
            Some(database) => vec![self.database(&live, Some(database), log)?],
            None => self.visible_databases(&live.session, &live.access).await?,
        };
        let needle = args.pattern.to_lowercase();

        let mut tables = BTreeSet::new();
        let mut columns = Vec::new();
        let mut truncated = false;
        for database in &databases {
            for column in live.session.session.list_columns(database).await? {
                if column.table.to_lowercase().contains(&needle) {
                    tables.insert((database.clone(), column.table.clone()));
                }
                if column.name.to_lowercase().contains(&needle) {
                    if columns.len() == SEARCH_LIMIT {
                        truncated = true;
                        continue;
                    }
                    columns.push(json!({
                        "database": database,
                        "table": column.table,
                        "column": column.name,
                        "type": column.data_type,
                    }));
                }
            }
        }
        truncated |= tables.len() > SEARCH_LIMIT;
        let tables: Vec<Value> = tables
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(database, table)| json!({ "database": database, "table": table }))
            .collect();
        Ok(json!({ "tables": tables, "columns": columns, "truncated": truncated }))
    }

    pub async fn get_relationships(&self, args: RelationshipsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let keys: Vec<_> = live
            .session
            .session
            .foreign_keys(&database)
            .await?
            .into_iter()
            .filter(|key| {
                args.table.as_ref().is_none_or(|table| {
                    (key.table == *table && key.database == database)
                        || (key.key.referenced_table == *table && key.key.referenced_database == database)
                })
            })
            .collect();
        Ok(json!({ "foreignKeys": keys }))
    }

    pub async fn read_rows(&self, args: ReadRowsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let limit = args.limit.unwrap_or(DEFAULT_ROWS).clamp(1, MAX_ROWS);
        let query = ReadQuery::Rows {
            table: args.table,
            columns: args.columns.unwrap_or_default(),
            filters: filters(args.filters)?,
            order: args
                .order_by
                .unwrap_or_default()
                .into_iter()
                .map(|order| RowOrder { column: order.column, descending: order.descending })
                .collect(),
            limit: limit + 1,
            offset: args.offset.unwrap_or(0),
        };
        let set = self.read_query(&live, &database, &query, limit as usize, log).await?;
        Ok(format::result_set(&set))
    }

    pub async fn count_rows(&self, args: CountRowsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let query = ReadQuery::Count { table: args.table, filters: filters(args.filters)? };
        let set = self.read_query(&live, &database, &query, 1, log).await?;
        let count = set
            .rows
            .first()
            .and_then(|row| row.first().cloned().flatten())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| AppError::general("The server didn't return a count"))?;
        Ok(json!({ "count": count }))
    }

    pub async fn column_values(&self, args: ColumnValuesArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let limit = args.limit.unwrap_or(DEFAULT_VALUES).clamp(1, MAX_ROWS);
        let query = ReadQuery::ColumnValues {
            table: args.table,
            column: args.column.clone(),
            filters: filters(args.filters)?,
            limit: limit + 1,
        };
        let set = self.read_query(&live, &database, &query, limit as usize, log).await?;
        let values: Vec<Value> = set
            .rows
            .iter()
            .map(|row| {
                let count = row.get(1).cloned().flatten().and_then(|count| count.parse::<u64>().ok());
                json!({ "value": row.first().cloned().flatten(), "count": count })
            })
            .collect();
        Ok(json!({ "column": args.column, "values": values, "truncated": set.truncated }))
    }

    pub async fn run_query(&self, args: RunQueryArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_raw_sql(&live, log)?;
        let database = self.optional_database(&live, args.database, log)?;

        let statements = live.session.session.classify(&args.sql);
        if statements.is_empty() {
            return Err(AppError::general("There's no statement to run"));
        }
        log.statements(&statements.iter().map(|statement| statement.text.clone()).collect::<Vec<_>>());
        let refusals = refused_statements(&statements);
        if !refusals.is_empty() {
            let hint = if live.mode == AgentMode::Ask { " Use execute_sql for changes." } else { "" };
            return Err(refused(log, format!("run_query only runs reads. {}.{hint}", refusals.join(". "))));
        }

        let row_limit = args.max_rows.unwrap_or(DEFAULT_ROWS as usize).clamp(1, MAX_ROWS as usize);
        let outcomes = self.run_read(&live, database.as_deref(), &args.sql, row_limit, log).await?;
        if let Some((index, error)) = outcomes
            .iter()
            .enumerate()
            .find_map(|(index, outcome)| outcome.error.as_ref().map(|error| (index, error)))
        {
            return Err(AppError::general(format!("Statement {} failed: {}", index + 1, error.message)));
        }
        Ok(json!({ "statements": outcomes.iter().map(format::statement).collect::<Vec<_>>() }))
    }

    pub async fn explain_query(&self, args: SqlArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_raw_sql(&live, log)?;
        let database = self.optional_database(&live, args.database, log)?;
        let session = &live.session.session;

        let statement = match session.classify(&args.sql).as_slice() {
            [statement] => statement.text.clone(),
            _ => return Err(AppError::general("Pass exactly one statement to explain")),
        };
        let explain = session.explain_sql(&statement);
        log.statements(std::slice::from_ref(&explain));
        let classified = session.classify(&explain);
        let refusals = refused_statements(&classified);
        if !refusals.is_empty() || classified.len() != 1 {
            return Err(refused(log, format!("That can't be explained safely. {}", refusals.join(". "))));
        }

        let outcomes = self.run_read(&live, database.as_deref(), &explain, MAX_ROWS as usize, log).await?;
        Ok(json!({ "plan": format::result_set(&first_result(outcomes)?) }))
    }

    pub async fn insert_rows(&self, args: InsertRowsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let columns = self.table_columns(&live, &database, &args.table, log).await?;
        let changes = args
            .rows
            .iter()
            .map(|row| Ok(RowChange::Insert { values: edits(&columns, row, &args.table)? }))
            .collect::<AppResult<Vec<_>>>()?;
        let statements = live.session.session.row_changes_sql(&database, &args.table, &columns, &changes)?;
        let preview = RowPreview {
            table: args.table.clone(),
            rows: args
                .rows
                .iter()
                .take(PREVIEW_ROWS)
                .map(|row| PreviewRow {
                    action: RowAction::Insert,
                    key: Vec::new(),
                    cells: row
                        .iter()
                        .map(|(column, value)| PreviewCell {
                            column: column.clone(),
                            before: None,
                            after: preview_text(cell_text(value)),
                        })
                        .collect(),
                    found: None,
                })
                .collect(),
            more: args.rows.len().saturating_sub(PREVIEW_ROWS),
        };
        self.run_changes(&live, Some(&database), "insert_rows", statements, Some(preview), log).await
    }

    pub async fn update_rows(&self, args: UpdateRowsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let columns = self.table_columns(&live, &database, &args.table, log).await?;
        let changes = args
            .rows
            .iter()
            .map(|row| {
                Ok(RowChange::Update {
                    key: key_row(&columns, &row.key, &args.table)?,
                    values: edits(&columns, &row.values, &args.table)?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let statements = live.session.session.row_changes_sql(&database, &args.table, &columns, &changes)?;
        let preview = self.update_preview(&live, &database, &args.table, &args.rows, log).await;
        self.run_changes(&live, Some(&database), "update_rows", statements, Some(preview), log).await
    }

    pub async fn delete_rows(&self, args: DeleteRowsArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let columns = self.table_columns(&live, &database, &args.table, log).await?;
        let changes = args
            .keys
            .iter()
            .map(|key| Ok(RowChange::Delete { key: key_row(&columns, key, &args.table)? }))
            .collect::<AppResult<Vec<_>>>()?;
        let statements = live.session.session.row_changes_sql(&database, &args.table, &columns, &changes)?;
        let preview = self.delete_preview(&live, &database, &args.table, &args.keys, log).await;
        self.run_changes(&live, Some(&database), "delete_rows", statements, Some(preview), log).await
    }

    pub async fn get_table_design(&self, args: TableArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        let database = self.database(&live, args.database, log)?;
        let design = live.session.session.table_design(&database, &args.table).await?;
        Ok(serde_json::to_value(design)?)
    }

    pub async fn create_table(&self, args: CreateTableArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let design = parse_design(args.design)?;
        let statements = live.session.session.table_design_sql(&database, None, &design)?;
        self.run_changes(&live, Some(&database), "create_table", statements, None, log).await
    }

    pub async fn alter_table(&self, args: AlterTableArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let original = live.session.session.table_design(&database, &args.table).await?;
        let edited = parse_design(args.design)?;
        let statements = live.session.session.table_design_sql(&database, Some(&original), &edited)?;
        if statements.is_empty() {
            return Err(AppError::general("The design is the same as the table, so there's nothing to change"));
        }
        self.run_changes(&live, Some(&database), "alter_table", statements, None, log).await
    }

    pub async fn drop_object(&self, args: DropObjectArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_changes(&live, log)?;
        let database = self.database(&live, args.database, log)?;
        let sql = live
            .session
            .session
            .object_sql(&database, &args.name, args.kind.into(), args.signature.as_deref(), SqlTemplate::Drop)
            .await?;
        let statement = sql.trim().trim_end_matches(';').to_string();
        self.run_changes(&live, Some(&database), "drop_object", vec![statement], None, log).await
    }

    pub async fn execute_sql(&self, args: SqlArgs, log: &CallLog) -> AppResult<Value> {
        let live = self.live(&args.session, log).await?;
        self.require_raw_sql(&live, log)?;
        self.require_changes(&live, log)?;
        let database = self.optional_database(&live, args.database, log)?;
        let statements: Vec<String> = live
            .session
            .session
            .classify(&args.sql)
            .into_iter()
            .map(|statement| statement.text)
            .collect();
        self.run_changes(&live, database.as_deref(), "execute_sql", statements, None, log).await
    }
}

fn parse_design(design: Value) -> AppResult<TableDesign> {
    serde_json::from_value(design).map_err(|error| {
        AppError::general(format!("The design doesn't have the shape get_table_design returns: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::ValueCategory;

    fn column(name: &str, primary_key: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            type_name: String::new(),
            category: ValueCategory::Integer,
            nullable: true,
            primary_key,
            database: None,
            table: None,
            source_name: Some(name.to_string()),
            has_choices: false,
        }
    }

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn builds_keys_from_the_primary_key() {
        let columns = vec![column("a", true), column("b", false), column("c", true)];
        assert_eq!(
            key_row(&columns, &map(json!({ "a": 1, "c": "x" })), "t").unwrap(),
            vec![Some("1".to_string()), None, Some("x".to_string())]
        );
        assert!(key_row(&columns, &map(json!({ "a": 1 })), "t").is_err());
        assert!(key_row(&columns, &map(json!({ "a": 1, "c": 2, "b": 3 })), "t").is_err());
    }

    #[test]
    fn writes_cells_like_the_row_editor() {
        let columns = vec![column("id", true), column("flag", false)];
        let edits = edits(&columns, &map(json!({ "flag": true, "id": null })), "t").unwrap();
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().any(|edit| edit.column == 1 && edit.value.as_deref() == Some("1")));
        assert!(edits.iter().any(|edit| edit.column == 0 && edit.value.is_none()));
        assert!(super::edits(&columns, &map(json!({ "nope": 1 })), "t").is_err());
    }
}
