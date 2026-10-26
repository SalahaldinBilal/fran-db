use super::client::{PgConnection, PgError, TextRow};
use super::ddl;
use super::sqlgen::{Dialect, qualified, quote_identifier};
use crate::db::types::{
    CatalogColumn, ColumnChoices, ColumnDefinition, DatabaseInfo, DatabaseSize, ForeignKeyDefinition, IndexDefinition, ObjectInfo,
    ObjectKind, ObjectStructure, SqlTemplate, TableForeignKey,
};
use crate::error::AppError;

const SYSTEM_SCHEMAS: [&str; 2] = ["pg_catalog", "information_schema"];

pub(super) fn text(row: &TextRow, index: usize) -> Option<String> {
    row.get(index).cloned().flatten()
}

pub(super) fn name(row: &TextRow, index: usize) -> String {
    text(row, index).unwrap_or_default()
}

pub(super) fn flag(row: &TextRow, index: usize) -> bool {
    row.get(index).and_then(|value| value.as_deref()) == Some("t")
}

pub(super) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn general(message: impl Into<String>) -> PgError {
    PgError::Other(AppError::general(message))
}

pub async fn list_catalogs(conn: &mut PgConnection) -> Result<Vec<String>, PgError> {
    let rows = conn
        .query_text(
            "SELECT datname FROM pg_database \
             WHERE datallowconn AND NOT datistemplate AND has_database_privilege(datname, 'CONNECT') \
             ORDER BY datname",
            &[],
        )
        .await?;
    Ok(rows.iter().map(|row| name(row, 0)).collect())
}

pub async fn list_databases(conn: &mut PgConnection) -> Result<Vec<DatabaseInfo>, PgError> {
    let rows = conn
        .query_text(
            "SELECT nspname FROM pg_namespace WHERE nspname !~ '^pg_(toast|temp_)' ORDER BY nspname",
            &[],
        )
        .await?;

    Ok(rows
        .iter()
        .map(|row| {
            let schema = name(row, 0);
            DatabaseInfo {
                system: SYSTEM_SCHEMAS.contains(&schema.as_str()),
                name: schema,
            }
        })
        .collect())
}

/// Partitioned parents have no storage of their own, so only plain tables and materialized views are summed.
pub async fn database_sizes(conn: &mut PgConnection) -> Result<Vec<DatabaseSize>, PgError> {
    let rows = conn
        .query_text(
            "SELECT n.nspname, sum(pg_total_relation_size(c.oid))::bigint::text \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('r', 'm') AND n.nspname !~ '^pg_(toast|temp_)' \
             GROUP BY n.nspname",
            &[],
        )
        .await?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let size_bytes = text(row, 1)?.parse().ok()?;
            Some(DatabaseSize { name: name(row, 0), size_bytes })
        })
        .collect())
}

/// `tgtype` bits: 1 row, 2 before, 4 insert, 8 delete, 16 update, 32 truncate, 64 instead.
fn trigger_detail(tgtype: u32, table: &str) -> String {
    let timing = if tgtype & 2 != 0 {
        "BEFORE"
    } else if tgtype & 64 != 0 {
        "INSTEAD OF"
    } else {
        "AFTER"
    };
    let events: Vec<&str> = [(4u32, "INSERT"), (16, "UPDATE"), (8, "DELETE"), (32, "TRUNCATE")]
        .iter()
        .filter(|(bit, _)| tgtype & *bit != 0)
        .map(|(_, event)| *event)
        .collect();
    format!("{timing} {} on {table}", events.join(" OR "))
}

