use super::client::{PgConnection, PgError};
use super::metadata::{flag, name, non_empty, text};
use super::sqlgen::{Dialect, qualified, quote_identifier};
use crate::db::types::{
    CollationInfo, ColumnDesign, DesignCapabilities, DesignOptions, ForeignKeyDesign, IndexColumnDesign, IndexDesign,
    IndexKind, TableDesign,
};
use crate::error::{AppError, AppResult};

const REFERENTIAL_ACTIONS: [&str; 5] = ["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"];
/// The designer's own name for a primary key it hasn't named, so Postgres picks `<table>_pkey`.
const UNNAMED_PRIMARY: &str = "PRIMARY";

/// What the generated SQL may rely on.
#[derive(Debug, Clone, Copy)]
pub struct Server {
    pub dialect: Dialect,
    pub major_version: u32,
}

pub fn major_version(version: &str) -> u32 {
    version
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|major| major.parse().ok())
        .unwrap_or(0)
}

pub async fn design_options(conn: &mut PgConnection, major_version: u32) -> Result<DesignOptions, PgError> {
    let collations = conn
        .query_text(
            "SELECT collname FROM pg_collation \
             WHERE collencoding IN (-1, (SELECT encoding FROM pg_database WHERE datname = current_database())) \
             ORDER BY collname",
            &[],
        )
        .await?;
    let methods = conn
        .query_text("SELECT amname FROM pg_am WHERE amtype = 'i' ORDER BY amname", &[])
        .await?;

    Ok(DesignOptions {
        engines: Vec::new(),
        default_engine: None,
        charsets: Vec::new(),
        collations: collations
            .iter()
            .map(|row| CollationInfo {
                name: name(row, 0),
                charset: String::new(),
            })
            .collect(),
        index_methods: methods.iter().map(|row| name(row, 0)).collect(),
        capabilities: DesignCapabilities {
            engines: false,
            charsets: false,
            column_collations: true,
            on_update: false,
            index_lengths: false,
            reorder_columns: false,
            virtual_columns: major_version >= 18,
            unsigned: false,
            zerofill: false,
        },
    })
}

fn referential_action(code: &str) -> String {
    match code {
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => "NO ACTION",
    }
    .to_string()
}

