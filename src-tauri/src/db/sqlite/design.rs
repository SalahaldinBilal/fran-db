use std::collections::HashMap;

use super::connection::SqliteConnection;
use super::definition;
use super::metadata::{self, PRIMARY_INDEX, foreign_key_name};
use super::sqlgen::{qualified, quote_identifier};
use crate::db::types::{
    CollationInfo, ColumnDesign, DesignCapabilities, DesignOptions, ForeignKeyDesign, IndexColumnDesign, IndexDesign,
    IndexKind, TableDesign,
};
use crate::error::{AppError, AppResult};

const REFERENTIAL_ACTIONS: [&str; 5] = ["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"];
const COLLATIONS: [&str; 3] = ["BINARY", "NOCASE", "RTRIM"];

pub fn design_options() -> DesignOptions {
    DesignOptions {
        engines: Vec::new(),
        default_engine: None,
        charsets: Vec::new(),
        collations: COLLATIONS
            .iter()
            .map(|collation| CollationInfo {
                name: collation.to_string(),
                charset: String::new(),
            })
            .collect(),
        index_methods: Vec::new(),
        capabilities: DesignCapabilities {
            engines: false,
            charsets: false,
            column_collations: true,
            on_update: false,
            index_lengths: false,
            reorder_columns: false,
            virtual_columns: true,
            unsigned: false,
            zerofill: false,
        },
    }
}