pub async fn list_objects(conn: &mut PgConnection, schema: &str) -> Result<Vec<ObjectInfo>, PgError> {
    let mut objects = Vec::new();

    let relations = conn
        .query_text(
            "SELECT c.relname, c.relkind::text, c.reltuples::bigint::text, \
                    CASE WHEN c.relkind IN ('r', 'p', 'm') THEN pg_total_relation_size(c.oid)::text END, \
                    obj_description(c.oid, 'pg_class') \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f', 'v', 'm', 'S') \
             ORDER BY c.relname",
            &[Some(schema)],
        )
        .await?;

    for row in &relations {
        let relkind = name(row, 1);
        let (kind, detail) = match relkind.as_str() {
            "v" => (ObjectKind::View, None),
            "m" => (ObjectKind::MaterializedView, None),
            "S" => (ObjectKind::Sequence, None),
            "p" => (ObjectKind::Table, Some("partitioned".to_string())),
            "f" => (ObjectKind::Table, Some("foreign".to_string())),
            _ => (ObjectKind::Table, None),
        };
        let stores_rows = matches!(kind, ObjectKind::Table | ObjectKind::MaterializedView);
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind,
            signature: None,
            detail,
            row_estimate: if stores_rows {
                text(row, 2).and_then(|value| value.parse::<i64>().ok()).and_then(|value| u64::try_from(value).ok())
            } else {
                None
            },
            size_bytes: text(row, 3).and_then(|value| value.parse().ok()),
            comment: non_empty(text(row, 4)),
        });
    }

    let routines = conn
        .query_text(
            "SELECT p.proname, p.prokind::text, pg_get_function_identity_arguments(p.oid), \
                    obj_description(p.oid, 'pg_proc') \
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = $1 AND p.prokind IN ('f', 'p') \
               AND NOT EXISTS ( \
                   SELECT 1 FROM pg_depend d \
                   WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e' \
               ) \
             ORDER BY p.proname, 3",
            &[Some(schema)],
        )
        .await?;

    for row in &routines {
        let signature = name(row, 2);
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: if name(row, 1) == "p" { ObjectKind::Procedure } else { ObjectKind::Function },
            detail: Some(format!("({signature})")),
            signature: Some(signature),
            row_estimate: None,
            size_bytes: None,
            comment: non_empty(text(row, 3)),
        });
    }

    let triggers = conn
        .query_text(
            "SELECT t.tgname, c.relname, t.tgtype::int::text, obj_description(t.oid, 'pg_trigger') \
             FROM pg_trigger t \
             JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND NOT t.tgisinternal \
             ORDER BY t.tgname, c.relname",
            &[Some(schema)],
        )
        .await?;

    for row in &triggers {
        let table = name(row, 1);
        let tgtype = text(row, 2).and_then(|value| value.parse().ok()).unwrap_or(0);
        objects.push(ObjectInfo {
            name: name(row, 0),
            kind: ObjectKind::Trigger,
            detail: Some(trigger_detail(tgtype, &table)),
            signature: Some(table),
            row_estimate: None,
            size_bytes: None,
            comment: non_empty(text(row, 3)),
        });
    }

    Ok(objects)
}

pub async fn list_columns(conn: &mut PgConnection, schema: &str) -> Result<Vec<CatalogColumn>, PgError> {
    let rows = conn
        .query_text(
            "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f', 'v', 'm') AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum",
            &[Some(schema)],
        )
        .await?;

    Ok(rows
        .iter()
        .map(|row| CatalogColumn {
            table: name(row, 0),
            name: name(row, 1),
            data_type: name(row, 2),
        })
        .collect())
}

