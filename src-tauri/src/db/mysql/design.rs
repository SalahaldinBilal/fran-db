use mysql_async::{Conn, Row, prelude::Queryable};

use super::metadata::{name, non_empty, number, text};
use super::sqlgen::{Dialect, plain_word, qualified, quote_identifier};
use crate::db::types::{
    CharsetInfo, CollationInfo, ColumnDesign, DesignCapabilities, DesignOptions, ForeignKeyDesign, IndexColumnDesign,
    IndexDesign, IndexKind, TableDesign,
};
use crate::error::{AppError, AppResult};

const NUMERIC_TYPES: [&str; 11] = [
    "tinyint", "smallint", "mediumint", "int", "integer", "bigint", "decimal", "numeric", "float", "double", "bit",
];
const REFERENTIAL_ACTIONS: [&str; 5] = ["RESTRICT", "CASCADE", "SET NULL", "NO ACTION", "SET DEFAULT"];

pub async fn design_options(conn: &mut Conn) -> Result<DesignOptions, mysql_async::Error> {
    let engines: Vec<Row> = conn
        .query("SELECT ENGINE, SUPPORT FROM information_schema.ENGINES WHERE SUPPORT IN ('YES', 'DEFAULT') ORDER BY ENGINE")
        .await?;
    let charsets: Vec<Row> = conn
        .query("SELECT CHARACTER_SET_NAME, DEFAULT_COLLATE_NAME FROM information_schema.CHARACTER_SETS ORDER BY CHARACTER_SET_NAME")
        .await?;
    let collations: Vec<Row> = conn
        .query("SELECT COLLATION_NAME, CHARACTER_SET_NAME FROM information_schema.COLLATIONS ORDER BY COLLATION_NAME")
        .await?;

    Ok(DesignOptions {
        index_methods: Vec::new(),
        capabilities: DesignCapabilities {
            engines: true,
            charsets: true,
            column_collations: true,
            on_update: true,
            index_lengths: true,
            reorder_columns: true,
            virtual_columns: true,
            unsigned: true,
            zerofill: true,
        },
        default_engine: engines
            .iter()
            .find(|row| name(row, 1).eq_ignore_ascii_case("DEFAULT"))
            .map(|row| name(row, 0)),
        engines: engines.iter().map(|row| name(row, 0)).collect(),
        charsets: charsets
            .iter()
            .map(|row| CharsetInfo {
                name: name(row, 0),
                default_collation: name(row, 1),
            })
            .collect(),
        collations: collations
            .iter()
            .filter_map(|row| {
                Some(CollationInfo {
                    name: name(row, 0),
                    charset: text(row, 1)?,
                })
            })
            .collect(),
    })
}

fn default_for_mysql(dialect: Dialect, raw: Option<String>, extra: &str, data_type: &str) -> Option<String> {
    let raw = raw?;

    if extra.to_ascii_lowercase().contains("default_generated") {
        let upper = raw.to_ascii_uppercase();
        let is_time_function = ["CURRENT_TIMESTAMP", "NOW(", "LOCALTIME", "CURRENT_DATE", "CURRENT_TIME"]
            .iter()
            .any(|prefix| upper.starts_with(prefix));
        return Some(if is_time_function { raw } else { format!("({raw})") });
    }

    if NUMERIC_TYPES.contains(&data_type.to_ascii_lowercase().as_str()) {
        return Some(raw);
    }

    Some(dialect.string_literal(&raw))
}

fn on_update_clause(extra: &str) -> Option<String> {
    let position = extra.to_ascii_lowercase().find("on update ")?;
    non_empty(Some(extra[position + "on update ".len()..].trim().to_string()))
}

