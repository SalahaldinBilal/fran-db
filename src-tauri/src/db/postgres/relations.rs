use std::collections::HashMap;

use super::client::{PgConnection, PgError};
use super::metadata::{flag, name, text};

/// Where a result column comes from, when it's a plain column of a table or view.
#[derive(Debug, Clone)]
pub struct SourceColumn {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub not_null: bool,
    pub primary_key: bool,
}

pub async fn lookup(
    conn: &mut PgConnection,
    keys: &[(u32, i16)],
) -> Result<HashMap<(u32, i16), SourceColumn>, PgError> {
    let mut tables: Vec<u32> = keys.iter().map(|(table, _)| *table).collect();
    tables.sort_unstable();
    tables.dedup();
    if tables.is_empty() {
        return Ok(HashMap::new());
    }

    let list = format!("{{{}}}", tables.iter().map(u32::to_string).collect::<Vec<_>>().join(","));
    let rows = conn
        .query_text(
            "SELECT a.attrelid::text, a.attnum::text, n.nspname, c.relname, a.attname, a.attnotnull, \
                    c.relkind IN ('r', 'p') AND EXISTS ( \
                        SELECT 1 FROM pg_index x \
                        WHERE x.indrelid = a.attrelid AND x.indisprimary AND a.attnum = ANY (x.indkey) \
                    ) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE a.attrelid = ANY ($1::oid[]) AND a.attnum > 0 AND NOT a.attisdropped",
            &[Some(&list)],
        )
        .await?;

    let mut sources = HashMap::new();
    for row in &rows {
        let table = text(row, 0).and_then(|value| value.parse::<u32>().ok());
        let column = text(row, 1).and_then(|value| value.parse::<i16>().ok());
        let (Some(table), Some(column)) = (table, column) else {
            continue;
        };
        if !keys.contains(&(table, column)) {
            continue;
        }

        sources.insert(
            (table, column),
            SourceColumn {
                schema: name(row, 2),
                table: name(row, 3),
                column: name(row, 4),
                not_null: flag(row, 5),
                primary_key: flag(row, 6),
            },
        );
    }

    Ok(sources)
}
