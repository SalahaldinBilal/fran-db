use mysql_async::{Conn, Row, prelude::Queryable};

use super::results::read_result_set;
use super::sqlgen::{Dialect, qualified, quote_identifier, search_condition};
use super::values::{column_meta, format_value, parse_choices};
use crate::db::types::{
    CatalogColumn, ColumnChoices, ColumnDefinition, ColumnMeta, DatabaseInfo, ForeignKeyDefinition, IndexDefinition, ObjectInfo,
    ObjectKind, ObjectStructure, ResultSet, ServerInfo, SqlTemplate, ValueCategory,
};
use crate::error::{AppError, AppResult};

const SYSTEM_DATABASES: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

pub(super) fn text(row: &Row, index: usize) -> Option<String> {
    format_value(row.as_ref(index), ValueCategory::Text)
}

pub(super) fn name(row: &Row, index: usize) -> String {
    text(row, index).unwrap_or_default()
}

pub(super) fn number(row: &Row, index: usize) -> Option<u64> {
    text(row, index)?.parse().ok()
}

pub(super) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

pub async fn dialect(conn: &mut Conn) -> Result<Dialect, mysql_async::Error> {
    let rows: Vec<Row> = conn.query("SELECT @@SESSION.sql_mode").await?;
    let sql_mode = rows.first().and_then(|row| text(row, 0)).unwrap_or_default();
    Ok(Dialect {
        no_backslash_escapes: sql_mode.to_ascii_uppercase().contains("NO_BACKSLASH_ESCAPES"),
    })
}

pub async fn list_columns(conn: &mut Conn, database: &str) -> Result<Vec<CatalogColumn>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
        )
        .await?;

    Ok(rows
        .iter()
        .map(|row| CatalogColumn {
            table: name(row, 0),
            name: name(row, 1),
            data_type: name(row, 2),
        })
        .collect())
}

fn object_keyword(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "TABLE",
        ObjectKind::View => "VIEW",
        ObjectKind::MaterializedView => "MATERIALIZED VIEW",
        ObjectKind::Sequence => "SEQUENCE",
        ObjectKind::Procedure => "PROCEDURE",
        ObjectKind::Function => "FUNCTION",
        ObjectKind::Trigger => "TRIGGER",
        ObjectKind::Event => "EVENT",
    }
}

async fn column_names(conn: &mut Conn, database: &str, table: &str) -> Result<Vec<String>, mysql_async::Error> {
    Ok(columns(conn, database, table)
        .await?
        .into_iter()
        .map(|column| column.name)
        .collect())
}

