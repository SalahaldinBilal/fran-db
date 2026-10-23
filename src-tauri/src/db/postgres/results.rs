use std::time::{Duration, Instant};

use super::client::{PgConnection, PgError, RawColumn, RawResult, Report, SimpleOutcome};
use super::relations;
use super::types::{self, TypeCache};
use crate::db::types::{ColumnMeta, ResultSet, StatementOutcome};

pub fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Column details for a result, with types and source tables looked up on `conn`.
pub async fn describe(conn: &mut PgConnection, cache: &TypeCache, columns: &[RawColumn]) -> Result<Vec<ColumnMeta>, PgError> {
    if columns.is_empty() {
        return Ok(Vec::new());
    }

    let oids: Vec<u32> = columns.iter().map(|column| column.type_oid).collect();
    let found = types::resolve(conn, cache, &oids).await?;

    let keys: Vec<(u32, i16)> = columns
        .iter()
        .filter(|column| column.table_oid != 0 && column.column_id > 0)
        .map(|column| (column.table_oid, column.column_id))
        .collect();
    let sources = relations::lookup(conn, &keys).await?;

    Ok(columns
        .iter()
        .map(|column| {
            let pg_type = found
                .get(&column.type_oid)
                .cloned()
                .unwrap_or_else(|| types::unresolved(column.type_oid));
            let source = sources.get(&(column.table_oid, column.column_id));

            ColumnMeta {
                name: column.name.clone(),
                type_name: types::display_name(&pg_type, column.type_oid, column.type_modifier),
                category: pg_type.category,
                nullable: source.is_none_or(|source| !source.not_null),
                primary_key: source.is_some_and(|source| source.primary_key),
                database: source.map(|source| source.schema.clone()),
                table: source.map(|source| source.table.clone()),
                source_name: source.map(|source| source.column.clone()),
                has_choices: pg_type.enumerated && source.is_some(),
            }
        })
        .collect())
}

/// Column details from the row description alone, for when the catalog couldn't be asked.
pub fn basic_columns(columns: &[RawColumn]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|column| {
            let pg_type = types::unresolved(column.type_oid);
            ColumnMeta {
                name: column.name.clone(),
                type_name: types::display_name(&pg_type, column.type_oid, column.type_modifier),
                category: pg_type.category,
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

pub fn result_set(raw: RawResult, columns: Vec<ColumnMeta>) -> ResultSet {
    let rows = raw
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(index, value)| {
                    let category = columns.get(index).map(|column| column.category);
                    match category {
                        Some(category) => types::display_value(value, category),
                        None => value,
                    }
                })
                .collect()
        })
        .collect();

    ResultSet {
        columns,
        rows,
        truncated: raw.truncated,
    }
}

/// The row count at the end of a command tag such as `INSERT 0 5` or `UPDATE 3`.
fn tag_count(tag: &str) -> Option<u64> {
    let (head, last) = tag.rsplit_once(' ')?;
    if head.is_empty() {
        return None;
    }
    last.parse().ok()
}

pub fn notice_text(notices: &[Report]) -> Vec<String> {
    notices
        .iter()
        .map(|notice| format!("{}: {}", notice.severity, notice.message))
        .collect()
}

pub fn warning_count(notices: &[Report]) -> u16 {
    let count = notices.iter().filter(|notice| notice.severity == "WARNING").count();
    u16::try_from(count).unwrap_or(u16::MAX)
}

pub struct Timing {
    pub started: Instant,
    pub finished: Instant,
}

pub fn outcome(
    sql: &str,
    raw: SimpleOutcome,
    columns: Vec<Vec<ColumnMeta>>,
    notices: &[Report],
    lookup_note: Option<String>,
    timing: Timing,
) -> StatementOutcome {
    let first_reply = raw.first_reply.unwrap_or(timing.finished);
    let result_sets: Vec<ResultSet> = raw
        .results
        .into_iter()
        .zip(columns)
        .map(|(result, columns)| result_set(result, columns))
        .collect();

    let affected_rows = if result_sets.is_empty() {
        raw.tags.iter().filter_map(|tag| tag_count(tag)).sum()
    } else {
        0
    };

    let mut info: Vec<String> = raw.tags.iter().filter(|tag| tag_count(tag).is_none()).cloned().collect();
    info.extend(notice_text(notices));
    info.extend(lookup_note);

    StatementOutcome {
        sql: sql.to_string(),
        result_sets,
        affected_rows,
        last_insert_id: None,
        warnings: warning_count(notices),
        info: (!info.is_empty()).then(|| info.join("\n")),
        elapsed_ms: milliseconds(timing.finished - timing.started),
        execution_ms: milliseconds(first_reply.saturating_duration_since(timing.started)),
        network_ms: None,
        fetch_ms: milliseconds(timing.finished.saturating_duration_since(first_reply)),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::tag_count;

    #[test]
    fn reads_counts_from_command_tags() {
        assert_eq!(tag_count("INSERT 0 5"), Some(5));
        assert_eq!(tag_count("UPDATE 3"), Some(3));
        assert_eq!(tag_count("SELECT 10"), Some(10));
        assert_eq!(tag_count("CREATE TABLE"), None);
        assert_eq!(tag_count("BEGIN"), None);
    }
}
