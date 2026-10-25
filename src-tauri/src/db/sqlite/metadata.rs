use super::connection::{SqlValue, SqliteConnection};
use super::definition;
use super::results::{describe, result_set};
use super::sqlgen::{qualified, quote_identifier, search_condition, string_literal};
use crate::db::types::{
    CatalogColumn, ColumnDefinition, DatabaseInfo, ForeignKeyDefinition, IndexDefinition, ObjectInfo, ObjectKind,
    ObjectStructure, ResultSet, SqlTemplate,
};
use crate::error::{AppError, AppResult};

pub type Row = Vec<SqlValue>;

pub fn text(row: &Row, index: usize) -> Option<String> {
    match row.get(index)? {
        SqlValue::Null | SqlValue::Blob(_) => None,
        SqlValue::Integer(value) => Some(value.to_string()),
        SqlValue::Real(value) => Some(value.to_string()),
        SqlValue::Text(value) => Some(value.clone()),
    }
}

pub fn name(row: &Row, index: usize) -> String {
    text(row, index).unwrap_or_default()
}

pub fn int(row: &Row, index: usize) -> i64 {
    match row.get(index) {
        Some(SqlValue::Integer(value)) => *value,
        Some(SqlValue::Text(value)) => value.parse().unwrap_or(0),
        _ => 0,
    }
}

pub fn param(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

pub async fn rows(conn: &dyn SqliteConnection, sql: &str, params: &[SqlValue]) -> AppResult<Vec<Row>> {
    Ok(conn.query(sql, params, usize::MAX).await?.rows)
}

fn master(schema: &str) -> String {
    format!("{}.sqlite_master", quote_identifier(schema))
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    /// Position in the key, 0 when outside it.
    pub primary_key: i64,
    /// 2 for a virtual generated column, 3 for a stored one.
    pub hidden: i64,
}

pub async fn table_columns(conn: &dyn SqliteConnection, schema: &str, table: &str) -> AppResult<Vec<TableColumn>> {
    let found = rows(
        conn,
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1, ?2) \
         WHERE hidden <> 1 ORDER BY cid",
        &[param(table), param(schema)],
    )
    .await?;

    Ok(found
        .iter()
        .map(|row| TableColumn {
            name: name(row, 0),
            data_type: name(row, 1),
            not_null: int(row, 2) != 0,
            default_value: text(row, 3),
            primary_key: int(row, 4),
            hidden: int(row, 5),
        })
        .collect())
}

pub fn key_columns(columns: &[TableColumn]) -> Vec<String> {
    let mut key: Vec<&TableColumn> = columns.iter().filter(|column| column.primary_key > 0).collect();
    key.sort_by_key(|column| column.primary_key);
    key.into_iter().map(|column| column.name.clone()).collect()
}

pub async fn stored_sql(conn: &dyn SqliteConnection, schema: &str, kind: &str, object: &str) -> AppResult<Option<String>> {
    let found = rows(
        conn,
        &format!("SELECT sql FROM {} WHERE type = ?1 AND name = ?2", master(schema)),
        &[param(kind), param(object)],
    )
    .await?;
    Ok(found.first().and_then(|row| text(row, 0)))
}

pub async fn list_databases(conn: &dyn SqliteConnection) -> AppResult<Vec<DatabaseInfo>> {
    let found = rows(conn, "SELECT name FROM pragma_database_list WHERE name <> 'temp' ORDER BY seq", &[]).await?;
    Ok(found
        .iter()
        .map(|row| DatabaseInfo {
            name: name(row, 0),
            system: false,
        })
        .collect())
}

fn trigger_detail(sql: &str, table: &str) -> String {
    let words: Vec<String> = sql
        .split_whitespace()
        .map(str::to_ascii_uppercase)
        .take_while(|word| word != "ON")
        .collect();
    let timing = if words.iter().any(|word| word == "INSTEAD") {
        "INSTEAD OF"
    } else if words.iter().any(|word| word == "BEFORE") {
        "BEFORE"
    } else {
        "AFTER"
    };
    match words.iter().find(|word| matches!(word.as_str(), "INSERT" | "UPDATE" | "DELETE")) {
        Some(event) => format!("{timing} {event} on {table}"),
        None => format!("on {table}"),
    }
}