pub async fn object_sql(
    conn: &mut Conn,
    database: &str,
    object: &str,
    kind: ObjectKind,
    template: SqlTemplate,
) -> AppResult<String> {
    let target = qualified(database, object);
    let has_rows = matches!(kind, ObjectKind::Table | ObjectKind::View);

    let sql = match template {
        SqlTemplate::Create => object_ddl(conn, database, object, kind)
            .await
            .map_err(super::session::db_error)?
            .ok_or_else(|| AppError::general("The server didn't return a definition"))?,
        SqlTemplate::Drop => format!("DROP {} {target}", object_keyword(kind)),
        _ if !has_rows => {
            return Err(AppError::general("Only tables and views have row statements"));
        }
        SqlTemplate::Select => {
            let names = column_names(conn, database, object).await.map_err(super::session::db_error)?;
            let list = names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(",\n    ");
            format!("SELECT\n    {list}\nFROM {target}")
        }
        SqlTemplate::Insert => {
            let names = column_names(conn, database, object).await.map_err(super::session::db_error)?;
            let list = names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(", ");
            let placeholders = names.iter().map(|name| format!("<{{{name}}}>")).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {target}\n    ({list})\nVALUES\n    ({placeholders})")
        }
        SqlTemplate::Update => {
            let names = column_names(conn, database, object).await.map_err(super::session::db_error)?;
            let assignments = names
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

pub async fn database_sql(conn: &mut Conn, database: &str, template: SqlTemplate) -> AppResult<String> {
    match template {
        SqlTemplate::Create => {
            let rows: Vec<Row> = conn
                .query(format!("SHOW CREATE DATABASE {}", quote_identifier(database)))
                .await
                .map_err(super::session::db_error)?;
            rows.first()
                .and_then(|row| text(row, 1))
                .map(|sql| format!("{sql};"))
                .ok_or_else(|| AppError::general("The server didn't return a definition"))
        }
        SqlTemplate::Drop => Ok(format!("DROP DATABASE {};", quote_identifier(database))),
        _ => Err(AppError::general("Databases only have create and drop statements")),
    }
}

pub async fn server_info(conn: &mut Conn) -> Result<ServerInfo, mysql_async::Error> {
    let rows: Vec<Row> = conn.query("SELECT VERSION()").await?;
    let version = rows.first().and_then(|row| text(row, 0)).unwrap_or_else(|| {
        let (major, minor, patch) = conn.server_version();
        format!("{major}.{minor}.{patch}")
    });

    let mariadb = version.to_ascii_lowercase().contains("mariadb");

    Ok(ServerInfo {
        product: if mariadb { "MariaDB" } else { "MySQL" }.to_string(),
        atomic_ddl: atomic_ddl(mariadb, &version),
        transactional_ddl: false,
        version,
    })
}

/// A failed DDL statement leaves nothing behind on MySQL 8.0+ and MariaDB 10.6+.
fn atomic_ddl(mariadb: bool, version: &str) -> bool {
    let mut parts = version.split(|character: char| !character.is_ascii_digit());
    let major: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    if mariadb {
        (major, minor) >= (10, 6)
    } else {
        major >= 8
    }
}

#[cfg(test)]
mod tests {
    use super::atomic_ddl;

    #[test]
    fn knows_which_servers_have_atomic_ddl() {
        assert!(atomic_ddl(false, "8.0.36"));
        assert!(atomic_ddl(false, "9.1.0"));
        assert!(!atomic_ddl(false, "5.7.44-log"));
        assert!(atomic_ddl(true, "10.11.6-MariaDB-1:10.11.6+maria~ubu2204"));
        assert!(atomic_ddl(true, "11.4.2-MariaDB"));
        assert!(!atomic_ddl(true, "10.5.23-MariaDB"));
    }
}

pub async fn list_databases(conn: &mut Conn) -> Result<Vec<DatabaseInfo>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
        .await?;

    Ok(rows
        .iter()
        .map(|row| {
            let database = name(row, 0);
            let system = SYSTEM_DATABASES.contains(&database.to_ascii_lowercase().as_str());
            DatabaseInfo {
                name: database,
                system,
            }
        })
        .collect())
}

pub async fn list_objects(conn: &mut Conn, database: &str) -> Result<Vec<ObjectInfo>, mysql_async::Error> {
    let mut objects = Vec::new();

    let tables: Vec<Row> = conn
        .exec(
            "SELECT TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH, TABLE_COMMENT \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await?;

    for row in &tables {
        let is_view = name(row, 1).eq_ignore_ascii_case("VIEW");
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: if is_view { ObjectKind::View } else { ObjectKind::Table },
            signature: None,
            detail: non_empty(text(row, 2)),
            row_estimate: if is_view { None } else { number(row, 3) },
            size_bytes: if is_view { None } else { number(row, 4) },
            comment: if is_view { None } else { non_empty(text(row, 5)) },
        });
    }

    let routines: Vec<Row> = conn
        .exec(
            "SELECT ROUTINE_NAME, ROUTINE_TYPE, ROUTINE_COMMENT FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME",
            (database,),
        )
        .await?;

    for row in &routines {
        let is_function = name(row, 1).eq_ignore_ascii_case("FUNCTION");
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: if is_function { ObjectKind::Function } else { ObjectKind::Procedure },
            signature: None,
            detail: None,
            row_estimate: None,
            size_bytes: None,
            comment: non_empty(text(row, 2)),
        });
    }

    let triggers: Vec<Row> = conn
        .exec(
            "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, EVENT_OBJECT_TABLE \
             FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA = ? ORDER BY TRIGGER_NAME",
            (database,),
        )
        .await?;

    for row in &triggers {
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: ObjectKind::Trigger,
            signature: None,
            detail: Some(format!("{} {} on {}", name(row, 1), name(row, 2), name(row, 3))),
            row_estimate: None,
            size_bytes: None,
            comment: None,
        });
    }

    let events: Vec<Row> = conn
        .exec(
            "SELECT EVENT_NAME, STATUS, EVENT_COMMENT FROM information_schema.EVENTS \
             WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME",
            (database,),
        )
        .await?;

    for row in &events {
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: ObjectKind::Event,
            signature: None,
            detail: non_empty(text(row, 1)),
            row_estimate: None,
            size_bytes: None,
            comment: non_empty(text(row, 2)),
        });
    }

    Ok(objects)
}

pub async fn object_structure(
    conn: &mut Conn,
    database: &str,
    object: &str,
    kind: ObjectKind,
) -> Result<ObjectStructure, mysql_async::Error> {
    let mut structure = ObjectStructure {
        ddl: object_ddl(conn, database, object, kind).await?,
        ..Default::default()
    };

    if matches!(kind, ObjectKind::Table | ObjectKind::View) {
        structure.columns = columns(conn, database, object).await?;
    }

    if kind == ObjectKind::Table {
        structure.indexes = indexes(conn, database, object).await?;
        structure.foreign_keys = foreign_keys(conn, database, object).await?;
    }

    Ok(structure)
}

