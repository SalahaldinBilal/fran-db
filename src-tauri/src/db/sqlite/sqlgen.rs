use crate::db::types::{ColumnMeta, RowChange, ValueCategory};
use crate::error::{AppError, AppResult};

pub fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", quote_identifier(schema), quote_identifier(name))
}

/// SQLite strings never take backslash escapes.
pub fn string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub fn value_literal(value: Option<&str>, category: ValueCategory) -> String {
    let Some(value) = value else {
        return "NULL".to_string();
    };

    match category {
        ValueCategory::Binary => match hex_body(value) {
            Some(hex) => format!("X'{hex}'"),
            None => string_literal(value),
        },
        ValueCategory::Boolean if value.eq_ignore_ascii_case("true") => "1".to_string(),
        ValueCategory::Boolean if value.eq_ignore_ascii_case("false") => "0".to_string(),
        ValueCategory::Integer
        | ValueCategory::Decimal
        | ValueCategory::Float
        | ValueCategory::Boolean
        | ValueCategory::Other
            if is_number(value) =>
        {
            value.to_string()
        }
        _ => string_literal(value),
    }
}

fn hex_body(value: &str) -> Option<&str> {
    let body = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X"))?;
    (body.len() % 2 == 0 && body.chars().all(|character| character.is_ascii_hexdigit())).then_some(body)
}

fn is_number(value: &str) -> bool {
    let unsigned = value.strip_prefix(['-', '+']).unwrap_or(value);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };

    let mut parts = mantissa.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    let digits = |part: &str| part.chars().all(|character| character.is_ascii_digit());

    let mantissa_ok = !(whole.is_empty() && fraction.is_empty()) && digits(whole) && digits(fraction);
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });

    mantissa_ok && exponent_ok
}

fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '!' | '%' | '_') {
            escaped.push('!');
        }
        escaped.push(character);
    }
    escaped
}

/// LIKE ignores ASCII case in SQLite. Blob columns match on their hex.
pub fn search_condition(columns: &[ColumnMeta], term: &str) -> String {
    let pattern = string_literal(&format!("%{}%", escape_like(term)));
    let hex = term.strip_prefix("0x").or_else(|| term.strip_prefix("0X")).unwrap_or(term);
    let hex_pattern = (!hex.is_empty() && hex.chars().all(|character| character.is_ascii_hexdigit()))
        .then(|| format!("'%{}%'", hex.to_ascii_uppercase()));

    let conditions: Vec<String> = columns
        .iter()
        .filter_map(|column| {
            let name = quote_identifier(&column.name);
            match column.category {
                ValueCategory::Binary => hex_pattern.as_ref().map(|hex| format!("hex({name}) LIKE {hex}")),
                _ => Some(format!("CAST({name} AS TEXT) LIKE {pattern} ESCAPE '!'")),
            }
        })
        .collect();

    if conditions.is_empty() {
        "0".to_string()
    } else {
        conditions.join(" OR ")
    }
}

fn source_identifier(meta: &ColumnMeta) -> AppResult<String> {
    meta.source_name
        .as_deref()
        .map(quote_identifier)
        .ok_or_else(|| AppError::general(format!("{} isn't a table column, so it can't be written", meta.name)))
}

const MAX_ROWS_PER_STATEMENT: usize = 500;

struct KeyedRow {
    key: Vec<Option<String>>,
    tuple: String,
}