pub async fn list_objects(
    conn: &dyn SqliteConnection,
    schema: &str,
    is_internal: &(dyn Fn(&str) -> bool + Sync),
) -> AppResult<Vec<ObjectInfo>> {
    let found = rows(
        conn,
        &format!(
            "SELECT type, name, tbl_name, sql FROM {} WHERE type IN ('table', 'view', 'trigger') ORDER BY name",
            master(schema)
        ),
        &[],
    )
    .await?;

    Ok(found
        .iter()
        .filter(|row| !is_internal(&name(row, 1)))
        .map(|row| {
            let sql = name(row, 3);
            let table = name(row, 2);
            let (kind, signature, detail) = match name(row, 0).as_str() {
                "view" => (ObjectKind::View, None, None),
                "trigger" => (ObjectKind::Trigger, Some(table.clone()), Some(trigger_detail(&sql, &table))),
                _ => {
                    let is_virtual =
                        sql.split_whitespace().nth(1).is_some_and(|word| word.eq_ignore_ascii_case("VIRTUAL"));
                    (ObjectKind::Table, None, is_virtual.then(|| "virtual".to_string()))
                }
            };
            ObjectInfo {
                name: name(row, 1),
                kind,
                signature,
                detail,
                row_estimate: None,
                size_bytes: None,
                comment: None,
            }
        })
        .collect())
}

async fn relation_names(
    conn: &dyn SqliteConnection,
    schema: &str,
    is_internal: &(dyn Fn(&str) -> bool + Sync),
) -> AppResult<Vec<String>> {
    let found = rows(
        conn,
        &format!("SELECT name FROM {} WHERE type IN ('table', 'view') ORDER BY name", master(schema)),
        &[],
    )
    .await?;
    Ok(found.iter().map(|row| name(row, 0)).filter(|table| !is_internal(table)).collect())
}

/// One query for the whole database, or table by table when a broken view makes that fail.
pub async fn list_columns(
    conn: &dyn SqliteConnection,
    schema: &str,
    is_internal: &(dyn Fn(&str) -> bool + Sync),
) -> AppResult<Vec<CatalogColumn>> {
    let joined = rows(
        conn,
        &format!(
            "SELECT m.name, p.name, p.type FROM {} m JOIN pragma_table_xinfo(m.name, ?1) p \
             WHERE m.type IN ('table', 'view') AND p.hidden <> 1 ORDER BY m.name, p.cid",
            master(schema)
        ),
        &[param(schema)],
    )
    .await;

    if let Ok(found) = joined {
        return Ok(found
            .iter()
            .filter(|row| !is_internal(&name(row, 0)))
            .map(|row| CatalogColumn {
                table: name(row, 0),
                name: name(row, 1),
                data_type: name(row, 2),
            })
            .collect());
    }

    let mut columns = Vec::new();
    for table in relation_names(conn, schema, is_internal).await? {
        let Ok(found) = table_columns(conn, schema, &table).await else {
            continue;
        };
        columns.extend(found.into_iter().map(|column| CatalogColumn {
            table: table.clone(),
            name: column.name,
            data_type: column.data_type,
        }));
    }
    Ok(columns)
}

fn column_definitions(columns: &[TableColumn], create_sql: Option<&str>) -> Vec<ColumnDefinition> {
    let clauses = create_sql.map(definition::column_clauses).unwrap_or_default();
    let single_key = columns.iter().filter(|column| column.primary_key > 0).count() == 1;

    columns
        .iter()
        .map(|column| {
            let clause = clauses.iter().find(|clause| clause.name.eq_ignore_ascii_case(&column.name));
            let generated = clause.and_then(|clause| clause.generated.clone()).unwrap_or_default();
            let extra = match column.hidden {
                2 => Some(format!("GENERATED ALWAYS AS ({generated}) VIRTUAL")),
                3 => Some(format!("GENERATED ALWAYS AS ({generated}) STORED")),
                _ if single_key && column.primary_key > 0 && clause.is_some_and(|clause| clause.autoincrement) => {
                    Some("AUTOINCREMENT".to_string())
                }
                _ => None,
            };
            ColumnDefinition {
                name: column.name.clone(),
                data_type: column.data_type.clone(),
                nullable: !column.not_null,
                default_value: column.default_value.clone(),
                primary_key: column.primary_key > 0,
                extra,
                comment: None,
                collation: clause.and_then(|clause| clause.collation.clone()),
            }
        })
        .collect()
}

