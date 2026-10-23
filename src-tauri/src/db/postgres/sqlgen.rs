use crate::db::types::{ColumnMeta, RowChange, ValueCategory};
use crate::error::{AppError, AppResult};

/// What the server needs to know to write literals correctly for this session.
#[derive(Debug, Clone, Copy)]
pub struct Dialect {
    pub standard_conforming_strings: bool,
}

impl Default for Dialect {
    fn default() -> Self {
        Self {
            standard_conforming_strings: true,
        }
    }
}

pub fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", quote_identifier(schema), quote_identifier(name))
}

impl Dialect {
    pub fn string_literal(&self, value: &str) -> String {
        let escaped = value.replace('\'', "''");
        if self.standard_conforming_strings || !value.contains('\\') {
            format!("'{escaped}'")
        } else {
            format!("E'{}'", escaped.replace('\\', "\\\\"))
        }
    }

    pub fn value_literal(&self, value: Option<&str>, category: ValueCategory) -> String {
        let Some(value) = value else {
            return "NULL".to_string();
        };

        match category {
            ValueCategory::Binary => match hex_body(value) {
                Some(hex) => format!("decode('{hex}', 'hex')"),
                None => self.string_literal(value),
            },
            ValueCategory::Spatial => self.string_literal(hex_body(value).unwrap_or(value)),
            ValueCategory::Integer | ValueCategory::Decimal | ValueCategory::Float if is_number(value) => {
                value.to_string()
            }
            _ => self.string_literal(value),
        }
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

/// Matches rows where any column, as displayed, contains `term`. Bytea and PostGIS columns match on their hex.
pub fn search_condition(dialect: Dialect, columns: &[ColumnMeta], term: &str) -> String {
    let pattern = dialect.string_literal(&format!("%{}%", escape_like(term)));
    let hex = term.strip_prefix("0x").or_else(|| term.strip_prefix("0X")).unwrap_or(term);
    let hex_pattern = (!hex.is_empty() && hex.chars().all(|character| character.is_ascii_hexdigit()))
        .then(|| format!("'%{}%'", hex.to_ascii_lowercase()));

    let conditions: Vec<String> = columns
        .iter()
        .filter_map(|column| {
            let name = quote_identifier(&column.name);
            match column.category {
                ValueCategory::Binary => hex_pattern.as_ref().map(|hex| format!("encode({name}, 'hex') ILIKE {hex}")),
                ValueCategory::Spatial => hex_pattern.as_ref().map(|hex| format!("CAST({name} AS text) ILIKE {hex}")),
                _ => Some(format!("CAST({name} AS text) ILIKE {pattern} ESCAPE '!'")),
            }
        })
        .collect();

    if conditions.is_empty() {
        "FALSE".to_string()
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
    dialect: Dialect,
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
                    Some(value) => format!("{name} = {}", dialect.value_literal(Some(&value), meta.category)),
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
            literals.push(dialect.value_literal(Some(&value), column(index)?.category));
        }
        Ok(Some(match literals.as_slice() {
            [single] => single.clone(),
            many => format!("({})", many.join(", ")),
        }))
    };

    let keyed = |head: &str, rows: &[KeyedRow]| -> AppResult<String> {
        if let [row] = rows {
            return Ok(format!("{head} WHERE {}", where_clause(&row.key)?));
        }
        let tuples: Vec<&str> = rows.iter().map(|row| row.tuple.as_str()).collect();
        Ok(format!("{head} WHERE {key_target} IN ({})", tuples.join(", ")))
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
                            dialect.value_literal(edit.value.as_deref(), meta.category)
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
                    .map(|edit| column(edit.column).map(|meta| dialect.value_literal(edit.value.as_deref(), meta.category)))
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
            database: Some("app".to_string()),
            table: Some("t".to_string()),
            source_name: Some(name.to_string()),
            has_choices: false,
        }
    }

    #[test]
    fn groups_changes_without_limits() {
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
        ];

        let statements = row_changes_sql(Dialect::default(), "app", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![
            r#"UPDATE "app"."t" SET "rating" = NULL WHERE "id" IN (1, 15)"#,
            r#"UPDATE "app"."t" SET "rating" = 'pg' WHERE "id" = 16"#,
            r#"DELETE FROM "app"."t" WHERE "id" IN (19, 20)"#,
            r#"INSERT INTO "app"."t" ("rating") VALUES ('r'), ('g')"#,
        ]);
    }

    #[test]
    fn inserts_default_rows() {
        let columns = vec![column("id", ValueCategory::Integer, true)];
        let changes = vec![RowChange::Insert { values: Vec::new() }];
        let statements = row_changes_sql(Dialect::default(), "app", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![r#"INSERT INTO "app"."t" DEFAULT VALUES"#]);
    }

    #[test]
    fn writes_typed_literals() {
        let dialect = Dialect::default();
        assert_eq!(dialect.value_literal(Some("42"), ValueCategory::Integer), "42");
        assert_eq!(dialect.value_literal(Some("NaN"), ValueCategory::Float), "'NaN'");
        assert_eq!(dialect.value_literal(Some("0xBEEF"), ValueCategory::Binary), "decode('BEEF', 'hex')");
        assert_eq!(dialect.value_literal(Some("0x0101"), ValueCategory::Spatial), "'0101'");
        assert_eq!(dialect.value_literal(Some("true"), ValueCategory::Boolean), "'true'");
        assert_eq!(dialect.value_literal(None, ValueCategory::Text), "NULL");
    }

    #[test]
    fn escapes_strings_for_both_settings() {
        assert_eq!(Dialect::default().string_literal(r"it's a \ test"), r"'it''s a \ test'");
        let legacy = Dialect { standard_conforming_strings: false };
        assert_eq!(legacy.string_literal(r"a\b"), r"E'a\\b'");
        assert_eq!(legacy.string_literal("plain"), "'plain'");
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(qualified("my schema", r#"a"b"#), r#""my schema"."a""b""#);
    }

    #[test]
    fn builds_search_conditions() {
        let columns = vec![
            column("id", ValueCategory::Integer, true),
            column("data", ValueCategory::Binary, false),
        ];
        assert_eq!(
            search_condition(Dialect::default(), &columns, "Ab"),
            r#"CAST("id" AS text) ILIKE '%Ab%' ESCAPE '!' OR encode("data", 'hex') ILIKE '%ab%'"#
        );
        assert_eq!(search_condition(Dialect::default(), &columns[1..], "zz"), "FALSE");
    }
}