/// Expression, partial and exclusion indexes don't fit the designer's column lists, so they're left out
/// of the design and the designer never touches them.
pub async fn load_design(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Option<TableDesign>, PgError> {
    let tables = conn
        .query_text(
            "SELECT c.oid::text, obj_description(c.oid, 'pg_class') FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')",
            &[Some(schema), Some(table)],
        )
        .await?;
    let Some(table_row) = tables.first() else {
        return Ok(None);
    };
    let oid = name(table_row, 0);

    let column_rows = conn
        .query_text(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, pg_get_expr(d.adbin, d.adrelid), \
                    a.attidentity::text, a.attgenerated::text, \
                    CASE WHEN a.attcollation <> t.typcollation THEN co.collname END, \
                    col_description(a.attrelid, a.attnum) \
             FROM pg_attribute a \
             JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             LEFT JOIN pg_collation co ON co.oid = a.attcollation \
             WHERE a.attrelid = $1::oid AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[Some(&oid)],
        )
        .await?;

    let columns = column_rows
        .iter()
        .map(|row| {
            let expression = text(row, 3);
            let identity = !name(row, 4).is_empty();
            let generated = name(row, 5);
            let is_generated = !generated.is_empty();

            ColumnDesign {
                original_name: Some(name(row, 0)),
                name: name(row, 0),
                data_type: name(row, 1),
                nullable: !flag(row, 2),
                default_value: if is_generated || identity { None } else { expression.clone() },
                auto_increment: identity,
                on_update: None,
                comment: text(row, 7).unwrap_or_default(),
                collation: non_empty(text(row, 6)),
                generated_expression: if is_generated { expression } else { None },
                generated_stored: generated == "s",
            }
        })
        .collect();

    let index_rows = conn
        .query_text(
            "SELECT i.relname, x.indisprimary, x.indisunique, am.amname, con.oid IS NOT NULL, a.attname, \
                    x.indexprs IS NOT NULL OR x.indpred IS NOT NULL OR con.contype = 'x' \
             FROM pg_index x \
             JOIN pg_class i ON i.oid = x.indexrelid \
             JOIN pg_am am ON am.oid = i.relam \
             LEFT JOIN pg_constraint con ON con.conindid = x.indexrelid AND con.contype IN ('p', 'u', 'x') \
             CROSS JOIN LATERAL generate_series(1, x.indnkeyatts::int) AS k(n) \
             LEFT JOIN pg_attribute a ON a.attrelid = x.indrelid AND a.attnum = x.indkey[k.n - 1] \
             WHERE x.indrelid = $1::oid \
             ORDER BY x.indisprimary DESC, i.relname, k.n",
            &[Some(&oid)],
        )
        .await?;

    let mut indexes: Vec<IndexDesign> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for row in &index_rows {
        let index_name = name(row, 0);
        if skipped.contains(&index_name) {
            continue;
        }
        let Some(column) = text(row, 5).filter(|_| !flag(row, 6)) else {
            indexes.retain(|index| index.name != index_name);
            skipped.push(index_name);
            continue;
        };
        let column = IndexColumnDesign { name: column, length: None };

        match indexes.last_mut() {
            Some(last) if last.name == index_name => last.columns.push(column),
            _ => {
                let kind = if flag(row, 1) {
                    IndexKind::Primary
                } else if flag(row, 2) {
                    IndexKind::Unique
                } else {
                    IndexKind::Index
                };
                indexes.push(IndexDesign {
                    original_name: Some(index_name.clone()),
                    name: index_name,
                    kind,
                    columns: vec![column],
                    method: non_empty(text(row, 3)),
                    constraint: flag(row, 4),
                });
            }
        }
    }

    let key_rows = conn
        .query_text(
            "SELECT con.conname, a.attname, rn.nspname, rc.relname, ra.attname, \
                    con.confupdtype::text, con.confdeltype::text \
             FROM pg_constraint con \
             JOIN pg_class rc ON rc.oid = con.confrelid \
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             CROSS JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(col, refcol, ord) \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.col \
             JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = k.refcol \
             WHERE con.contype = 'f' AND con.conrelid = $1::oid \
             ORDER BY con.conname, k.ord",
            &[Some(&oid)],
        )
        .await?;

    let mut foreign_keys: Vec<ForeignKeyDesign> = Vec::new();
    for row in &key_rows {
        let constraint = name(row, 0);
        match foreign_keys.last_mut() {
            Some(last) if last.name == constraint => {
                last.columns.push(name(row, 1));
                last.referenced_columns.push(name(row, 4));
            }
            _ => foreign_keys.push(ForeignKeyDesign {
                original_name: Some(constraint.clone()),
                name: constraint,
                columns: vec![name(row, 1)],
                referenced_database: name(row, 2),
                referenced_table: name(row, 3),
                referenced_columns: vec![name(row, 4)],
                on_update: referential_action(&name(row, 5)),
                on_delete: referential_action(&name(row, 6)),
            }),
        }
    }

    Ok(Some(TableDesign {
        name: table.to_string(),
        comment: text(table_row, 1).unwrap_or_default(),
        engine: None,
        charset: None,
        collation: None,
        columns,
        indexes,
        foreign_keys,
    }))
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// The generation expression, refusing a column that's marked generated but has none.
fn generation(column: &ColumnDesign) -> AppResult<Option<&str>> {
    match column.generated_expression.as_deref().map(str::trim) {
        Some("") => Err(AppError::general(format!(
            "{} is a generated column but has no expression",
            column.name
        ))),
        other => Ok(other),
    }
}

/// Index methods go into DDL unquoted, so only plain words are allowed.
fn plain_word(value: &str, what: &str) -> AppResult<String> {
    let value = value.trim();
    if !value.is_empty() && value.chars().all(|character| character.is_ascii_alphanumeric() || character == '_') {
        Ok(value.to_string())
    } else {
        Err(AppError::general(format!("Invalid {what}: {value}")))
    }
}

fn comment_literal(dialect: Dialect, comment: &str) -> String {
    if comment.is_empty() {
        "NULL".to_string()
    } else {
        dialect.string_literal(comment)
    }
}

fn column_definition(server: Server, column: &ColumnDesign) -> AppResult<String> {
    let column_name = column.name.trim();
    if column_name.is_empty() {
        return Err(AppError::general("Every column needs a name"));
    }
    let data_type = column.data_type.trim();
    if data_type.is_empty() {
        return Err(AppError::general(format!("Column {column_name} needs a type")));
    }

    let mut parts = vec![quote_identifier(column_name), data_type.to_string()];

    if let Some(collation) = trimmed(column.collation.as_deref()) {
        parts.push(format!("COLLATE {}", quote_identifier(collation)));
    }

    if let Some(expression) = generation(column)? {
        if !column.generated_stored && server.major_version < 18 {
            return Err(AppError::general(format!(
                "{column_name} is a virtual generated column, which needs PostgreSQL 18 or newer. Make it stored."
            )));
        }
        let storage = if column.generated_stored { "STORED" } else { "VIRTUAL" };
        parts.push(format!("GENERATED ALWAYS AS ({expression}) {storage}"));
    } else if column.auto_increment {
        parts.push("GENERATED BY DEFAULT AS IDENTITY".to_string());
    } else if let Some(default) = trimmed(column.default_value.as_deref()) {
        parts.push(format!("DEFAULT {default}"));
    }

    if !column.nullable {
        parts.push("NOT NULL".to_string());
    }

    Ok(parts.join(" "))
}

fn index_columns(index: &IndexDesign) -> AppResult<String> {
    if index.columns.is_empty() {
        return Err(AppError::general(format!("Index {} has no columns", index.name)));
    }
    Ok(index
        .columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", "))
}

fn primary_key_clause(index: &IndexDesign) -> AppResult<String> {
    let columns = index_columns(index)?;
    let index_name = index.name.trim();
    if index_name.is_empty() || index_name.eq_ignore_ascii_case(UNNAMED_PRIMARY) {
        Ok(format!("PRIMARY KEY ({columns})"))
    } else {
        Ok(format!("CONSTRAINT {} PRIMARY KEY ({columns})", quote_identifier(index_name)))
    }
}

/// The statement that creates a non-primary index on `target`.
fn create_index(target: &str, index: &IndexDesign) -> AppResult<String> {
    let unique = match index.kind {
        IndexKind::Unique => "UNIQUE ",
        IndexKind::Index => "",
        IndexKind::Primary => return Err(AppError::general("A primary key isn't a standalone index")),
        IndexKind::Fulltext | IndexKind::Spatial => {
            return Err(AppError::general(
                "Postgres has no fulltext or spatial index kinds. Use a GIN or GiST index instead.",
            ));
        }
    };
    let index_name = index.name.trim();
    if index_name.is_empty() {
        return Err(AppError::general("Every index needs a name"));
    }
    let using = match trimmed(index.method.as_deref()) {
        Some(method) => format!(" USING {}", plain_word(method, "index method")?),
        None => String::new(),
    };

    Ok(format!(
        "CREATE {unique}INDEX {} ON {target}{using} ({})",
        quote_identifier(index_name),
        index_columns(index)?
    ))
}

fn add_index(target: &str, index: &IndexDesign) -> AppResult<String> {
    match index.kind {
        IndexKind::Primary => Ok(format!("ALTER TABLE {target} ADD {}", primary_key_clause(index)?)),
        _ => create_index(target, index),
    }
}

fn drop_index(schema: &str, target: &str, index: &IndexDesign) -> String {
    if index.constraint || index.kind == IndexKind::Primary {
        format!("ALTER TABLE {target} DROP CONSTRAINT {}", quote_identifier(&index.name))
    } else {
        format!("DROP INDEX {}", qualified(schema, &index.name))
    }
}

fn referential(action: &str) -> AppResult<String> {
    let upper = action.trim().to_ascii_uppercase();
    if REFERENTIAL_ACTIONS.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(AppError::general(format!("Unknown referential action: {action}")))
    }
}