pub async fn load_design(
    conn: &mut Conn,
    dialect: Dialect,
    mariadb: bool,
    database: &str,
    table: &str,
) -> Result<Option<TableDesign>, mysql_async::Error> {
    let tables: Vec<Row> = conn
        .exec(
            "SELECT t.ENGINE, t.TABLE_COLLATION, t.TABLE_COMMENT, c.CHARACTER_SET_NAME \
             FROM information_schema.TABLES t \
             LEFT JOIN information_schema.COLLATIONS c ON c.COLLATION_NAME = t.TABLE_COLLATION \
             WHERE t.TABLE_SCHEMA = ? AND t.TABLE_NAME = ?",
            (database, table),
        )
        .await?;

    let Some(table_row) = tables.first() else {
        return Ok(None);
    };

    let column_rows: Vec<Row> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT, \
                    COLLATION_NAME, GENERATION_EXPRESSION \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await?;

    let columns = column_rows
        .iter()
        .map(|row| {
            let extra = name(row, 5);
            let generated_expression = non_empty(text(row, 8)).map(|expression| expression.replace("\\`", "`"));
            let raw_default = text(row, 4);
            let default_value = if generated_expression.is_some() {
                None
            } else if mariadb {
                raw_default.filter(|value| value != "NULL")
            } else {
                default_for_mysql(dialect, raw_default, &extra, &name(row, 2))
            };

            ColumnDesign {
                original_name: Some(name(row, 0)),
                name: name(row, 0),
                data_type: name(row, 1),
                nullable: name(row, 3).eq_ignore_ascii_case("YES"),
                default_value,
                auto_increment: extra.to_ascii_lowercase().contains("auto_increment"),
                on_update: on_update_clause(&extra),
                comment: name(row, 6),
                collation: non_empty(text(row, 7)),
                generated_stored: generated_expression.is_some() && extra.to_ascii_uppercase().contains("STORED"),
                generated_expression,
            }
        })
        .collect();

    let index_rows: Vec<Row> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, INDEX_TYPE, SUB_PART FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
             ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX",
            (database, table),
        )
        .await?;

    let mut indexes: Vec<IndexDesign> = Vec::new();
    for row in &index_rows {
        let index_name = name(row, 0);
        let Some(column_name) = text(row, 2) else {
            continue;
        };
        let column = IndexColumnDesign {
            name: column_name,
            length: number(row, 4).and_then(|length| u32::try_from(length).ok()),
        };

        match indexes.last_mut() {
            Some(last) if last.name == index_name => last.columns.push(column),
            _ => {
                let index_type = name(row, 3).to_ascii_uppercase();
                let kind = if index_name == "PRIMARY" {
                    IndexKind::Primary
                } else if index_type == "FULLTEXT" {
                    IndexKind::Fulltext
                } else if index_type == "SPATIAL" {
                    IndexKind::Spatial
                } else if number(row, 1) == Some(0) {
                    IndexKind::Unique
                } else {
                    IndexKind::Index
                };
                indexes.push(IndexDesign {
                    original_name: Some(index_name.clone()),
                    name: index_name,
                    kind,
                    columns: vec![column],
                    method: None,
                    constraint: false,
                });
            }
        }
    }

    let key_rows: Vec<Row> = conn
        .exec(
            "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, \
                    k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE \
             FROM information_schema.KEY_COLUMN_USAGE k \
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME \
             WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            (database, table),
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
                on_update: name(row, 5),
                on_delete: name(row, 6),
            }),
        }
    }

    Ok(Some(TableDesign {
        name: table.to_string(),
        engine: non_empty(text(table_row, 0)),
        collation: non_empty(text(table_row, 1)),
        comment: name(table_row, 2),
        charset: non_empty(text(table_row, 3)),
        columns,
        indexes,
        foreign_keys,
    }))
}

fn is_textual(data_type: &str) -> bool {
    let lower = data_type.to_ascii_lowercase();
    ["char", "text", "enum", "set"].iter().any(|word| lower.contains(word))
}

fn column_definition(dialect: Dialect, column: &ColumnDesign, table_collation: Option<&str>) -> AppResult<String> {
    if column.name.trim().is_empty() {
        return Err(AppError::general("Every column needs a name"));
    }
    if column.data_type.trim().is_empty() {
        return Err(AppError::general(format!("Column {} needs a type", column.name)));
    }

    let mut parts = vec![quote_identifier(column.name.trim()), column.data_type.trim().to_string()];

    if let Some(expression) = column.generated_expression.as_deref() {
        if expression.trim().is_empty() {
            return Err(AppError::general(format!("{} is a generated column but has no expression", column.name)));
        }
        let storage = if column.generated_stored { "STORED" } else { "VIRTUAL" };
        parts.push(format!("GENERATED ALWAYS AS ({}) {storage}", expression.trim()));
    }

    if let Some(collation) = column.collation.as_deref()
        && is_textual(&column.data_type)
        && Some(collation) != table_collation
    {
        parts.push(format!("COLLATE {}", plain_word(collation, "collation")?));
    }

    parts.push(if column.nullable { "NULL" } else { "NOT NULL" }.to_string());

    if column.generated_expression.is_none()
        && let Some(default) = column.default_value.as_deref().filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("DEFAULT {}", default.trim()));
    }

    if let Some(on_update) = column.on_update.as_deref().filter(|value| !value.trim().is_empty()) {
        parts.push(format!("ON UPDATE {}", on_update.trim()));
    }

    if column.auto_increment {
        parts.push("AUTO_INCREMENT".to_string());
    }

    if !column.comment.is_empty() {
        parts.push(format!("COMMENT {}", dialect.string_literal(&column.comment)));
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
        .map(|column| match column.length {
            Some(length) => format!("{}({length})", quote_identifier(&column.name)),
            None => quote_identifier(&column.name),
        })
        .collect::<Vec<_>>()
        .join(", "))
}