pub fn row_changes_sql(
    schema: &str,
    table: &str,
    columns: &[ColumnMeta],
    changes: &[RowChange],
) -> AppResult<Vec<String>> {
    let key_columns: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.primary_key)
        .map(|(index, _)| index)
        .collect();

    if key_columns.is_empty() {
        return Err(AppError::general(
            "This table has no primary key, so rows can't be matched safely for editing",
        ));
    }

    let target = qualified(schema, table);
    let column = |index: usize| -> AppResult<&ColumnMeta> {
        columns
            .get(index)
            .ok_or_else(|| AppError::general(format!("Column {index} is out of range")))
    };

    let where_clause = |key: &[Option<String>]| -> AppResult<String> {
        key_columns
            .iter()
            .map(|&index| {
                let meta = column(index)?;
                let name = source_identifier(meta)?;
                let value = key.get(index).cloned().flatten();
                Ok(match value {
                    Some(value) => format!("{name} = {}", value_literal(Some(&value), meta.category)),
                    None => format!("{name} IS NULL"),
                })
            })
            .collect::<AppResult<Vec<_>>>()
            .map(|conditions| conditions.join(" AND "))
    };

    let key_names = key_columns
        .iter()
        .map(|&index| column(index).and_then(source_identifier))
        .collect::<AppResult<Vec<_>>>()?;
    let key_target = match key_names.as_slice() {
        [single] => single.clone(),
        names => format!("({})", names.join(", ")),
    };

    // None when part of the key is NULL, since `IN` never matches NULL.
    let key_tuple = |key: &[Option<String>]| -> AppResult<Option<String>> {
        let mut literals = Vec::with_capacity(key_columns.len());
        for &index in &key_columns {
            let Some(value) = key.get(index).cloned().flatten() else {
                return Ok(None);
            };
            literals.push(value_literal(Some(&value), column(index)?.category));
        }
        Ok(Some(match literals.as_slice() {
            [single] => single.clone(),
            many => format!("({})", many.join(", ")),
        }))
    };

    // A row value on the left of IN needs a subquery on the right, so composite keys go through VALUES.
    let keyed = |head: &str, rows: &[KeyedRow]| -> AppResult<String> {
        if let [row] = rows {
            return Ok(format!("{head} WHERE {}", where_clause(&row.key)?));
        }
        let tuples = rows.iter().map(|row| row.tuple.as_str()).collect::<Vec<_>>().join(", ");
        Ok(if key_columns.len() == 1 {
            format!("{head} WHERE {key_target} IN ({tuples})")
        } else {
            format!("{head} WHERE {key_target} IN (VALUES {tuples})")
        })
    };

    let mut single_updates = Vec::new();
    let mut single_deletes = Vec::new();
    let mut updates: Vec<(String, Vec<KeyedRow>)> = Vec::new();
    let mut deletes: Vec<KeyedRow> = Vec::new();
    let mut inserts: Vec<(String, Vec<String>)> = Vec::new();

    for change in changes {
        match change {
            RowChange::Update { key, values } => {
                if values.is_empty() {
                    continue;
                }
                let mut sorted: Vec<_> = values.iter().collect();
                sorted.sort_by_key(|edit| edit.column);
                let assignments = sorted
                    .iter()
                    .map(|edit| {
                        let meta = column(edit.column)?;
                        Ok(format!(
                            "{} = {}",
                            source_identifier(meta)?,
                            value_literal(edit.value.as_deref(), meta.category)
                        ))
                    })
                    .collect::<AppResult<Vec<_>>>()?
                    .join(", ");
                let head = format!("UPDATE {target} SET {assignments}");

                let changes_key = sorted.iter().any(|edit| key_columns.contains(&edit.column));
                match key_tuple(key)? {
                    Some(tuple) if !changes_key => {
                        let row = KeyedRow { key: key.clone(), tuple };
                        match updates.iter_mut().find(|(existing, _)| *existing == head) {
                            Some((_, rows)) => rows.push(row),
                            None => updates.push((head, vec![row])),
                        }
                    }
                    _ => single_updates.push(format!("{head} WHERE {}", where_clause(key)?)),
                }
            }
            RowChange::Delete { key } => match key_tuple(key)? {
                Some(tuple) => deletes.push(KeyedRow { key: key.clone(), tuple }),
                None => single_deletes.push(format!("DELETE FROM {target} WHERE {}", where_clause(key)?)),
            },
            RowChange::Insert { values } => {
                let mut sorted: Vec<_> = values.iter().collect();
                sorted.sort_by_key(|edit| edit.column);
                if sorted.is_empty() {
                    inserts.push((String::new(), Vec::new()));
                    continue;
                }
                let names = sorted
                    .iter()
                    .map(|edit| column(edit.column).and_then(source_identifier))
                    .collect::<AppResult<Vec<_>>>()?
                    .join(", ");
                let literals = sorted
                    .iter()
                    .map(|edit| column(edit.column).map(|meta| value_literal(edit.value.as_deref(), meta.category)))
                    .collect::<AppResult<Vec<_>>>()?
                    .join(", ");
                match inserts.iter_mut().find(|(existing, _)| *existing == names) {
                    Some((_, rows)) => rows.push(format!("({literals})")),
                    None => inserts.push((names, vec![format!("({literals})")])),
                }
            }
        }
    }

    let mut statements = single_updates;
    for (head, rows) in &updates {
        for chunk in rows.chunks(MAX_ROWS_PER_STATEMENT) {
            statements.push(keyed(head, chunk)?);
        }
    }
    for chunk in deletes.chunks(MAX_ROWS_PER_STATEMENT) {
        statements.push(keyed(&format!("DELETE FROM {target}"), chunk)?);
    }
    statements.extend(single_deletes);
    for (names, rows) in &inserts {
        if names.is_empty() {
            statements.push(format!("INSERT INTO {target} DEFAULT VALUES"));
            continue;
        }
        for chunk in rows.chunks(MAX_ROWS_PER_STATEMENT) {
            statements.push(format!("INSERT INTO {target} ({names}) VALUES {}", chunk.join(", ")));
        }
    }

    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::CellEdit;

    fn column(name: &str, category: ValueCategory, primary_key: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            type_name: String::new(),
            category,
            nullable: !primary_key,
            primary_key,
            database: Some("main".to_string()),
            table: Some("t".to_string()),
            source_name: Some(name.to_string()),
            has_choices: false,
        }
    }

    #[test]
    fn groups_changes() {
        let columns = vec![
            column("id", ValueCategory::Integer, true),
            column("rating", ValueCategory::Text, false),
        ];
        let update = |id: &str, rating: Option<&str>| RowChange::Update {
            key: vec![Some(id.into()), Some("x".into())],
            values: vec![CellEdit { column: 1, value: rating.map(Into::into) }],
        };
        let changes = vec![
            update("1", None),
            update("15", None),
            update("16", Some("pg")),
            RowChange::Delete { key: vec![Some("19".into()), None] },
            RowChange::Delete { key: vec![Some("20".into()), None] },
            RowChange::Insert { values: vec![CellEdit { column: 1, value: Some("r".into()) }] },
            RowChange::Insert { values: vec![CellEdit { column: 1, value: Some("g".into()) }] },
            RowChange::Insert { values: Vec::new() },
        ];

        let statements = row_changes_sql("main", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![
            r#"UPDATE "main"."t" SET "rating" = NULL WHERE "id" IN (1, 15)"#,
            r#"UPDATE "main"."t" SET "rating" = 'pg' WHERE "id" = 16"#,
            r#"DELETE FROM "main"."t" WHERE "id" IN (19, 20)"#,
            r#"INSERT INTO "main"."t" ("rating") VALUES ('r'), ('g')"#,
            r#"INSERT INTO "main"."t" DEFAULT VALUES"#,
        ]);
    }

    #[test]
    fn composite_keys_use_values() {
        let columns = vec![
            column("a", ValueCategory::Integer, true),
            column("b", ValueCategory::Text, true),
        ];
        let changes = vec![
            RowChange::Delete { key: vec![Some("1".into()), Some("x".into())] },
            RowChange::Delete { key: vec![Some("2".into()), Some("y".into())] },
        ];
        let statements = row_changes_sql("main", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![r#"DELETE FROM "main"."t" WHERE ("a", "b") IN (VALUES (1, 'x'), (2, 'y'))"#]);
    }

    #[test]
    fn writes_typed_literals() {
        assert_eq!(value_literal(Some("42"), ValueCategory::Integer), "42");
        assert_eq!(value_literal(Some("abc"), ValueCategory::Integer), "'abc'");
        assert_eq!(value_literal(Some("42"), ValueCategory::Other), "42");
        assert_eq!(value_literal(Some("abc"), ValueCategory::Other), "'abc'");
        assert_eq!(value_literal(Some("0xBEEF"), ValueCategory::Binary), "X'BEEF'");
        assert_eq!(value_literal(Some("true"), ValueCategory::Boolean), "1");
        assert_eq!(value_literal(Some("0"), ValueCategory::Boolean), "0");
        assert_eq!(value_literal(Some(r"it's \ ok"), ValueCategory::Text), r"'it''s \ ok'");
        assert_eq!(value_literal(None, ValueCategory::Text), "NULL");
    }

    #[test]
    fn builds_search_conditions() {
        let columns = vec![
            column("id", ValueCategory::Integer, true),
            column("data", ValueCategory::Binary, false),
        ];
        assert_eq!(
            search_condition(&columns, "ab"),
            r#"CAST("id" AS TEXT) LIKE '%ab%' ESCAPE '!' OR hex("data") LIKE '%AB%'"#
        );
        assert_eq!(search_condition(&columns[1..], "zz"), "0");
    }
}