pub const PRIMARY_INDEX: &str = "PRIMARY";

async fn indexes(
    conn: &dyn SqliteConnection,
    schema: &str,
    table: &str,
    columns: &[TableColumn],
) -> AppResult<Vec<IndexDefinition>> {
    let mut indexes = Vec::new();
    let key = key_columns(columns);
    if !key.is_empty() {
        indexes.push(IndexDefinition {
            name: PRIMARY_INDEX.to_string(),
            primary: true,
            unique: true,
            columns: key,
            index_type: None,
        });
    }

    let found = rows(
        conn,
        "SELECT l.name, l.\"unique\", l.partial, i.name FROM pragma_index_list(?1, ?2) l \
         JOIN pragma_index_info(l.name, ?2) i WHERE l.origin <> 'pk' ORDER BY l.name, i.seqno",
        &[param(table), param(schema)],
    )
    .await?;

    for row in &found {
        let index_name = name(row, 0);
        let column = text(row, 3).unwrap_or_else(|| "(expression)".to_string());
        match indexes.last_mut() {
            Some(last) if last.name == index_name => last.columns.push(column),
            _ => indexes.push(IndexDefinition {
                name: index_name,
                primary: false,
                unique: int(row, 1) != 0,
                columns: vec![column],
                index_type: (int(row, 2) != 0).then(|| "partial".to_string()),
            }),
        }
    }

    Ok(indexes)
}

/// SQLite keys have no names here, so they're numbered.
pub struct ForeignKeyRow {
    pub id: i64,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

pub async fn foreign_key_rows(conn: &dyn SqliteConnection, schema: &str, table: &str) -> AppResult<Vec<ForeignKeyRow>> {
    let found = rows(
        conn,
        "SELECT id, \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list(?1, ?2) \
         ORDER BY id, seq",
        &[param(table), param(schema)],
    )
    .await?;

    let mut keys: Vec<ForeignKeyRow> = Vec::new();
    for row in &found {
        let id = int(row, 0);
        let referenced = text(row, 3);
        match keys.last_mut() {
            Some(last) if last.id == id => {
                last.columns.push(name(row, 2));
                last.referenced_columns.extend(referenced);
            }
            _ => keys.push(ForeignKeyRow {
                id,
                columns: vec![name(row, 2)],
                referenced_table: name(row, 1),
                referenced_columns: referenced.into_iter().collect(),
                on_update: name(row, 4),
                on_delete: name(row, 5),
            }),
        }
    }

    // A key that names no parent columns points at the parent's primary key.
    for key in &mut keys {
        if key.referenced_columns.len() != key.columns.len() {
            key.referenced_columns = key_columns(&table_columns(conn, schema, &key.referenced_table).await?);
        }
    }

    Ok(keys)
}

pub fn foreign_key_name(table: &str, id: i64) -> String {
    format!("{table}_fk{id}")
}

async fn foreign_keys(conn: &dyn SqliteConnection, schema: &str, table: &str) -> AppResult<Vec<ForeignKeyDefinition>> {
    Ok(foreign_key_rows(conn, schema, table)
        .await?
        .into_iter()
        .map(|key| ForeignKeyDefinition {
            name: foreign_key_name(table, key.id),
            columns: key.columns,
            referenced_database: schema.to_string(),
            referenced_table: key.referenced_table,
            referenced_columns: key.referenced_columns,
            on_update: Some(key.on_update),
            on_delete: Some(key.on_delete),
        })
        .collect())
}

fn kind_name(kind: ObjectKind) -> AppResult<&'static str> {
    match kind {
        ObjectKind::Table => Ok("table"),
        ObjectKind::View => Ok("view"),
        ObjectKind::Trigger => Ok("trigger"),
        _ => Err(AppError::general("SQLite only has tables, views and triggers")),
    }
}