fn index_definition(index: &IndexDesign, adding: bool) -> AppResult<String> {
    let columns = index_columns(index)?;
    let keyword = if adding { "INDEX" } else { "KEY" };
    let named = |prefix: &str| -> AppResult<String> {
        if index.name.trim().is_empty() {
            return Err(AppError::general("Every index needs a name"));
        }
        Ok(format!("{prefix} {keyword} {} ({columns})", quote_identifier(index.name.trim())))
    };

    let definition = match index.kind {
        IndexKind::Primary => format!("PRIMARY KEY ({columns})"),
        IndexKind::Unique => named("UNIQUE")?,
        IndexKind::Index => named("")?.trim_start().to_string(),
        IndexKind::Fulltext => named("FULLTEXT")?,
        IndexKind::Spatial => named("SPATIAL")?,
    };

    Ok(if adding { format!("ADD {definition}") } else { definition })
}

fn referential_action(action: &str) -> AppResult<String> {
    let upper = action.trim().to_ascii_uppercase();
    if REFERENTIAL_ACTIONS.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(AppError::general(format!("Unknown referential action: {action}")))
    }
}

fn foreign_key_definition(database: &str, key: &ForeignKeyDesign) -> AppResult<String> {
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
    let referenced_database = if key.referenced_database.trim().is_empty() {
        database
    } else {
        key.referenced_database.trim()
    };

    Ok(format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
        quote_identifier(key.name.trim()),
        list(&key.columns),
        qualified(referenced_database, key.referenced_table.trim()),
        list(&key.referenced_columns),
        referential_action(&key.on_delete)?,
        referential_action(&key.on_update)?,
    ))
}

fn table_options(dialect: Dialect, design: &TableDesign) -> AppResult<Vec<String>> {
    let mut options = Vec::new();
    if let Some(engine) = design.engine.as_deref() {
        options.push(format!("ENGINE = {}", plain_word(engine, "engine")?));
    }
    if let Some(charset) = design.charset.as_deref() {
        options.push(format!("DEFAULT CHARACTER SET = {}", plain_word(charset, "character set")?));
    }
    if let Some(collation) = design.collation.as_deref() {
        options.push(format!("COLLATE = {}", plain_word(collation, "collation")?));
    }
    if !design.comment.is_empty() {
        options.push(format!("COMMENT = {}", dialect.string_literal(&design.comment)));
    }
    Ok(options)
}

pub fn create_sql(dialect: Dialect, database: &str, design: &TableDesign) -> AppResult<Vec<String>> {
    if design.name.trim().is_empty() {
        return Err(AppError::general("The table needs a name"));
    }
    if design.columns.is_empty() {
        return Err(AppError::general("The table needs at least one column"));
    }

    let mut lines = design
        .columns
        .iter()
        .map(|column| column_definition(dialect, column, design.collation.as_deref()))
        .collect::<AppResult<Vec<_>>>()?;
    for index in &design.indexes {
        lines.push(index_definition(index, false)?);
    }
    for key in &design.foreign_keys {
        lines.push(foreign_key_definition(database, key)?);
    }

    let options = table_options(dialect, design)?;
    let mut sql = format!(
        "CREATE TABLE {} (\n  {}\n)",
        qualified(database, design.name.trim()),
        lines.join(",\n  ")
    );
    if !options.is_empty() {
        sql.push('\n');
        sql.push_str(&options.join("\n"));
    }

    Ok(vec![sql])
}