async fn object_ddl(
    conn: &mut Conn,
    database: &str,
    object: &str,
    kind: ObjectKind,
) -> Result<Option<String>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .query(format!("SHOW CREATE {} {}", object_keyword(kind), qualified(database, object)))
        .await?;

    Ok(rows.first().and_then(|row| {
        let index = row.columns_ref().iter().position(|column| {
            let column_name = column.name_str();
            column_name.starts_with("Create ") || column_name == "SQL Original Statement"
        })?;
        text(row, index)
    }))
}

async fn columns(conn: &mut Conn, database: &str, table: &str) -> Result<Vec<ColumnDefinition>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, COLUMN_KEY, EXTRA, COLUMN_COMMENT, COLLATION_NAME \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await?;

    Ok(rows
        .iter()
        .map(|row| ColumnDefinition {
            name: name(row, 0),
            data_type: name(row, 1),
            nullable: name(row, 2).eq_ignore_ascii_case("YES"),
            default_value: text(row, 3),
            primary_key: name(row, 4).eq_ignore_ascii_case("PRI"),
            extra: non_empty(text(row, 5)),
            comment: non_empty(text(row, 6)),
            collation: non_empty(text(row, 7)),
        })
        .collect())
}

async fn indexes(conn: &mut Conn, database: &str, table: &str) -> Result<Vec<IndexDefinition>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, INDEX_TYPE FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
             ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX",
            (database, table),
        )
        .await?;

    let mut indexes: Vec<IndexDefinition> = Vec::new();
    for row in &rows {
        let index_name = name(row, 0);
        let column = text(row, 2).unwrap_or_else(|| "(expression)".to_string());

        match indexes.last_mut() {
            Some(last) if last.name == index_name => last.columns.push(column),
            _ => indexes.push(IndexDefinition {
                primary: index_name == "PRIMARY",
                unique: number(row, 1) == Some(0),
                name: index_name,
                columns: vec![column],
                index_type: non_empty(text(row, 3)),
            }),
        }
    }

    Ok(indexes)
}

async fn foreign_keys(
    conn: &mut Conn,
    database: &str,
    table: &str,
) -> Result<Vec<ForeignKeyDefinition>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, \
                    k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE k \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME \
             WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            (database, table),
        )
        .await?;

    let mut keys: Vec<ForeignKeyDefinition> = Vec::new();
    for row in &rows {
        let constraint = name(row, 0);

        match keys.last_mut() {
            Some(last) if last.name == constraint => {
                last.columns.push(name(row, 1));
                last.referenced_columns.push(name(row, 4));
            }
            _ => keys.push(ForeignKeyDefinition {
                name: constraint,
                columns: vec![name(row, 1)],
                referenced_database: name(row, 2),
                referenced_table: name(row, 3),
                referenced_columns: vec![name(row, 4)],
                on_update: non_empty(text(row, 5)),
                on_delete: non_empty(text(row, 6)),
            }),
        }
    }

    Ok(keys)
}

pub async fn column_choices(
    conn: &mut Conn,
    database: &str,
    table: &str,
    column: &str,
) -> Result<Option<ColumnChoices>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT COLUMN_TYPE FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND COLUMN_NAME = ?",
            (database, table, column),
        )
        .await?;

    Ok(rows.first().and_then(|row| parse_choices(&name(row, 0))))
}

pub async fn primary_key(conn: &mut Conn, database: &str, table: &str) -> Result<Vec<String>, mysql_async::Error> {
    let rows: Vec<Row> = conn
        .exec(
            "SELECT COLUMN_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY' ORDER BY SEQ_IN_INDEX",
            (database, table),
        )
        .await?;

    Ok(rows.iter().map(|row| name(row, 0)).collect())
}

pub async fn fetch_rows(
    conn: &mut Conn,
    dialect: Dialect,
    database: &str,
    table: &str,
    offset: u64,
    limit: u64,
    search: Option<&str>,
) -> Result<ResultSet, mysql_async::Error> {
    let target = qualified(database, table);
    let filter = match search.map(str::trim).filter(|term| !term.is_empty()) {
        Some(term) => {
            let probe = conn.query_iter(format!("SELECT * FROM {target} LIMIT 0")).await?;
            let columns: Vec<ColumnMeta> = probe.columns_ref().iter().map(column_meta).collect();
            probe.drop_result().await?;
            format!(" WHERE {}", search_condition(dialect, &columns, term))
        }
        None => String::new(),
    };
    let sql = format!("SELECT * FROM {target}{filter} LIMIT {limit} OFFSET {offset}");

    let mut result = conn.query_iter(sql).await?;
    let set = read_result_set(&mut result, usize::try_from(limit).unwrap_or(usize::MAX)).await?;
    result.drop_result().await?;

    Ok(set.unwrap_or(ResultSet {
        columns: Vec::new(),
        rows: Vec::new(),
        truncated: false,
    }))
}