/// Expression and partial indexes don't fit the designer's column lists, so they're left out
/// and the designer never touches them.
pub async fn load_design(conn: &dyn SqliteConnection, schema: &str, table: &str) -> AppResult<Option<TableDesign>> {
    let Some(create_sql) = metadata::stored_sql(conn, schema, "table", table).await? else {
        return Ok(None);
    };
    if create_sql.split_whitespace().nth(1).is_some_and(|word| word.eq_ignore_ascii_case("VIRTUAL")) {
        return Err(AppError::general("Virtual tables can't be edited in the designer"));
    }

    let table_columns = metadata::table_columns(conn, schema, table).await?;
    let clauses = definition::column_clauses(&create_sql);
    let clause_of = |column: &str| clauses.iter().find(|clause| clause.name.eq_ignore_ascii_case(column));

    let columns = table_columns
        .iter()
        .map(|column| {
            let clause = clause_of(&column.name);
            let generated = column.hidden >= 2;
            ColumnDesign {
                original_name: Some(column.name.clone()),
                name: column.name.clone(),
                data_type: column.data_type.clone(),
                nullable: !column.not_null,
                default_value: if generated { None } else { column.default_value.clone() },
                auto_increment: clause.is_some_and(|clause| clause.autoincrement),
                on_update: None,
                comment: String::new(),
                collation: clause.and_then(|clause| clause.collation.clone()),
                generated_expression: if generated { clause.and_then(|clause| clause.generated.clone()) } else { None },
                generated_stored: column.hidden == 3,
            }
        })
        .collect();

    let mut indexes = Vec::new();
    let key = metadata::key_columns(&table_columns);
    if !key.is_empty() {
        indexes.push(IndexDesign {
            original_name: Some(PRIMARY_INDEX.to_string()),
            name: PRIMARY_INDEX.to_string(),
            kind: IndexKind::Primary,
            columns: key.into_iter().map(|name| IndexColumnDesign { name, length: None }).collect(),
            method: None,
            constraint: true,
        });
    }

    for index in metadata::index_rows(conn, schema, table).await? {
        let Some(columns) = index.columns.into_iter().collect::<Option<Vec<_>>>().filter(|_| !index.partial) else {
            continue;
        };
        indexes.push(IndexDesign {
            original_name: Some(index.name.clone()),
            name: index.name,
            kind: if index.unique { IndexKind::Unique } else { IndexKind::Index },
            columns: columns.into_iter().map(|name| IndexColumnDesign { name, length: None }).collect(),
            method: None,
            constraint: index.origin == "u",
        });
    }

    let foreign_keys = metadata::foreign_key_rows(conn, schema, table)
        .await?
        .into_iter()
        .map(|key| {
            let key_name = foreign_key_name(table, key.id);
            ForeignKeyDesign {
                original_name: Some(key_name.clone()),
                name: key_name,
                columns: key.columns,
                referenced_database: schema.to_string(),
                referenced_table: key.referenced_table,
                referenced_columns: key.referenced_columns,
                on_update: key.on_update,
                on_delete: key.on_delete,
            }
        })
        .collect();

    Ok(Some(TableDesign {
        name: table.to_string(),
        comment: String::new(),
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

/// SQLite takes literals after DEFAULT as they are, anything else has to be in parentheses.
fn default_clause(value: &str) -> String {
    let upper = value.to_ascii_uppercase();
    let is_number = value
        .trim_start_matches(['-', '+'])
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
        && value.chars().any(|character| character.is_ascii_digit());
    let is_literal = is_number
        || value.starts_with('\'')
        || value.starts_with('(')
        || ((upper.starts_with("X'")) && value.ends_with('\''))
        || ["NULL", "TRUE", "FALSE", "CURRENT_TIME", "CURRENT_DATE", "CURRENT_TIMESTAMP"].contains(&upper.as_str());
    if is_literal { format!("DEFAULT {value}") } else { format!("DEFAULT ({value})") }
}

fn column_definition(column: &ColumnDesign, inline_key: bool) -> AppResult<String> {
    let column_name = column.name.trim();
    if column_name.is_empty() {
        return Err(AppError::general("Every column needs a name"));
    }

    let mut parts = vec![quote_identifier(column_name)];
    if let Some(data_type) = trimmed(Some(&column.data_type)) {
        parts.push(data_type.to_string());
    }
    if inline_key {
        parts.push("PRIMARY KEY AUTOINCREMENT".to_string());
    }
    if !column.nullable {
        parts.push("NOT NULL".to_string());
    }
    if let Some(collation) = trimmed(column.collation.as_deref()) {
        parts.push(format!("COLLATE {}", quote_identifier(collation)));
    }

    match column.generated_expression.as_deref().map(str::trim) {
        Some("") => {
            return Err(AppError::general(format!(
                "{column_name} is a generated column but has no expression"
            )));
        }
        Some(expression) => {
            let storage = if column.generated_stored { "STORED" } else { "VIRTUAL" };
            parts.push(format!("GENERATED ALWAYS AS ({expression}) {storage}"));
        }
        None => {
            if let Some(default) = trimmed(column.default_value.as_deref()) {
                parts.push(default_clause(default));
            }
        }
    }

    Ok(parts.join(" "))
}

fn list(names: &[String]) -> String {
    names.iter().map(|name| quote_identifier(name.trim())).collect::<Vec<_>>().join(", ")
}

fn index_column_names(index: &IndexDesign) -> AppResult<Vec<String>> {
    if index.columns.is_empty() {
        return Err(AppError::general(format!("Index {} has no columns", index.name)));
    }
    Ok(index.columns.iter().map(|column| column.name.clone()).collect())
}

/// Indexes live in the table's database, so the index name carries the schema and the table doesn't.
fn create_index(schema: &str, table: &str, index: &IndexDesign) -> AppResult<String> {
    let unique = match index.kind {
        IndexKind::Unique => "UNIQUE ",
        IndexKind::Index => "",
        IndexKind::Primary => return Err(AppError::general("A primary key isn't a standalone index")),
        IndexKind::Fulltext | IndexKind::Spatial => {
            return Err(AppError::general(
                "SQLite has no fulltext or spatial indexes. Use an FTS5 or R*Tree virtual table instead.",
            ));
        }
    };
    let index_name = index.name.trim();
    if index_name.is_empty() {
        return Err(AppError::general("Every index needs a name"));
    }
    Ok(format!(
        "CREATE {unique}INDEX {} ON {} ({})",
        qualified(schema, index_name),
        quote_identifier(table),
        list(&index_column_names(index)?)
    ))
}

fn referential(action: &str) -> AppResult<String> {
    let upper = action.trim().to_ascii_uppercase();
    if REFERENTIAL_ACTIONS.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(AppError::general(format!("Unknown referential action: {action}")))
    }
}

/// Foreign keys can't point into another attached database, so the parent stays unqualified.
fn foreign_key_definition(schema: &str, key: &ForeignKeyDesign) -> AppResult<String> {
    if key.columns.is_empty() || key.columns.len() != key.referenced_columns.len() {
        return Err(AppError::general(format!(
            "Foreign key {} needs the same number of columns on both sides",
            key.name
        )));
    }
    if trimmed(Some(&key.referenced_database)).is_some_and(|database| database != schema) {
        return Err(AppError::general("A SQLite foreign key can only point at a table in the same database"));
    }
    let constraint = match trimmed(Some(&key.name)) {
        Some(key_name) => format!("CONSTRAINT {} ", quote_identifier(key_name)),
        None => String::new(),
    };
    Ok(format!(
        "{constraint}FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
        list(&key.columns),
        quote_identifier(key.referenced_table.trim()),
        list(&key.referenced_columns),
        referential(&key.on_delete)?,
        referential(&key.on_update)?,
    ))
}

pub fn create_sql(schema: &str, design: &TableDesign) -> AppResult<Vec<String>> {
    let table_name = design.name.trim();
    if table_name.is_empty() {
        return Err(AppError::general("The table needs a name"));
    }
    if design.columns.is_empty() {
        return Err(AppError::general("A table needs at least one column"));
    }

    let primary = design.indexes.iter().find(|index| index.kind == IndexKind::Primary);
    let key_names = primary.map(index_column_names).transpose()?.unwrap_or_default();

    let auto: Vec<&ColumnDesign> = design.columns.iter().filter(|column| column.auto_increment).collect();
    let inline_key = match auto.as_slice() {
        [] => None,
        [column]
            if key_names.len() == 1
                && key_names[0] == column.name.trim()
                && column.data_type.trim().eq_ignore_ascii_case("INTEGER") =>
        {
            Some(column.name.trim())
        }
        _ => {
            return Err(AppError::general(
                "AUTOINCREMENT only works on a single INTEGER column that is the whole primary key",
            ));
        }
    };

    let mut lines = design
        .columns
        .iter()
        .map(|column| column_definition(column, inline_key == Some(column.name.trim())))
        .collect::<AppResult<Vec<_>>>()?;
    if inline_key.is_none() && !key_names.is_empty() {
        lines.push(format!("PRIMARY KEY ({})", list(&key_names)));
    }
    for key in &design.foreign_keys {
        lines.push(foreign_key_definition(schema, key)?);
    }

    let mut statements = vec![format!(
        "CREATE TABLE {} (\n    {}\n)",
        qualified(schema, table_name),
        lines.join(",\n    ")
    )];
    for index in design.indexes.iter().filter(|index| index.kind != IndexKind::Primary) {
        statements.push(create_index(schema, table_name, index)?);
    }
    Ok(statements)
}

/// Anything ALTER TABLE can't do needs a rebuild, which isn't generated because dropping the old table
/// fires ON DELETE actions on tables that reference it.
pub fn alter_sql(schema: &str, original: &TableDesign, edited: &TableDesign) -> AppResult<Vec<String>> {
    let table_name = edited.name.trim();
    if table_name.is_empty() {
        return Err(AppError::general("The table needs a name"));
    }

    let mut statements = Vec::new();
    let mut unsupported: Vec<String> = Vec::new();

    if table_name != original.name {
        statements.push(format!(
            "ALTER TABLE {} RENAME TO {}",
            qualified(schema, &original.name),
            quote_identifier(table_name)
        ));
    }
    if edited.comment.trim() != original.comment.trim() {
        unsupported.push("table comments".to_string());
    }
    let target = qualified(schema, table_name);

    let mut renamed: HashMap<&str, &str> = HashMap::new();
    let mut added = Vec::new();
    for column in &edited.columns {
        let Some(original_name) = column.original_name.as_deref() else {
            added.push(column);
            continue;
        };
        let before = original
            .columns
            .iter()
            .find(|candidate| candidate.name == original_name)
            .ok_or_else(|| AppError::general(format!("Column {original_name} is no longer in the table")))?;
        let new_name = column.name.trim();
        if new_name.is_empty() {
            return Err(AppError::general("Every column needs a name"));
        }
        renamed.insert(original_name, new_name);
        if new_name != original_name {
            statements.push(format!(
                "ALTER TABLE {target} RENAME COLUMN {} TO {}",
                quote_identifier(original_name),
                quote_identifier(new_name)
            ));
        }
        let comparable = |design: &ColumnDesign| {
            (
                design.data_type.trim().to_string(),
                design.nullable,
                trimmed(design.default_value.as_deref()).map(str::to_string),
                design.auto_increment,
                trimmed(design.collation.as_deref()).map(str::to_ascii_uppercase),
                trimmed(design.generated_expression.as_deref()).map(str::to_string),
                design.generated_stored,
                design.comment.trim().to_string(),
            )
        };
        if comparable(column) != comparable(before) {
            unsupported.push(format!("changing column {new_name}"));
        }
    }

    let rename = |name: &str| renamed.get(name).map_or_else(|| name.to_string(), |new| new.to_string());

    let primary_of = |design: &TableDesign, map: bool| -> Vec<String> {
        design
            .indexes
            .iter()
            .find(|index| index.kind == IndexKind::Primary)
            .map(|index| {
                index
                    .columns
                    .iter()
                    .map(|column| if map { rename(&column.name) } else { column.name.clone() })
                    .collect()
            })
            .unwrap_or_default()
    };
    if primary_of(original, true) != primary_of(edited, false) {
        unsupported.push("the primary key".to_string());
    }

    let key_shape = |key: &ForeignKeyDesign, map: bool| {
        (
            key.columns.iter().map(|column| if map { rename(column) } else { column.clone() }).collect::<Vec<_>>(),
            key.referenced_table.clone(),
            key.referenced_columns.clone(),
            key.on_update.to_ascii_uppercase(),
            key.on_delete.to_ascii_uppercase(),
        )
    };
    let original_keys: Vec<_> = original.foreign_keys.iter().map(|key| key_shape(key, true)).collect();
    let edited_keys: Vec<_> = edited.foreign_keys.iter().map(|key| key_shape(key, false)).collect();
    if original_keys != edited_keys {
        unsupported.push("foreign keys".to_string());
    }

    let mut drop_indexes = Vec::new();
    let mut create_indexes = Vec::new();
    let secondary = |design: &TableDesign| -> Vec<IndexDesign> {
        design.indexes.iter().filter(|index| index.kind != IndexKind::Primary).cloned().collect()
    };
    let edited_indexes = secondary(edited);
    for before in secondary(original) {
        let after = edited_indexes
            .iter()
            .find(|index| index.original_name.as_deref() == Some(before.name.as_str()));
        let mapped = IndexDesign {
            columns: before
                .columns
                .iter()
                .map(|column| IndexColumnDesign {
                    name: rename(&column.name),
                    length: column.length,
                })
                .collect(),
            ..before.clone()
        };
        let unchanged = after.is_some_and(|after| {
            after.name.trim() == mapped.name && after.kind == mapped.kind && after.columns == mapped.columns
        });
        if unchanged {
            continue;
        }
        if before.constraint {
            unsupported.push(format!("the UNIQUE constraint {}", before.name));
            continue;
        }
        drop_indexes.push(format!("DROP INDEX {}", qualified(schema, &before.name)));
        if let Some(after) = after {
            create_indexes.push(create_index(schema, table_name, after)?);
        }
    }
    for index in edited_indexes.iter().filter(|index| index.original_name.is_none()) {
        create_indexes.push(create_index(schema, table_name, index)?);
    }

    statements.extend(drop_indexes);
    for before in &original.columns {
        if !renamed.contains_key(before.name.as_str()) {
            statements.push(format!("ALTER TABLE {target} DROP COLUMN {}", quote_identifier(&before.name)));
        }
    }
    for column in added {
        if column.auto_increment {
            unsupported.push(format!("adding {} as an AUTOINCREMENT column", column.name.trim()));
            continue;
        }
        statements.push(format!("ALTER TABLE {target} ADD COLUMN {}", column_definition(column, false)?));
    }
    statements.extend(create_indexes);

    if !unsupported.is_empty() {
        return Err(AppError::general(format!(
            "SQLite can't change {} in place. That takes rebuilding the table: create a new table, \
             copy the rows over, drop the old one and rename the new one.",
            unsupported.join(", ")
        )));
    }

    Ok(statements)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn index(name: &str, kind: IndexKind, columns: &[&str]) -> IndexDesign {
        IndexDesign {
            original_name: Some(name.to_string()),
            name: name.to_string(),
            kind,
            columns: columns
                .iter()
                .map(|column| IndexColumnDesign { name: column.to_string(), length: None })
                .collect(),
            method: None,
            constraint: kind == IndexKind::Primary,
        }
    }

    fn table() -> TableDesign {
        let mut id = column("id", "INTEGER");
        id.nullable = false;
        id.auto_increment = true;
        TableDesign {
            name: "users".to_string(),
            comment: String::new(),
            engine: None,
            charset: None,
            collation: None,
            columns: vec![id, column("email", "TEXT"), column("age", "INTEGER")],
            indexes: vec![
                index(PRIMARY_INDEX, IndexKind::Primary, &["id"]),
                index("users_email", IndexKind::Unique, &["email"]),
            ],
            foreign_keys: Vec::new(),
        }
    }

    #[test]
    fn creates_tables() {
        let mut design = table();
        design.columns[2].default_value = Some("abs(-1)".to_string());
        design.columns[1].collation = Some("NOCASE".to_string());
        let statements = create_sql("main", &design).unwrap();
        assert_eq!(statements, vec![
            "CREATE TABLE \"main\".\"users\" (\n    \"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,\n    \
             \"email\" TEXT COLLATE \"NOCASE\",\n    \"age\" INTEGER DEFAULT (abs(-1))\n)",
            "CREATE UNIQUE INDEX \"main\".\"users_email\" ON \"users\" (\"email\")",
        ]);
    }

    #[test]
    fn refuses_autoincrement_outside_the_key() {
        let mut design = table();
        design.columns[0].data_type = "BIGINT".to_string();
        assert!(create_sql("main", &design).is_err());
    }

    #[test]
    fn alters_what_sqlite_allows() {
        let original = table();
        let mut edited = table();
        edited.name = "people".to_string();
        edited.columns[1].name = "mail".to_string();
        edited.columns.remove(2);
        edited.columns.push(ColumnDesign {
            original_name: None,
            ..column("nick", "TEXT")
        });
        edited.indexes[1].columns[0].name = "mail".to_string();
        edited.indexes.push(IndexDesign {
            original_name: None,
            ..index("people_nick", IndexKind::Index, &["nick"])
        });

        let statements = alter_sql("main", &original, &edited).unwrap();
        assert_eq!(statements, vec![
            r#"ALTER TABLE "main"."users" RENAME TO "people""#,
            r#"ALTER TABLE "main"."people" RENAME COLUMN "email" TO "mail""#,
            r#"ALTER TABLE "main"."people" DROP COLUMN "age""#,
            r#"ALTER TABLE "main"."people" ADD COLUMN "nick" TEXT"#,
            r#"CREATE INDEX "main"."people_nick" ON "people" ("nick")"#,
        ]);
    }

    #[test]
    fn rebuilds_changed_indexes() {
        let original = table();
        let mut edited = table();
        edited.indexes[1].kind = IndexKind::Index;
        let statements = alter_sql("main", &original, &edited).unwrap();
        assert_eq!(statements, vec![
            r#"DROP INDEX "main"."users_email""#,
            r#"CREATE INDEX "main"."users_email" ON "users" ("email")"#,
        ]);
    }

    #[test]
    fn refuses_changes_that_need_a_rebuild() {
        let original = table();
        let mut edited = table();
        edited.columns[2].data_type = "TEXT".to_string();
        let error = alter_sql("main", &original, &edited).unwrap_err();
        assert!(error.message.contains("changing column age"));
    }
}
