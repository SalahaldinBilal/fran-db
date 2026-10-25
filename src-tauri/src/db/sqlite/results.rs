use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use super::connection::{RawColumn, RawResult, SqlValue, SqliteConnection};
use super::metadata::{self, TableColumn};
use crate::db::types::{ColumnMeta, ResultSet, StatementOutcome, ValueCategory};
use crate::error::AppResult;

pub fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Follows SQLite's own affinity rules, with dates, booleans and JSON picked out by name first.
/// An untyped column stays `Other`: it has no affinity, so a quoted number would never match the
/// integer stored in it.
pub fn category(decl_type: &str) -> ValueCategory {
    let upper = decl_type.to_ascii_uppercase();
    let has = |part: &str| upper.contains(part);

    if upper.trim().is_empty() {
        ValueCategory::Other
    } else if has("JSON") {
        ValueCategory::Json
    } else if has("BOOL") {
        ValueCategory::Boolean
    } else if has("DATE") || has("TIME") {
        ValueCategory::Temporal
    } else if has("INT") {
        ValueCategory::Integer
    } else if has("CHAR") || has("CLOB") || has("TEXT") {
        ValueCategory::Text
    } else if has("BLOB") {
        ValueCategory::Binary
    } else if has("REAL") || has("FLOA") || has("DOUB") {
        ValueCategory::Float
    } else if has("DEC") || has("NUM") {
        ValueCategory::Decimal
    } else {
        ValueCategory::Other
    }
}

fn inferred_type(rows: &[Vec<SqlValue>], index: usize) -> &'static str {
    rows.iter()
        .filter_map(|row| row.get(index))
        .find_map(|value| match value {
            SqlValue::Null => None,
            SqlValue::Integer(_) => Some("INTEGER"),
            SqlValue::Real(_) => Some("REAL"),
            SqlValue::Text(_) => Some("TEXT"),
            SqlValue::Blob(_) => Some("BLOB"),
        })
        .unwrap_or_default()
}

fn format_real(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

pub fn display_value(value: &SqlValue) -> Option<String> {
    match value {
        SqlValue::Null => None,
        SqlValue::Integer(value) => Some(value.to_string()),
        SqlValue::Real(value) => Some(format_real(*value)),
        SqlValue::Text(value) => Some(value.clone()),
        SqlValue::Blob(bytes) => Some(format!("0x{}", hex::encode_upper(bytes))),
    }
}

/// The schema and table a result came from when the backend can't say per column, as for a table's rows.
pub type FallbackSource<'a> = Option<(&'a str, &'a str)>;

pub async fn describe(
    conn: &dyn SqliteConnection,
    raw: &RawResult,
    fallback: FallbackSource<'_>,
) -> AppResult<Vec<ColumnMeta>> {
    let mut tables: HashMap<(String, String), Vec<TableColumn>> = HashMap::new();
    let mut wanted: Vec<(String, String)> = raw
        .columns
        .iter()
        .filter_map(|column| Some((column.schema.clone()?, column.table.clone()?)))
        .collect();
    if let Some((schema, table)) = fallback {
        wanted.push((schema.to_string(), table.to_string()));
    }
    for key in wanted {
        if !tables.contains_key(&key) {
            let columns = metadata::table_columns(conn, &key.0, &key.1).await?;
            tables.insert(key, columns);
        }
    }

    Ok(raw
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let source = source_of(column, fallback).and_then(|(schema, table, origin)| {
                let found = tables
                    .get(&(schema.clone(), table.clone()))?
                    .iter()
                    .find(|candidate| candidate.name == origin)?;
                Some((schema, table, found))
            });
            let type_name = column
                .decl_type
                .clone()
                .filter(|decl| !decl.is_empty())
                .or_else(|| source.as_ref().map(|(_, _, found)| found.data_type.clone()))
                .unwrap_or_else(|| inferred_type(&raw.rows, index).to_string());

            ColumnMeta {
                name: column.name.clone(),
                category: category(&type_name),
                type_name,
                nullable: source.as_ref().is_none_or(|(_, _, found)| !found.not_null),
                primary_key: source.as_ref().is_some_and(|(_, _, found)| found.primary_key > 0),
                source_name: source.as_ref().map(|(_, _, found)| found.name.clone()),
                database: source.as_ref().map(|(schema, _, _)| schema.clone()),
                table: source.map(|(_, table, _)| table),
                has_choices: false,
            }
        })
        .collect())
}