/// No trailing semicolon.
async fn object_ddl(conn: &dyn SqliteConnection, schema: &str, object: &str, kind: ObjectKind) -> AppResult<String> {
    let sql = stored_sql(conn, schema, kind_name(kind)?, object)
        .await?
        .ok_or_else(|| AppError::not_found(format!("{object} not found")))?;

    if kind != ObjectKind::Table {
        return Ok(sql);
    }

    let index_rows = rows(
        conn,
        &format!(
            "SELECT sql FROM {} WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL ORDER BY name",
            master(schema)
        ),
        &[param(object)],
    )
    .await?;

    let mut parts = vec![sql];
    parts.extend(index_rows.iter().filter_map(|row| text(row, 0)));
    Ok(parts.join(";\n\n"))
}

pub async fn object_structure(
    conn: &dyn SqliteConnection,
    schema: &str,
    object: &str,
    kind: ObjectKind,
) -> AppResult<ObjectStructure> {
    let ddl = object_ddl(conn, schema, object, kind).await?;
    let mut structure = ObjectStructure::default();

    if matches!(kind, ObjectKind::Table | ObjectKind::View) {
        let columns = table_columns(conn, schema, object).await?;
        let create_sql = (kind == ObjectKind::Table).then(|| ddl.split(";\n\n").next().unwrap_or_default());
        structure.columns = column_definitions(&columns, create_sql);
        if kind == ObjectKind::Table {
            structure.indexes = indexes(conn, schema, object, &columns).await?;
            structure.foreign_keys = foreign_keys(conn, schema, object).await?;
        }
    }

    structure.ddl = Some(ddl);
    Ok(structure)
}

pub async fn object_sql(
    conn: &dyn SqliteConnection,
    schema: &str,
    object: &str,
    kind: ObjectKind,
    template: SqlTemplate,
) -> AppResult<String> {
    let target = qualified(schema, object);
    let has_rows = matches!(kind, ObjectKind::Table | ObjectKind::View);
    let names = || async {
        table_columns(conn, schema, object)
            .await
            .map(|columns| columns.into_iter().map(|column| column.name).collect::<Vec<_>>())
    };

    let sql = match template {
        SqlTemplate::Create => object_ddl(conn, schema, object, kind).await?,
        SqlTemplate::Drop => format!("DROP {} {target}", kind_name(kind)?.to_ascii_uppercase()),
        _ if !has_rows => return Err(AppError::general("Only tables and views have row statements")),
        SqlTemplate::Select => {
            let list = names().await?.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(",\n    ");
            format!("SELECT\n    {list}\nFROM {target}")
        }
        SqlTemplate::Insert => {
            let names = names().await?;
            let list = names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(", ");
            let placeholders = names.iter().map(|name| format!("<{{{name}}}>")).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {target}\n    ({list})\nVALUES\n    ({placeholders})")
        }
        SqlTemplate::Update => {
            let assignments = names()
                .await?
                .iter()
                .map(|name| format!("{} = <{{{name}}}>", quote_identifier(name)))
                .collect::<Vec<_>>()
                .join(",\n    ");
            format!("UPDATE {target}\nSET\n    {assignments}\nWHERE <{{where_expression}}>")
        }
        SqlTemplate::Delete => format!("DELETE FROM {target}\nWHERE <{{where_expression}}>"),
    };

    Ok(format!("{sql};"))
}

pub async fn database_sql(conn: &dyn SqliteConnection, schema: &str, template: SqlTemplate) -> AppResult<String> {
    if schema == "main" {
        return Err(AppError::general("main is the connection's own database file"));
    }
    match template {
        SqlTemplate::Create => {
            let found = rows(conn, "SELECT file FROM pragma_database_list WHERE name = ?1", &[param(schema)]).await?;
            let file = found
                .first()
                .map(|row| name(row, 0))
                .ok_or_else(|| AppError::not_found(format!("Database {schema} not found")))?;
            Ok(format!("ATTACH DATABASE {} AS {};", string_literal(&file), quote_identifier(schema)))
        }
        SqlTemplate::Drop => Ok(format!("DETACH DATABASE {};", quote_identifier(schema))),
        _ => Err(AppError::general("Databases only have attach and detach statements")),
    }
}

pub async fn primary_key(conn: &dyn SqliteConnection, schema: &str, table: &str) -> AppResult<Vec<String>> {
    if stored_sql(conn, schema, "table", table).await?.is_none() {
        return Ok(Vec::new());
    }
    Ok(key_columns(&table_columns(conn, schema, table).await?))
}