fn foreign_key_definition(schema: &str, key: &ForeignKeyDesign) -> AppResult<String> {
    if key.name.trim().is_empty() {
        return Err(AppError::general("Every foreign key needs a name"));
    }
    if key.columns.is_empty() || key.columns.len() != key.referenced_columns.len() {
        return Err(AppError::general(format!(
            "Foreign key {} needs the same number of columns on both sides",
            key.name
        )));
    }

    let list = |names: &[String]| names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(", ");
    let referenced_schema = trimmed(Some(key.referenced_database.as_str())).unwrap_or(schema);

    Ok(format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
        quote_identifier(key.name.trim()),
        list(&key.columns),
        qualified(referenced_schema, key.referenced_table.trim()),
        list(&key.referenced_columns),
        referential(&key.on_delete)?,
        referential(&key.on_update)?,
    ))
}

fn column_comments(dialect: Dialect, target: &str, columns: &[&ColumnDesign]) -> Vec<String> {
    columns
        .iter()
        .map(|column| {
            format!(
                "COMMENT ON COLUMN {target}.{} IS {}",
                quote_identifier(column.name.trim()),
                comment_literal(dialect, &column.comment)
            )
        })
        .collect()
}

pub fn create_sql(server: Server, schema: &str, design: &TableDesign) -> AppResult<Vec<String>> {
    let table_name = design.name.trim();
    if table_name.is_empty() {
        return Err(AppError::general("The table needs a name"));
    }
    if design.columns.is_empty() {
        return Err(AppError::general("The table needs at least one column"));
    }

    let target = qualified(schema, table_name);
    let mut lines = design
        .columns
        .iter()
        .map(|column| column_definition(server, column))
        .collect::<AppResult<Vec<_>>>()?;
    for index in design.indexes.iter().filter(|index| index.kind == IndexKind::Primary) {
        lines.push(primary_key_clause(index)?);
    }
    for key in &design.foreign_keys {
        lines.push(foreign_key_definition(schema, key)?);
    }

    let mut statements = vec![format!("CREATE TABLE {target} (\n  {}\n)", lines.join(",\n  "))];
    for index in design.indexes.iter().filter(|index| index.kind != IndexKind::Primary) {
        statements.push(create_index(&target, index)?);
    }
    if !design.comment.is_empty() {
        statements.push(format!(
            "COMMENT ON TABLE {target} IS {}",
            comment_literal(server.dialect, &design.comment)
        ));
    }
    let commented: Vec<&ColumnDesign> = design.columns.iter().filter(|column| !column.comment.is_empty()).collect();
    statements.extend(column_comments(server.dialect, &target, &commented));

    Ok(statements)
}

