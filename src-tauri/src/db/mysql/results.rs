use std::time::{Duration, Instant};

use mysql_async::{Conn, QueryResult, Row, TextProtocol, prelude::Queryable};

use super::values::{column_meta, format_value};
use crate::db::types::{ColumnMeta, ResultSet, StatementOutcome, ValueCategory};

/// Reads the current result set. Rows past `row_limit` are still drained so the connection stays usable.
pub async fn read_result_set(
    result: &mut QueryResult<'_, 'static, TextProtocol>,
    row_limit: usize,
) -> Result<Option<ResultSet>, mysql_async::Error> {
    let columns: Vec<ColumnMeta> = result.columns_ref().iter().map(column_meta).collect();
    let categories: Vec<ValueCategory> = columns.iter().map(|column| column.category).collect();
    let limit = if row_limit == 0 { usize::MAX } else { row_limit };

    let mut rows = Vec::new();
    let mut truncated = false;

    result
        .for_each(|row: Row| {
            if rows.len() >= limit {
                truncated = true;
                return;
            }
            rows.push(
                categories
                    .iter()
                    .enumerate()
                    .map(|(index, category)| format_value(row.as_ref(index), *category))
                    .collect(),
            );
        })
        .await?;

    if columns.is_empty() {
        return Ok(None);
    }

    Ok(Some(ResultSet {
        columns,
        rows,
        truncated,
    }))
}

pub async fn run_statement(
    conn: &mut Conn,
    sql: &str,
    row_limit: usize,
) -> Result<StatementOutcome, mysql_async::Error> {
    let started = Instant::now();
    let mut result = conn.query_iter(sql).await?;
    let answered = Instant::now();

    let mut outcome = StatementOutcome {
        sql: sql.to_string(),
        result_sets: Vec::new(),
        affected_rows: 0,
        last_insert_id: None,
        warnings: 0,
        info: None,
        elapsed_ms: 0.0,
        execution_ms: milliseconds(answered - started),
        network_ms: None,
        fetch_ms: 0.0,
        error: None,
    };

    record_status(&result, &mut outcome);
    while !result.is_empty() {
        if let Some(set) = read_result_set(&mut result, row_limit).await? {
            outcome.result_sets.push(set);
        }
        record_status(&result, &mut outcome);
    }

    outcome.fetch_ms = milliseconds(answered.elapsed());
    Ok(outcome)
}

pub fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn record_status(result: &QueryResult<'_, 'static, TextProtocol>, outcome: &mut StatementOutcome) {
    outcome.affected_rows = result.affected_rows();
    outcome.last_insert_id = result.last_insert_id().or(outcome.last_insert_id);
    outcome.warnings = outcome.warnings.max(result.warnings());

    let info = result.info();
    if !info.is_empty() {
        outcome.info = Some(info.into_owned());
    }
}
