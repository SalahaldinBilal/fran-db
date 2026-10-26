use serde_json::{Value, json};

use crate::db::types::{ResultSet, StatementOutcome};

const CELL_LIMIT: usize = 500;
const REPLY_LIMIT: usize = 100_000;

fn cell(value: &Option<String>, cut: &mut bool) -> Value {
    match value {
        None => Value::Null,
        Some(text) if text.chars().count() > CELL_LIMIT => {
            *cut = true;
            Value::String(format!("{}...", text.chars().take(CELL_LIMIT).collect::<String>()))
        }
        Some(text) => Value::String(text.clone()),
    }
}

/// Rows as arrays to save tokens, trimmed to keep the whole reply near `REPLY_LIMIT`.
pub fn result_set(set: &ResultSet) -> Value {
    let columns: Vec<Value> = set
        .columns
        .iter()
        .map(|column| json!({ "name": column.name, "type": column.type_name }))
        .collect();

    let mut cut_cells = false;
    let mut size = 0;
    let mut truncated = set.truncated;
    let mut rows = Vec::with_capacity(set.rows.len());
    for row in &set.rows {
        let row = Value::Array(row.iter().map(|value| cell(value, &mut cut_cells)).collect());
        size += row.to_string().len();
        if size > REPLY_LIMIT {
            truncated = true;
            break;
        }
        rows.push(row);
    }

    let mut reply = json!({ "columns": columns, "rows": rows, "truncated": truncated });
    if cut_cells {
        reply["note"] = json!(format!("Values longer than {CELL_LIMIT} characters were cut."));
    }
    reply
}

pub fn statement(outcome: &StatementOutcome) -> Value {
    let mut reply = json!({ "sql": outcome.sql });
    match &outcome.error {
        Some(error) => reply["error"] = json!(error.message),
        None if outcome.result_sets.is_empty() => reply["affectedRows"] = json!(outcome.affected_rows),
        None => reply["results"] = Value::Array(outcome.result_sets.iter().map(result_set).collect()),
    }
    if let Some(info) = &outcome.info {
        reply["info"] = json!(info);
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::{ColumnMeta, ValueCategory};

    #[test]
    fn cuts_long_cells_and_big_replies() {
        let column = ColumnMeta {
            name: "body".to_string(),
            type_name: "text".to_string(),
            category: ValueCategory::Text,
            nullable: true,
            primary_key: false,
            database: None,
            table: None,
            source_name: None,
            has_choices: false,
        };
        let set = ResultSet {
            columns: vec![column],
            rows: (0..400).map(|_| vec![Some("x".repeat(600))]).chain([vec![None]]).collect(),
            truncated: false,
        };

        let reply = result_set(&set);
        let rows = reply["rows"].as_array().unwrap();
        assert!(rows.len() < 400);
        assert_eq!(rows[0][0].as_str().unwrap().len(), CELL_LIMIT + 3);
        assert_eq!(reply["truncated"], true);
        assert!(reply["note"].is_string());
    }
}
