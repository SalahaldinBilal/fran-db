use crate::db::read_query::SqlWriter;
use crate::db::types::{ColumnMeta, RowChange, ValueCategory};
use crate::error::{AppError, AppResult};

/// What the server needs to know to write literals correctly for this session.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dialect {
    pub no_backslash_escapes: bool,
}

pub fn quote_identifier(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

pub fn qualified(database: &str, name: &str) -> String {
    format!("{}.{}", quote_identifier(database), quote_identifier(name))
}

impl Dialect {
    pub fn string_literal(&self, value: &str) -> String {
        let mut literal = String::with_capacity(value.len() + 2);
        literal.push('\'');
        for character in value.chars() {
            match character {
                '\'' => literal.push_str("''"),
                '\\' if !self.no_backslash_escapes => literal.push_str("\\\\"),
                '\0' if !self.no_backslash_escapes => literal.push_str("\\0"),
                '\u{1a}' if !self.no_backslash_escapes => literal.push_str("\\Z"),
                other => literal.push(other),
            }
        }
        literal.push('\'');
        literal
    }

    pub fn value_literal(&self, value: Option<&str>, category: ValueCategory) -> String {
        let Some(value) = value else {
            return "NULL".to_string();
        };

        match category {
            ValueCategory::Binary | ValueCategory::Spatial => match hex_body(value) {
                Some(hex) => format!("X'{hex}'"),
                None => self.string_literal(value),
            },
            ValueCategory::Bit if value.parse::<u128>().is_ok() => value.to_string(),
            ValueCategory::Integer | ValueCategory::Decimal | ValueCategory::Float if is_number(value) => {
                value.to_string()
            }
            _ => self.string_literal(value),
        }
    }
}

impl SqlWriter for Dialect {
    fn identifier(&self, name: &str) -> String {
        quote_identifier(name)
    }

    fn table(&self, database: &str, table: &str) -> String {
        qualified(database, table)
    }

    fn string(&self, value: &str) -> String {
        self.string_literal(value)
    }

    fn boolean(&self, value: bool) -> String {
        if value { "TRUE" } else { "FALSE" }.to_string()
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

/// Names like engines and collations go into DDL unquoted, so only plain words are allowed.
pub fn plain_word(value: &str, what: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|character| character.is_ascii_alphanumeric() || character == '_') {
        Ok(trimmed.to_string())
    } else {
        Err(AppError::general(format!("Invalid {what}: {value}")))
    }
}

fn source_identifier(meta: &ColumnMeta) -> AppResult<String> {
    meta.source_name
        .as_deref()
        .map(quote_identifier)
        .ok_or_else(|| AppError::general(format!("{} isn't a table column, so it can't be written", meta.name)))
}

pub fn row_changes_sql(
    dialect: Dialect,
    database: &str,
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

    let target = qualified(database, table);
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
            return Ok(format!("{head} WHERE {} LIMIT 1", where_clause(&row.key)?));
        }
        let tuples: Vec<&str> = rows.iter().map(|row| row.tuple.as_str()).collect();
        Ok(format!("{head} WHERE {key_target} IN ({}) LIMIT {}", tuples.join(", "), rows.len()))
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
                    _ => single_updates.push(format!("{head} WHERE {} LIMIT 1", where_clause(key)?)),
                }
            }
            RowChange::Delete { key } => match key_tuple(key)? {
                Some(tuple) => deletes.push(KeyedRow { key: key.clone(), tuple }),
                None => single_deletes.push(format!("DELETE FROM {target} WHERE {} LIMIT 1", where_clause(key)?)),
            },
            RowChange::Insert { values } => {
                let mut sorted: Vec<_> = values.iter().collect();
                sorted.sort_by_key(|edit| edit.column);
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
        for chunk in rows.chunks(MAX_ROWS_PER_STATEMENT) {
            statements.push(format!("INSERT INTO {target} ({names}) VALUES {}", chunk.join(", ")));
        }
    }

    Ok(statements)
}

const MAX_ROWS_PER_STATEMENT: usize = 500;