fn same_column(original: &ColumnDesign, edited: &ColumnDesign) -> bool {
    ColumnDesign {
        original_name: original.original_name.clone(),
        ..edited.clone()
    } == *original
}

pub fn alter_sql(
    dialect: Dialect,
    database: &str,
    original: &TableDesign,
    design: &TableDesign,
) -> AppResult<Vec<String>> {
    if design.name.trim().is_empty() {
        return Err(AppError::general("The table needs a name"));
    }

    let target = qualified(database, &original.name);
    let kept = |original_name: &str| design.columns.iter().any(|column| column.original_name.as_deref() == Some(original_name));
    let survivors: Vec<&str> = original
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .filter(|name| kept(name))
        .collect();
    let original_predecessor = |name: &str| -> Option<&str> {
        let position = survivors.iter().position(|survivor| *survivor == name)?;
        position.checked_sub(1).map(|previous| survivors[previous])
    };

    let mut clauses = Vec::new();

    for column in &original.columns {
        if !kept(&column.name) {
            clauses.push(format!("DROP COLUMN {}", quote_identifier(&column.name)));
        }
    }

    let changed_indexes: Vec<(&IndexDesign, Option<&IndexDesign>)> = design
        .indexes
        .iter()
        .map(|index| {
            let source = index
                .original_name
                .as_deref()
                .and_then(|name| original.indexes.iter().find(|existing| existing.name == name));
            (index, source)
        })
        .collect();

    for existing in &original.indexes {
        let survives_unchanged = changed_indexes
            .iter()
            .any(|(index, source)| source.is_some_and(|source| source.name == existing.name && same_index(source, index)));
        if !survives_unchanged {
            clauses.push(match existing.kind {
                IndexKind::Primary => "DROP PRIMARY KEY".to_string(),
                _ => format!("DROP INDEX {}", quote_identifier(&existing.name)),
            });
        }
    }

    let mut previous_new_name: Option<&str> = None;
    let mut previous_existing: Option<&str> = None;
    for column in &design.columns {
        let position = match previous_new_name {
            Some(previous) => format!("AFTER {}", quote_identifier(previous.trim())),
            None => "FIRST".to_string(),
        };
        let definition = column_definition(dialect, column, design.collation.as_deref())?;

        match column.original_name.as_deref() {
            None => clauses.push(format!("ADD COLUMN {definition} {position}")),
            Some(original_name) => {
                let source = original
                    .columns
                    .iter()
                    .find(|existing| existing.name == original_name)
                    .ok_or_else(|| AppError::general(format!("Column {original_name} no longer exists")))?;
                let moved = original_predecessor(original_name) != previous_existing;
                if moved {
                    clauses.push(format!("CHANGE COLUMN {} {definition} {position}", quote_identifier(original_name)));
                } else if !same_column(source, column) {
                    clauses.push(format!("CHANGE COLUMN {} {definition}", quote_identifier(original_name)));
                }
                previous_existing = Some(original_name);
            }
        }
        previous_new_name = Some(column.name.as_str());
    }

    for (index, source) in &changed_indexes {
        let unchanged = source.is_some_and(|source| same_index(source, index));
        if !unchanged {
            clauses.push(index_definition(index, true)?);
        }
    }

    let options = TableDesign {
        engine: design.engine.clone().filter(|engine| Some(engine) != original.engine.as_ref()),
        charset: design.charset.clone().filter(|charset| Some(charset) != original.charset.as_ref()),
        collation: design.collation.clone().filter(|collation| Some(collation) != original.collation.as_ref()),
        comment: String::new(),
        ..design.clone()
    };
    clauses.extend(table_options(dialect, &options)?);
    if design.comment != original.comment {
        clauses.push(format!("COMMENT = {}", dialect.string_literal(&design.comment)));
    }
    if design.name.trim() != original.name {
        clauses.push(format!("RENAME TO {}", qualified(database, design.name.trim())));
    }

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

    let dropped_keys: Vec<String> = original
        .foreign_keys
        .iter()
        .filter(|existing| {
            !changed_keys
                .iter()
                .any(|(key, unchanged)| *unchanged && key.original_name.as_deref() == Some(existing.name.as_str()))
        })
        .map(|existing| format!("DROP FOREIGN KEY {}", quote_identifier(&existing.name)))
        .collect();
    if !dropped_keys.is_empty() {
        statements.push(format!("ALTER TABLE {target}\n  {}", dropped_keys.join(",\n  ")));
    }

    if !clauses.is_empty() {
        statements.push(format!("ALTER TABLE {target}\n  {}", clauses.join(",\n  ")));
    }

    let final_target = qualified(database, design.name.trim());
    let added_keys = changed_keys
        .iter()
        .filter(|(_, unchanged)| !unchanged)
        .map(|(key, _)| foreign_key_definition(database, key).map(|definition| format!("ADD {definition}")))
        .collect::<AppResult<Vec<_>>>()?;
    if !added_keys.is_empty() {
        statements.push(format!("ALTER TABLE {final_target}\n  {}", added_keys.join(",\n  ")));
    }

    Ok(statements)
}