async fn column_definitions(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<ColumnDefinition>, PgError> {
    let rows = conn
        .query_text(
            "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
                    pg_get_expr(d.adbin, d.adrelid), \
                    EXISTS ( \
                        SELECT 1 FROM pg_index x \
                        WHERE x.indrelid = a.attrelid AND x.indisprimary AND a.attnum = ANY (x.indkey) \
                    ), \
                    a.attidentity::text, a.attgenerated::text, col_description(a.attrelid, a.attnum), \
                    CASE WHEN a.attcollation <> t.typcollation THEN co.collname END \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             LEFT JOIN pg_collation co ON co.oid = a.attcollation \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[Some(schema), Some(table)],
        )
        .await?;

    Ok(rows
        .iter()
        .map(|row| {
            let expression = text(row, 3);
            let generated = match name(row, 6).as_str() {
                "s" => Some("STORED"),
                "v" => Some("VIRTUAL"),
                _ => None,
            };
            let extra = match (name(row, 5).as_str(), generated) {
                (_, Some(storage)) => Some(format!(
                    "GENERATED ALWAYS AS ({}) {storage}",
                    expression.clone().unwrap_or_default()
                )),
                ("a", None) => Some("GENERATED ALWAYS AS IDENTITY".to_string()),
                ("d", None) => Some("GENERATED BY DEFAULT AS IDENTITY".to_string()),
                _ => None,
            };

            ColumnDefinition {
                name: name(row, 0),
                data_type: name(row, 1),
                nullable: flag(row, 2),
                default_value: if generated.is_some() { None } else { expression },
                primary_key: flag(row, 4),
                extra,
                comment: non_empty(text(row, 7)),
                collation: non_empty(text(row, 8)),
            }
        })
        .collect())
}

async fn indexes(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<IndexDefinition>, PgError> {
    let rows = conn
        .query_text(
            "SELECT i.relname, x.indisprimary, x.indisunique, am.amname, pg_get_indexdef(x.indexrelid, k.n, true) \
             FROM pg_index x \
             JOIN pg_class i ON i.oid = x.indexrelid \
             JOIN pg_class c ON c.oid = x.indrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_am am ON am.oid = i.relam \
             CROSS JOIN LATERAL generate_series(1, x.indnkeyatts::int) AS k(n) \
             WHERE n.nspname = $1 AND c.relname = $2 \
             ORDER BY x.indisprimary DESC, i.relname, k.n",
            &[Some(schema), Some(table)],
        )
        .await?;

    let mut indexes: Vec<IndexDefinition> = Vec::new();
    for row in &rows {
        let index_name = name(row, 0);
        let column = name(row, 4);
        match indexes.last_mut() {
            Some(last) if last.name == index_name => last.columns.push(column),
            _ => indexes.push(IndexDefinition {
                name: index_name,
                primary: flag(row, 1),
                unique: flag(row, 2),
                columns: vec![column],
                index_type: non_empty(text(row, 3)),
            }),
        }
    }

    Ok(indexes)
}

fn referential_action(code: &str) -> Option<String> {
    Some(
        match code {
            "a" => "NO ACTION",
            "r" => "RESTRICT",
            "c" => "CASCADE",
            "n" => "SET NULL",
            "d" => "SET DEFAULT",
            _ => return None,
        }
        .to_string(),
    )
}

async fn foreign_keys(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<ForeignKeyDefinition>, PgError> {
    let rows = conn
        .query_text(
            "SELECT con.conname, a.attname, rn.nspname, rc.relname, ra.attname, \
                    con.confupdtype::text, con.confdeltype::text \
             FROM pg_constraint con \
             JOIN pg_class c ON c.oid = con.conrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_class rc ON rc.oid = con.confrelid \
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             CROSS JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(col, refcol, ord) \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.col \
             JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = k.refcol \
             WHERE con.contype = 'f' AND n.nspname = $1 AND c.relname = $2 \
             ORDER BY con.conname, k.ord",
            &[Some(schema), Some(table)],
        )
        .await?;

    let mut keys: Vec<ForeignKeyDefinition> = Vec::new();
    for row in &rows {
        let constraint = name(row, 0);
        match keys.last_mut() {
            Some(last) if last.name == constraint => {
                last.columns.push(name(row, 1));
                last.referenced_columns.push(name(row, 4));
            }
            _ => keys.push(ForeignKeyDefinition {
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

    Ok(keys)
}

pub async fn schema_foreign_keys(conn: &mut PgConnection, schema: &str) -> Result<Vec<TableForeignKey>, PgError> {
    let rows = conn
        .query_text(
            "SELECT n.nspname, c.relname, con.conname, a.attname, rn.nspname, rc.relname, ra.attname, \
                    con.confupdtype::text, con.confdeltype::text \
             FROM pg_constraint con \
             JOIN pg_class c ON c.oid = con.conrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_class rc ON rc.oid = con.confrelid \
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             CROSS JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(col, refcol, ord) \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.col \
             JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = k.refcol \
             WHERE con.contype = 'f' AND (n.nspname = $1 OR rn.nspname = $1) \
             ORDER BY n.nspname, c.relname, con.conname, k.ord",
            &[Some(schema)],
        )
        .await?;

    let mut keys: Vec<TableForeignKey> = Vec::new();
    for row in &rows {
        let (owner, table, constraint) = (name(row, 0), name(row, 1), name(row, 2));
        match keys.last_mut() {
            Some(last) if last.database == owner && last.table == table && last.key.name == constraint => {
                last.key.columns.push(name(row, 3));
                last.key.referenced_columns.push(name(row, 6));
            }
            _ => keys.push(TableForeignKey {
                database: owner,
                table,
                key: ForeignKeyDefinition {
                    name: constraint,
                    columns: vec![name(row, 3)],
                    referenced_database: name(row, 4),
                    referenced_table: name(row, 5),
                    referenced_columns: vec![name(row, 6)],
                    on_update: referential_action(&name(row, 7)),
                    on_delete: referential_action(&name(row, 8)),
                },
            }),
        }
    }

    Ok(keys)
}

pub async fn object_structure(
    conn: &mut PgConnection,
    dialect: Dialect,
    schema: &str,
    object: &str,
    kind: ObjectKind,
    signature: Option<&str>,
) -> Result<ObjectStructure, PgError> {
    let mut structure = ObjectStructure {
        ddl: Some(ddl::object_ddl(conn, dialect, schema, object, kind, signature).await?),
        ..Default::default()
    };

    if matches!(kind, ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView) {
        structure.columns = column_definitions(conn, schema, object).await?;
    }

    if kind == ObjectKind::Table {
        structure.indexes = indexes(conn, schema, object).await?;
        structure.foreign_keys = foreign_keys(conn, schema, object).await?;
    }

    Ok(structure)
}

async fn column_names(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<String>, PgError> {
    let rows = conn
        .query_text(
            "SELECT a.attname FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[Some(schema), Some(table)],
        )
        .await?;
    Ok(rows.iter().map(|row| name(row, 0)).collect())
}

fn drop_statement(kind: ObjectKind, schema: &str, object: &str, signature: Option<&str>) -> Result<String, PgError> {
    let target = qualified(schema, object);
    Ok(match kind {
        ObjectKind::Table => format!("DROP TABLE {target}"),
        ObjectKind::View => format!("DROP VIEW {target}"),
        ObjectKind::MaterializedView => format!("DROP MATERIALIZED VIEW {target}"),
        ObjectKind::Sequence => format!("DROP SEQUENCE {target}"),
        ObjectKind::Function => format!("DROP FUNCTION {target}({})", signature.unwrap_or_default()),
        ObjectKind::Procedure => format!("DROP PROCEDURE {target}({})", signature.unwrap_or_default()),
        ObjectKind::Trigger => {
            let table = signature.ok_or_else(|| general("The trigger's table is unknown"))?;
            format!("DROP TRIGGER {} ON {}", quote_identifier(object), qualified(schema, table))
        }
        ObjectKind::Event => return Err(general("Postgres has no events")),
    })
}

pub async fn object_sql(
    conn: &mut PgConnection,
    dialect: Dialect,
    schema: &str,
    object: &str,
    kind: ObjectKind,
    signature: Option<&str>,
    template: SqlTemplate,
) -> Result<String, PgError> {
    let target = qualified(schema, object);
    let has_rows = matches!(kind, ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView);

    let sql = match template {
        SqlTemplate::Create => ddl::object_ddl(conn, dialect, schema, object, kind, signature).await?,
        SqlTemplate::Drop => drop_statement(kind, schema, object, signature)?,
        _ if !has_rows => return Err(general("Only tables and views have row statements")),
        SqlTemplate::Select => {
            let names = column_names(conn, schema, object).await?;
            let list = names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(",\n    ");
            format!("SELECT\n    {list}\nFROM {target}")
        }
        SqlTemplate::Insert => {
            let names = column_names(conn, schema, object).await?;
            let list = names.iter().map(|name| quote_identifier(name)).collect::<Vec<_>>().join(", ");
            let placeholders = names.iter().map(|name| format!("<{{{name}}}>")).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {target}\n    ({list})\nVALUES\n    ({placeholders})")
        }
        SqlTemplate::Update => {
            let names = column_names(conn, schema, object).await?;
            let assignments = names
                .iter()
                .map(|name| format!("{} = <{{{name}}}>", quote_identifier(name)))
                .collect::<Vec<_>>()
                .join(",\n    ");
            format!("UPDATE {target}\nSET\n    {assignments}\nWHERE <{{where_expression}}>")
        }
        SqlTemplate::Delete => format!("DELETE FROM {target}\nWHERE <{{where_expression}}>"),
    };

    Ok(format!("{sql};"))
}

pub async fn database_sql(conn: &mut PgConnection, schema: &str, template: SqlTemplate) -> Result<String, PgError> {
    match template {
        SqlTemplate::Create => {
            let rows = conn
                .query_text(
                    "SELECT pg_get_userbyid(nspowner) FROM pg_namespace WHERE nspname = $1",
                    &[Some(schema)],
                )
                .await?;
            let owner = rows
                .first()
                .and_then(|row| text(row, 0))
                .ok_or_else(|| PgError::Other(AppError::not_found(format!("Schema {schema} not found"))))?;
            Ok(format!(
                "CREATE SCHEMA {} AUTHORIZATION {};",
                quote_identifier(schema),
                quote_identifier(&owner)
            ))
        }
        SqlTemplate::Drop => Ok(format!("DROP SCHEMA {};", quote_identifier(schema))),
        _ => Err(general("Schemas only have create and drop statements")),
    }
}

pub async fn primary_key(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<String>, PgError> {
    let rows = conn
        .query_text(
            "SELECT a.attname \
             FROM pg_index x \
             JOIN pg_class c ON c.oid = x.indrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL unnest(x.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
             WHERE x.indisprimary AND n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p') \
             ORDER BY k.ord",
            &[Some(schema), Some(table)],
        )
        .await?;

    Ok(rows.iter().map(|row| name(row, 0)).collect())
}

pub async fn column_choices(
    conn: &mut PgConnection,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<Option<ColumnChoices>, PgError> {
    let rows = conn
        .query_text(
            "SELECT e.enumlabel \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_type t ON t.oid = a.atttypid \
             JOIN pg_enum e ON e.enumtypid = CASE WHEN t.typtype = 'd' THEN t.typbasetype ELSE t.oid END \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attname = $3 \
             ORDER BY e.enumsortorder",
            &[Some(schema), Some(table), Some(column)],
        )
        .await?;

    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(ColumnChoices {
        multiple: false,
        values: rows.iter().map(|row| name(row, 0)).collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::trigger_detail;

    #[test]
    fn describes_triggers() {
        assert_eq!(trigger_detail(2 | 1 | 4 | 16, "users"), "BEFORE INSERT OR UPDATE on users");
        assert_eq!(trigger_detail(8, "users"), "AFTER DELETE on users");
        assert_eq!(trigger_detail(64 | 4, "v"), "INSTEAD OF INSERT on v");
    }
}