struct KeyedRow {
    key: Vec<Option<String>>,
    tuple: String,
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
            database: Some("db".to_string()),
            table: Some("t".to_string()),
            source_name: Some(name.to_string()),
            has_choices: false,
        }
    }

    #[test]
    fn groups_matching_changes() {
        let columns = vec![
            column("id", ValueCategory::Integer, true),
            column("rating", ValueCategory::Text, false),
        ];
        let update = |id: &str, rating: Option<&str>| RowChange::Update {
            key: vec![Some(id.into()), Some("x".into())],
            values: vec![CellEdit { column: 1, value: rating.map(Into::into) }],
        };
        let delete = |id: &str| RowChange::Delete { key: vec![Some(id.into()), None] };
        let insert = |rating: &str| RowChange::Insert { values: vec![CellEdit { column: 1, value: Some(rating.into()) }] };

        let changes = vec![
            update("1", None),
            update("15", None),
            update("16", Some("pg")),
            delete("19"),
            delete("20"),
            insert("r"),
            insert("g"),
        ];

        let statements = row_changes_sql(Dialect::default(), "db", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![
            "UPDATE `db`.`t` SET `rating` = NULL WHERE `id` IN (1, 15) LIMIT 2",
            "UPDATE `db`.`t` SET `rating` = 'pg' WHERE `id` = 16 LIMIT 1",
            "DELETE FROM `db`.`t` WHERE `id` IN (19, 20) LIMIT 2",
            "INSERT INTO `db`.`t` (`rating`) VALUES ('r'), ('g')",
        ]);
    }

    #[test]
    fn groups_composite_keys_and_keeps_key_changes_apart() {
        let columns = vec![
            column("a", ValueCategory::Integer, true),
            column("b", ValueCategory::Text, true),
        ];
        let changes = vec![
            RowChange::Delete { key: vec![Some("1".into()), Some("x".into())] },
            RowChange::Delete { key: vec![Some("2".into()), Some("y".into())] },
            RowChange::Update {
                key: vec![Some("3".into()), Some("z".into())],
                values: vec![CellEdit { column: 0, value: Some("4".into()) }],
            },
            RowChange::Update {
                key: vec![Some("5".into()), Some("w".into())],
                values: vec![CellEdit { column: 0, value: Some("4".into()) }],
            },
        ];

        let statements = row_changes_sql(Dialect::default(), "db", "t", &columns, &changes).unwrap();
        assert_eq!(statements, vec![
            "UPDATE `db`.`t` SET `a` = 4 WHERE `a` = 3 AND `b` = 'z' LIMIT 1",
            "UPDATE `db`.`t` SET `a` = 4 WHERE `a` = 5 AND `b` = 'w' LIMIT 1",
            "DELETE FROM `db`.`t` WHERE (`a`, `b`) IN ((1, 'x'), (2, 'y')) LIMIT 2",
        ]);
    }

    #[test]
    fn writes_source_column_names() {
        let mut aliased = column("label", ValueCategory::Text, false);
        aliased.source_name = Some("name".to_string());
        let columns = vec![column("id", ValueCategory::Integer, true), aliased];
        let changes = vec![RowChange::Update {
            key: vec![Some("1".into()), Some("a".into())],
            values: vec![CellEdit { column: 1, value: Some("b".into()) }],
        }];

        let statements = row_changes_sql(Dialect::default(), "db", "t", &columns, &changes).unwrap();
        assert_eq!(statements[0], "UPDATE `db`.`t` SET `name` = 'b' WHERE `id` = 1 LIMIT 1");
    }

    #[test]
    fn refuses_computed_columns() {
        let mut computed = column("total", ValueCategory::Integer, false);
        computed.source_name = None;
        let columns = vec![column("id", ValueCategory::Integer, true), computed];
        let changes = vec![RowChange::Update {
            key: vec![Some("1".into()), Some("2".into())],
            values: vec![CellEdit { column: 1, value: Some("3".into()) }],
        }];

        assert!(row_changes_sql(Dialect::default(), "db", "t", &columns, &changes).is_err());
    }

    #[test]
    fn escapes_strings() {
        let dialect = Dialect::default();
        assert_eq!(dialect.string_literal(r"it's a \ test"), r"'it''s a \\ test'");
        let strict = Dialect { no_backslash_escapes: true };
        assert_eq!(strict.string_literal(r"a\b"), r"'a\b'");
    }

    #[test]
    fn writes_typed_literals() {
        let dialect = Dialect::default();
        assert_eq!(dialect.value_literal(Some("42"), ValueCategory::Integer), "42");
        assert_eq!(dialect.value_literal(Some("1.5e3"), ValueCategory::Float), "1.5e3");
        assert_eq!(dialect.value_literal(Some("12abc"), ValueCategory::Integer), "'12abc'");
        assert_eq!(dialect.value_literal(Some("0xBEEF"), ValueCategory::Binary), "X'BEEF'");
        assert_eq!(dialect.value_literal(None, ValueCategory::Text), "NULL");
    }

    #[test]
    fn builds_row_changes() {
        let columns = vec![
            column("id", ValueCategory::Integer, true),
            column("name", ValueCategory::Text, false),
        ];
        let changes = vec![
            RowChange::Update {
                key: vec![Some("7".into()), Some("old".into())],
                values: vec![CellEdit { column: 1, value: Some("new".into()) }],
            },
            RowChange::Delete { key: vec![Some("8".into()), None] },
            RowChange::Insert { values: vec![CellEdit { column: 1, value: None }] },
        ];

        let statements = row_changes_sql(Dialect::default(), "db", "t", &columns, &changes).unwrap();
        assert_eq!(statements[0], "UPDATE `db`.`t` SET `name` = 'new' WHERE `id` = 7 LIMIT 1");
        assert_eq!(statements[1], "DELETE FROM `db`.`t` WHERE `id` = 8 LIMIT 1");
        assert_eq!(statements[2], "INSERT INTO `db`.`t` (`name`) VALUES (NULL)");
    }

    #[test]
    fn refuses_tables_without_keys() {
        let columns = vec![column("name", ValueCategory::Text, false)];
        assert!(row_changes_sql(Dialect::default(), "db", "t", &columns, &[]).is_err());
    }
}