fn same_index(original: &IndexDesign, edited: &IndexDesign) -> bool {
    original.name == edited.name.trim() && original.kind == edited.kind && original.columns == edited.columns
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

    fn table() -> TableDesign {
        let mut id = column("id", "int");
        id.nullable = false;
        id.auto_increment = true;
        TableDesign {
            name: "users".to_string(),
            comment: String::new(),
            engine: Some("InnoDB".to_string()),
            charset: Some("utf8mb4".to_string()),
            collation: Some("utf8mb4_0900_ai_ci".to_string()),
            columns: vec![id, column("name", "varchar(100)"), column("email", "varchar(200)")],
            indexes: vec![IndexDesign {
                original_name: Some("PRIMARY".to_string()),
                name: "PRIMARY".to_string(),
                kind: IndexKind::Primary,
                columns: vec![IndexColumnDesign { name: "id".to_string(), length: None }],
                method: None,
                constraint: false,
            }],
            foreign_keys: Vec::new(),
        }
    }

    #[test]
    fn creates_tables() {
        let sql = create_sql(Dialect::default(), "app", &table()).unwrap();
        assert_eq!(
            sql[0],
            "CREATE TABLE `app`.`users` (\n  `id` int NOT NULL AUTO_INCREMENT,\n  `name` varchar(100) NULL,\n  \
             `email` varchar(200) NULL,\n  PRIMARY KEY (`id`)\n)\nENGINE = InnoDB\nDEFAULT CHARACTER SET = utf8mb4\n\
             COLLATE = utf8mb4_0900_ai_ci"
        );
    }

    #[test]
    fn unchanged_design_produces_nothing() {
        assert!(alter_sql(Dialect::default(), "app", &table(), &table()).unwrap().is_empty());
    }

    #[test]
    fn alters_renamed_added_and_dropped_columns() {
        let original = table();
        let mut design = table();
        design.columns[1].name = "full_name".to_string();
        design.columns.remove(2);
        let mut added = column("age", "int");
        added.original_name = None;
        design.columns.insert(1, added);

        let sql = alter_sql(Dialect::default(), "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE `app`.`users`\n  DROP COLUMN `email`,\n  ADD COLUMN `age` int NULL AFTER `id`,\n  \
                 CHANGE COLUMN `name` `full_name` varchar(100) NULL"
                    .to_string()
            ]
        );
    }

    #[test]
    fn moves_columns() {
        let original = table();
        let mut design = table();
        design.columns.swap(1, 2);

        let sql = alter_sql(Dialect::default(), "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec![
                "ALTER TABLE `app`.`users`\n  CHANGE COLUMN `email` `email` varchar(200) NULL AFTER `id`,\n  \
                 CHANGE COLUMN `name` `name` varchar(100) NULL AFTER `email`"
                    .to_string()
            ]
        );
    }

    #[test]
    fn rebuilds_changed_indexes() {
        let original = table();
        let mut design = table();
        design.indexes.push(IndexDesign {
            original_name: None,
            name: "email_unique".to_string(),
            kind: IndexKind::Unique,
            columns: vec![IndexColumnDesign { name: "email".to_string(), length: Some(50) }],
            method: None,
            constraint: false,
        });

        let sql = alter_sql(Dialect::default(), "app", &original, &design).unwrap();
        assert_eq!(
            sql,
            vec!["ALTER TABLE `app`.`users`\n  ADD UNIQUE INDEX `email_unique` (`email`(50))".to_string()]
        );
    }
}