fn source_of(column: &RawColumn, fallback: FallbackSource<'_>) -> Option<(String, String, String)> {
    match (&column.schema, &column.table, &column.origin) {
        (Some(schema), Some(table), Some(origin)) => Some((schema.clone(), table.clone(), origin.clone())),
        _ => fallback.map(|(schema, table)| (schema.to_string(), table.to_string(), column.name.clone())),
    }
}

pub fn basic_columns(raw: &RawResult) -> Vec<ColumnMeta> {
    raw.columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let type_name = column
                .decl_type
                .clone()
                .filter(|decl| !decl.is_empty())
                .unwrap_or_else(|| inferred_type(&raw.rows, index).to_string());
            ColumnMeta {
                name: column.name.clone(),
                category: category(&type_name),
                type_name,
                nullable: true,
                primary_key: false,
                database: None,
                table: None,
                source_name: None,
                has_choices: false,
            }
        })
        .collect()
}

pub fn result_set(raw: &RawResult, columns: Vec<ColumnMeta>) -> ResultSet {
    ResultSet {
        columns,
        rows: raw
            .rows
            .iter()
            .map(|row| row.iter().map(display_value).collect())
            .collect(),
        truncated: raw.truncated,
    }
}

pub struct Timing {
    pub started: Instant,
    pub first_reply: Instant,
    pub finished: Instant,
}

pub fn outcome(
    sql: &str,
    raw: &RawResult,
    columns: Vec<ColumnMeta>,
    lookup_note: Option<String>,
    timing: Timing,
) -> StatementOutcome {
    let has_rows = !raw.columns.is_empty();
    StatementOutcome {
        sql: sql.to_string(),
        result_sets: if has_rows { vec![result_set(raw, columns)] } else { Vec::new() },
        affected_rows: if has_rows { 0 } else { raw.changes },
        last_insert_id: raw.last_insert_id.and_then(|id| u64::try_from(id).ok()),
        warnings: 0,
        info: lookup_note,
        elapsed_ms: milliseconds(timing.finished - timing.started),
        execution_ms: raw
            .server_ms
            .unwrap_or_else(|| milliseconds(timing.first_reply - timing.started)),
        network_ms: None,
        fetch_ms: milliseconds(timing.finished.saturating_duration_since(timing.first_reply)),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_declared_types() {
        assert_eq!(category("INTEGER"), ValueCategory::Integer);
        assert_eq!(category("unsigned big int"), ValueCategory::Integer);
        assert_eq!(category("VARCHAR(20)"), ValueCategory::Text);
        assert_eq!(category("DATETIME"), ValueCategory::Temporal);
        assert_eq!(category("BOOLEAN"), ValueCategory::Boolean);
        assert_eq!(category("DOUBLE PRECISION"), ValueCategory::Float);
        assert_eq!(category("DECIMAL(10,2)"), ValueCategory::Decimal);
        assert_eq!(category("BLOB"), ValueCategory::Binary);
        assert_eq!(category("JSON"), ValueCategory::Json);
        assert_eq!(category(""), ValueCategory::Other);
    }

    #[test]
    fn displays_values() {
        assert_eq!(display_value(&SqlValue::Real(2.0)).unwrap(), "2.0");
        assert_eq!(display_value(&SqlValue::Real(0.25)).unwrap(), "0.25");
        assert_eq!(display_value(&SqlValue::Blob(vec![0xde, 0xad])).unwrap(), "0xDEAD");
        assert_eq!(display_value(&SqlValue::Null), None);
    }
}