fn same_type(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

/// `ALTER COLUMN` actions that turn `source` into `column`. Renames are handled separately.
fn column_changes(server: Server, source: &ColumnDesign, column: &ColumnDesign) -> AppResult<Vec<String>> {
    let column_name = quote_identifier(column.name.trim());
    let data_type = column.data_type.trim();
    let mut actions = Vec::new();

    let type_changed = !same_type(&source.data_type, data_type);
    let collation = trimmed(column.collation.as_deref());
    if type_changed || trimmed(source.collation.as_deref()) != collation {
        if data_type.is_empty() {
            return Err(AppError::general(format!("Column {} needs a type", column.name)));
        }
        let mut action = format!("ALTER COLUMN {column_name} TYPE {data_type}");
        if let Some(collation) = collation {
            action.push_str(&format!(" COLLATE {}", quote_identifier(collation)));
        }
        if type_changed {
            action.push_str(&format!(" USING {column_name}::{data_type}"));
        }
        actions.push(action);
    }

    let before = trimmed(source.generated_expression.as_deref());
    let after = generation(column)?;
    match (before, after) {
        (None, Some(_)) => {
            return Err(AppError::general(format!(
                "Postgres can't turn the existing column {} into a generated one. Remove it and add a new column.",
                column.name
            )));
        }
        (Some(_), None) => {
            if server.major_version < 13 {
                return Err(AppError::general("Removing a generation expression needs PostgreSQL 13 or newer"));
            }
            actions.push(format!("ALTER COLUMN {column_name} DROP EXPRESSION"));
        }
        (Some(old), Some(new)) => {
            if source.generated_stored != column.generated_stored {
                return Err(AppError::general(format!(
                    "Postgres can't switch {} between stored and virtual. Remove it and add a new column.",
                    column.name
                )));
            }
            if old != new {
                if server.major_version < 17 {
                    return Err(AppError::general("Changing a generation expression needs PostgreSQL 17 or newer"));
                }
                actions.push(format!("ALTER COLUMN {column_name} SET EXPRESSION AS ({new})"));
            }
        }
        (None, None) => {}
    }

    if source.auto_increment != column.auto_increment {
        if column.auto_increment {
            if trimmed(source.default_value.as_deref()).is_some() {
                actions.push(format!("ALTER COLUMN {column_name} DROP DEFAULT"));
            }
            actions.push(format!("ALTER COLUMN {column_name} ADD GENERATED BY DEFAULT AS IDENTITY"));
        } else {
            actions.push(format!("ALTER COLUMN {column_name} DROP IDENTITY"));
        }
    }

    let default = trimmed(column.default_value.as_deref());
    let default_applies = after.is_none() && !column.auto_increment;
    let default_was_dropped = column.auto_increment && !source.auto_increment;
    if default_applies && !default_was_dropped && trimmed(source.default_value.as_deref()) != default {
        actions.push(match default {
            Some(default) => format!("ALTER COLUMN {column_name} SET DEFAULT {default}"),
            None => format!("ALTER COLUMN {column_name} DROP DEFAULT"),
        });
    }

    if source.nullable != column.nullable {
        let change = if column.nullable { "DROP NOT NULL" } else { "SET NOT NULL" };
        actions.push(format!("ALTER COLUMN {column_name} {change}"));
    }

    Ok(actions)
}

fn same_index(original: &IndexDesign, edited: &IndexDesign) -> bool {
    original.name == edited.name.trim()
        && original.kind == edited.kind
        && original.columns == edited.columns
        && trimmed(original.method.as_deref()) == trimmed(edited.method.as_deref())
}

fn same_key(original: &ForeignKeyDesign, edited: &ForeignKeyDesign) -> bool {
    original.name == edited.name.trim()
        && original.columns == edited.columns
        && original.referenced_database == edited.referenced_database
        && original.referenced_table == edited.referenced_table
        && original.referenced_columns == edited.referenced_columns
        && original.on_update.eq_ignore_ascii_case(&edited.on_update)
        && original.on_delete.eq_ignore_ascii_case(&edited.on_delete)
}

/// New columns are always added at the end, Postgres can't place them between existing ones.
pub fn alter_sql(server: Server, schema: &str, original: &TableDesign, design: &TableDesign) -> AppResult<Vec<String>> {
    let new_name = design.name.trim();
    if new_name.is_empty() {
        return Err(AppError::general("The table needs a name"));
    }

    let old_target = qualified(schema, &original.name);
    let target = qualified(schema, new_name);
    let kept = |name: &str| design.columns.iter().any(|column| column.original_name.as_deref() == Some(name));
    let mut statements = Vec::new();

    let changed_keys: Vec<(&ForeignKeyDesign, bool)> = design
        .foreign_keys
        .iter()
        .map(|key| {
            let unchanged = key.original_name.as_deref().is_some_and(|name| {
                original
                    .foreign_keys
                    .iter()
                    .any(|existing| existing.name == name && same_key(existing, key))
            });
            (key, unchanged)
        })
        .collect();
    for existing in &original.foreign_keys {
        let survives = changed_keys
            .iter()
            .any(|(key, unchanged)| *unchanged && key.original_name.as_deref() == Some(existing.name.as_str()));
        if !survives {
            statements.push(format!(
                "ALTER TABLE {old_target} DROP CONSTRAINT {}",
                quote_identifier(&existing.name)
            ));
        }
    }

    let changed_indexes: Vec<(&IndexDesign, bool)> = design
        .indexes
        .iter()
        .map(|index| {
            let unchanged = index.original_name.as_deref().is_some_and(|name| {
                original
                    .indexes
                    .iter()
                    .any(|existing| existing.name == name && same_index(existing, index))
            });
            (index, unchanged)
        })
        .collect();
    for existing in &original.indexes {
        let survives = changed_indexes
            .iter()
            .any(|(index, unchanged)| *unchanged && index.original_name.as_deref() == Some(existing.name.as_str()));
        if !survives {
            statements.push(drop_index(schema, &old_target, existing));
        }
    }

    let dropped: Vec<String> = original
        .columns
        .iter()
        .filter(|column| !kept(&column.name))
        .map(|column| format!("DROP COLUMN {}", quote_identifier(&column.name)))
        .collect();
    if !dropped.is_empty() {
        statements.push(format!("ALTER TABLE {old_target}\n  {}", dropped.join(",\n  ")));
    }

    if new_name != original.name {
        statements.push(format!("ALTER TABLE {old_target} RENAME TO {}", quote_identifier(new_name)));
    }

    let mut actions = Vec::new();
    let mut comment_changes: Vec<&ColumnDesign> = Vec::new();
    for column in &design.columns {
        match column.original_name.as_deref() {
            None => {
                actions.push(format!("ADD COLUMN {}", column_definition(server, column)?));
                if !column.comment.is_empty() {
                    comment_changes.push(column);
                }
            }
            Some(original_name) => {
                let source = original
                    .columns
                    .iter()
                    .find(|existing| existing.name == original_name)
                    .ok_or_else(|| AppError::general(format!("Column {original_name} no longer exists")))?;
                let column_name = column.name.trim();
                if column_name.is_empty() {
                    return Err(AppError::general("Every column needs a name"));
                }
                if column_name != original_name {
                    statements.push(format!(
                        "ALTER TABLE {target} RENAME COLUMN {} TO {}",
                        quote_identifier(original_name),
                        quote_identifier(column_name)
                    ));
                }
                actions.extend(column_changes(server, source, column)?);
                if source.comment != column.comment {
                    comment_changes.push(column);
                }
            }
        }
    }
    if !actions.is_empty() {
        statements.push(format!("ALTER TABLE {target}\n  {}", actions.join(",\n  ")));
    }

    if design.comment != original.comment {
        statements.push(format!(
            "COMMENT ON TABLE {target} IS {}",
            comment_literal(server.dialect, &design.comment)
        ));
    }
    statements.extend(column_comments(server.dialect, &target, &comment_changes));

    for (index, unchanged) in &changed_indexes {
        if !unchanged {
            statements.push(add_index(&target, index)?);
        }
    }

    for (key, unchanged) in &changed_keys {
        if !unchanged {
            statements.push(format!("ALTER TABLE {target} ADD {}", foreign_key_definition(schema, key)?));
        }
    }

    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: Server = Server {
        dialect: Dialect {
            standard_conforming_strings: true,
        },
        major_version: 17,
    };

    fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original_name: Some(name.to_string()),
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            auto_increment: false,
            on_update: None,
            comment: String::new(),
            collation: None,
            generated_expression: None,
            generated_stored: false,
        }
    }

    fn table() -> TableDesign {
        let mut id = column("id", "integer");
        id.nullable = false;
        id.auto_increment = true;
        TableDesign {
            name: "users".to_string(),
            comment: String::new(),
            engine: None,
            charset: None,
            collation: None,
            columns: vec![id, column("name", "text"), column("email", "character varying(200)")],
            indexes: vec![IndexDesign {
                original_name: Some("users_pkey".to_string()),
                name: "users_pkey".to_string(),
                kind: IndexKind::Primary,
                columns: vec![IndexColumnDesign { name: "id".to_string(), length: None }],
                method: Some("btree".to_string()),
                constraint: true,
            }],
            foreign_keys: Vec::new(),
        }
    }

    #[test]
    fn reads_major_versions() {
        assert_eq!(major_version("16.4 (Debian 16.4-1.pgdg120+2)"), 16);
        assert_eq!(major_version("18beta1"), 18);
    }

    #[test]
    fn creates_tables() {
        let mut design = table();
        design.indexes[0].name = UNNAMED_PRIMARY.to_string();
        design.indexes.push(IndexDesign {
            original_name: None,
            name: "users_email".to_string(),
            kind: IndexKind::Unique,
            columns: vec![IndexColumnDesign { name: "email".to_string(), length: None }],
            method: None,
            constraint: false,
        });
        design.comment = "People".to_string();

        let sql = create_sql(SERVER, "app", &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "CREATE TABLE \"app\".\"users\" (\n  \"id\" integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n  \
                 \"name\" text,\n  \"email\" character varying(200),\n  PRIMARY KEY (\"id\")\n)"
                    .to_string(),
                "CREATE UNIQUE INDEX \"users_email\" ON \"app\".\"users\" (\"email\")".to_string(),
                "COMMENT ON TABLE \"app\".\"users\" IS 'People'".to_string(),
            ]
        );
    }

    #[test]
    fn unchanged_design_produces_nothing() {
        assert!(alter_sql(SERVER, "app", &table(), &table()).unwrap().is_empty());
    }

    #[test]
    fn renames_retypes_adds_and_drops() {
        let original = table();
        let mut design = table();
        design.name = "people".to_string();
        design.columns[1].name = "full_name".to_string();
        design.columns[2].data_type = "text".to_string();
        design.columns[2].nullable = false;
        design.columns.remove(1);
        let mut added = column("age", "integer");
        added.original_name = None;
        added.default_value = Some("0".to_string());
        design.columns.push(added);

        let sql = alter_sql(SERVER, "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE \"app\".\"users\"\n  DROP COLUMN \"name\"".to_string(),
                "ALTER TABLE \"app\".\"users\" RENAME TO \"people\"".to_string(),
                "ALTER TABLE \"app\".\"people\"\n  ALTER COLUMN \"email\" TYPE text USING \"email\"::text,\n  \
                 ALTER COLUMN \"email\" SET NOT NULL,\n  ADD COLUMN \"age\" integer DEFAULT 0"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn renames_columns_in_their_own_statement() {
        let original = table();
        let mut design = table();
        design.columns[1].name = "full_name".to_string();
        design.columns[1].comment = "Shown on invoices".to_string();

        let sql = alter_sql(SERVER, "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE \"app\".\"users\" RENAME COLUMN \"name\" TO \"full_name\"".to_string(),
                "COMMENT ON COLUMN \"app\".\"users\".\"full_name\" IS 'Shown on invoices'".to_string(),
            ]
        );
    }

    #[test]
    fn rebuilds_changed_indexes_and_keys() {
        let original = table();
        let mut design = table();
        design.indexes[0].columns.push(IndexColumnDesign { name: "email".to_string(), length: None });
        design.indexes.push(IndexDesign {
            original_name: None,
            name: "users_name_trgm".to_string(),
            kind: IndexKind::Index,
            columns: vec![IndexColumnDesign { name: "name".to_string(), length: None }],
            method: Some("gin".to_string()),
            constraint: false,
        });

        let sql = alter_sql(SERVER, "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE \"app\".\"users\" DROP CONSTRAINT \"users_pkey\"".to_string(),
                "ALTER TABLE \"app\".\"users\" ADD CONSTRAINT \"users_pkey\" PRIMARY KEY (\"id\", \"email\")".to_string(),
                "CREATE INDEX \"users_name_trgm\" ON \"app\".\"users\" USING gin (\"name\")".to_string(),
            ]
        );
    }

    #[test]
    fn switches_identity_and_defaults() {
        let original = table();
        let mut design = table();
        design.columns[0].auto_increment = false;
        design.columns[0].default_value = Some("nextval('users_id_seq')".to_string());

        let sql = alter_sql(SERVER, "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE \"app\".\"users\"\n  ALTER COLUMN \"id\" DROP IDENTITY,\n  \
                 ALTER COLUMN \"id\" SET DEFAULT nextval('users_id_seq')"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn refuses_what_postgres_cannot_do() {
        let original = table();
        let mut design = table();
        design.columns[1].generated_expression = Some("upper(email)".to_string());
        assert!(alter_sql(SERVER, "app", &original, &design).is_err());

        let mut virtual_column = column("total", "integer");
        virtual_column.original_name = None;
        virtual_column.generated_expression = Some("1 + 1".to_string());
        let mut design = table();
        design.columns.push(virtual_column);
        let old_server = Server { major_version: 16, ..SERVER };
        assert!(alter_sql(old_server, "app", &original, &design).is_err());

        let mut empty_generated = column("total", "integer");
        empty_generated.original_name = None;
        empty_generated.generated_expression = Some("  ".to_string());
        empty_generated.generated_stored = true;
        let mut design = table();
        design.columns.push(empty_generated);
        assert!(alter_sql(SERVER, "app", &original, &design).is_err());
    }
}