pub async fn fetch_rows(
    conn: &dyn SqliteConnection,
    schema: &str,
    table: &str,
    offset: u64,
    limit: u64,
    search: Option<&str>,
) -> AppResult<ResultSet> {
    let target = qualified(schema, table);
    let source = Some((schema, table));
    let filter = match search.map(str::trim).filter(|term| !term.is_empty()) {
        Some(term) => {
            let probe = conn.query(&format!("SELECT * FROM {target} LIMIT 0"), &[], 0).await?;
            let columns = describe(conn, &probe, source).await?;
            format!(" WHERE {}", search_condition(&columns, term))
        }
        None => String::new(),
    };

    let sql = format!("SELECT * FROM {target}{filter} LIMIT {limit} OFFSET {offset}");
    let raw = conn.query(&sql, &[], usize::try_from(limit).unwrap_or(usize::MAX)).await?;
    let columns = describe(conn, &raw, source).await?;
    Ok(result_set(&raw, columns))
}

fn missing_function(message: &str) -> Option<&str> {
    let rest = ["unknown function: ", "no such function: "]
        .iter()
        .find_map(|prefix| message.find(prefix).map(|at| &message[at + prefix.len()..]))?;
    let end = rest
        .find(|character: char| !(character.is_alphanumeric() || character == '_' || character == '$'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

fn calls_function(sql: &str, function: &str) -> bool {
    let lower = sql.to_lowercase();
    let function = function.to_lowercase();
    lower.match_indices(&function).any(|(at, _)| {
        let before = lower[..at].chars().next_back();
        let after = lower[at + function.len()..].trim_start();
        !before.is_some_and(|character| character.is_alphanumeric() || character == '_' || character == '$')
            && after.starts_with('(')
    })
}

fn object_label(kind: &str, object: &str, table: &str) -> String {
    match kind {
        "index" | "trigger" => format!("the {kind} {object} on {table}"),
        "table" => format!("a generated column or CHECK constraint of the table {object}"),
        _ => format!("the {kind} {object}"),
    }
}

/// Other errors come back unchanged.
pub async fn explain_missing_function(conn: &dyn SqliteConnection, error: AppError) -> AppError {
    let Some(function) = missing_function(&error.message) else {
        return error;
    };
    let Ok(databases) = list_databases(conn).await else {
        return error;
    };

    let mut users = Vec::new();
    for database in databases {
        let Ok(found) = rows(
            conn,
            &format!(
                "SELECT type, name, tbl_name, sql FROM {} WHERE sql IS NOT NULL AND instr(lower(sql), lower(?1)) > 0 \
                 ORDER BY type, name",
                master(&database.name)
            ),
            &[param(function)],
        )
        .await
        else {
            continue;
        };
        users.extend(
            found
                .iter()
                .filter(|row| calls_function(&name(row, 3), function))
                .map(|row| object_label(&name(row, 0), &name(row, 1), &name(row, 2))),
        );
    }
    if users.is_empty() {
        return error;
    }

    AppError {
        message: format!(
            "This change needs {function}(), which {} uses. Only the app that created this database has that \
             function, so make the change there.",
            users.join(", ")
        ),
        detail: error.detail,
    }
}

#[cfg(test)]
mod tests {
    use super::{calls_function, missing_function, trigger_detail};

    #[test]
    fn finds_missing_functions() {
        assert_eq!(missing_function("unknown function: rosemyne_fold()"), Some("rosemyne_fold"));
        assert_eq!(missing_function("no such function: fold"), Some("fold"));
        assert_eq!(missing_function("no such table: t"), None);
        assert!(calls_function("CREATE INDEX i ON t(Fold (a))", "fold"));
        assert!(!calls_function("CREATE INDEX i ON t(unfold(a), fold)", "fold"));
    }

    #[test]
    fn describes_triggers() {
        assert_eq!(
            trigger_detail("CREATE TRIGGER t BEFORE UPDATE OF a ON users BEGIN SELECT 1; END", "users"),
            "BEFORE UPDATE on users"
        );
        assert_eq!(trigger_detail("CREATE TRIGGER t DELETE ON users BEGIN END", "users"), "AFTER DELETE on users");
        assert_eq!(
            trigger_detail("create trigger t instead of insert on v begin end", "v"),
            "INSTEAD OF INSERT on v"
        );
    }
}
