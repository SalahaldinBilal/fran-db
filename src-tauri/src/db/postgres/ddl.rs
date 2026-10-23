use super::client::{PgConnection, PgError, TextRow};
use super::metadata::{flag, name, non_empty, text};
use super::sqlgen::{Dialect, qualified, quote_identifier};
use crate::db::types::ObjectKind;
use crate::error::AppError;

fn missing(what: &str, object: &str) -> PgError {
    PgError::Other(AppError::not_found(format!("{what} {object} not found")))
}

async fn first_row(conn: &mut PgConnection, sql: &str, params: &[Option<&str>]) -> Result<Option<TextRow>, PgError> {
    Ok(conn.query_text(sql, params).await?.into_iter().next())
}

pub async fn object_ddl(
    conn: &mut PgConnection,
    dialect: Dialect,
    schema: &str,
    object: &str,
    kind: ObjectKind,
    signature: Option<&str>,
) -> Result<String, PgError> {
    match kind {
        ObjectKind::Table => table_ddl(conn, dialect, schema, object).await,
        ObjectKind::View => view_ddl(conn, schema, object, false).await,
        ObjectKind::MaterializedView => view_ddl(conn, schema, object, true).await,
        ObjectKind::Sequence => sequence_ddl(conn, schema, object).await,
        ObjectKind::Function | ObjectKind::Procedure => routine_ddl(conn, schema, object, signature).await,
        ObjectKind::Trigger => trigger_ddl(conn, schema, object, signature).await,
        ObjectKind::Event => Err(PgError::Other(AppError::general("Postgres has no events"))),
    }
}

async fn view_ddl(conn: &mut PgConnection, schema: &str, view: &str, materialized: bool) -> Result<String, PgError> {
    let relkind = if materialized { "m" } else { "v" };
    let row = first_row(
        conn,
        "SELECT pg_get_viewdef(c.oid, true) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = $3::\"char\"",
        &[Some(schema), Some(view), Some(relkind)],
    )
    .await?;
    let definition = row.and_then(|row| text(&row, 0)).ok_or_else(|| missing("View", view))?;
    let body = definition.trim().trim_end_matches(';').trim_end();
    let head = if materialized { "CREATE MATERIALIZED VIEW" } else { "CREATE OR REPLACE VIEW" };
    Ok(format!("{head} {} AS\n{body}", qualified(schema, view)))
}

async fn routine_ddl(conn: &mut PgConnection, schema: &str, routine: &str, signature: Option<&str>) -> Result<String, PgError> {
    let row = first_row(
        conn,
        "SELECT pg_get_functiondef(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE n.nspname = $1 AND p.proname = $2 AND p.prokind IN ('f', 'p') \
           AND ($3::text IS NULL OR pg_get_function_identity_arguments(p.oid) = $3::text) \
         ORDER BY p.oid LIMIT 1",
        &[Some(schema), Some(routine), signature],
    )
    .await?;
    let definition = row.and_then(|row| text(&row, 0)).ok_or_else(|| missing("Routine", routine))?;
    Ok(definition.trim_end().to_string())
}

async fn trigger_ddl(conn: &mut PgConnection, schema: &str, trigger: &str, table: Option<&str>) -> Result<String, PgError> {
    let row = first_row(
        conn,
        "SELECT pg_get_triggerdef(t.oid, true) FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND t.tgname = $2 AND ($3::text IS NULL OR c.relname = $3::text) AND NOT t.tgisinternal \
         ORDER BY t.oid LIMIT 1",
        &[Some(schema), Some(trigger), table],
    )
    .await?;
    row.and_then(|row| text(&row, 0)).ok_or_else(|| missing("Trigger", trigger))
}

async fn sequence_ddl(conn: &mut PgConnection, schema: &str, sequence: &str) -> Result<String, PgError> {
    let row = first_row(
        conn,
        "SELECT format_type(s.seqtypid, NULL), s.seqstart::text, s.seqincrement::text, s.seqmin::text, \
                s.seqmax::text, s.seqcache::text, s.seqcycle \
         FROM pg_sequence s JOIN pg_class c ON c.oid = s.seqrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2",
        &[Some(schema), Some(sequence)],
    )
    .await?
    .ok_or_else(|| missing("Sequence", sequence))?;

    let mut sql = format!(
        "CREATE SEQUENCE {}\n    AS {}\n    START WITH {}\n    INCREMENT BY {}\n    MINVALUE {}\n    MAXVALUE {}\n    CACHE {}",
        qualified(schema, sequence),
        name(&row, 0),
        name(&row, 1),
        name(&row, 2),
        name(&row, 3),
        name(&row, 4),
        name(&row, 5),
    );
    if flag(&row, 6) {
        sql.push_str("\n    CYCLE");
    }
    Ok(sql)
}

struct TableColumn {
    name: String,
    data_type: String,
    not_null: bool,
    expression: Option<String>,
    identity: String,
    generated: String,
    collation: Option<String>,
    comment: Option<String>,
}

fn column_line(column: &TableColumn) -> String {
    let mut line = format!("{} {}", quote_identifier(&column.name), column.data_type);
    if let Some(collation) = &column.collation {
        line.push_str(&format!(" COLLATE {}", quote_identifier(collation)));
    }

    match (column.identity.as_str(), column.generated.as_str()) {
        ("a", _) => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
        ("d", _) => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
        (_, "s" | "v") => {
            let storage = if column.generated == "s" { "STORED" } else { "VIRTUAL" };
            let expression = column.expression.as_deref().unwrap_or_default();
            line.push_str(&format!(" GENERATED ALWAYS AS ({expression}) {storage}"));
        }
        _ => {
            if let Some(expression) = &column.expression {
                line.push_str(&format!(" DEFAULT {expression}"));
            }
        }
    }

    if column.not_null {
        line.push_str(" NOT NULL");
    }
    line
}

/// Rebuilt from the catalog, since Postgres has no `SHOW CREATE TABLE`.
async fn table_ddl(conn: &mut PgConnection, dialect: Dialect, schema: &str, table: &str) -> Result<String, PgError> {
    let relation = first_row(
        conn,
        "SELECT c.oid::text, c.relkind::text, c.relpersistence::text, c.relispartition, \
                CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END, \
                CASE WHEN c.relispartition THEN pg_get_expr(c.relpartbound, c.oid) END, \
                (SELECT pn.nspname FROM pg_inherits i JOIN pg_class pc ON pc.oid = i.inhparent \
                   JOIN pg_namespace pn ON pn.oid = pc.relnamespace WHERE i.inhrelid = c.oid LIMIT 1), \
                (SELECT pc.relname FROM pg_inherits i JOIN pg_class pc ON pc.oid = i.inhparent \
                   WHERE i.inhrelid = c.oid LIMIT 1), \
                obj_description(c.oid, 'pg_class'), \
                (SELECT s.srvname FROM pg_foreign_table f JOIN pg_foreign_server s ON s.oid = f.ftserver \
                   WHERE f.ftrelid = c.oid) \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p', 'f')",
        &[Some(schema), Some(table)],
    )
    .await?
    .ok_or_else(|| missing("Table", table))?;

    let oid = name(&relation, 0);
    let foreign = name(&relation, 1) == "f";
    let unlogged = name(&relation, 2) == "u";
    let partition_of = match (flag(&relation, 3), text(&relation, 6), text(&relation, 7)) {
        (true, Some(parent_schema), Some(parent)) => Some(qualified(&parent_schema, &parent)),
        _ => None,
    };
    let partition_key = non_empty(text(&relation, 4));
    let partition_bound = non_empty(text(&relation, 5));
    let table_comment = non_empty(text(&relation, 8));
    let server = non_empty(text(&relation, 9));

    let columns: Vec<TableColumn> = conn
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
        .await?
        .iter()
        .map(|row| TableColumn {
            name: name(row, 0),
            data_type: name(row, 1),
            not_null: flag(row, 2),
            expression: text(row, 3),
            identity: name(row, 4),
            generated: name(row, 5),
            collation: non_empty(text(row, 6)),
            comment: non_empty(text(row, 7)),
        })
        .collect();

    let constraints = conn
        .query_text(
            "SELECT con.conname, pg_get_constraintdef(con.oid, true) FROM pg_constraint con \
             WHERE con.conrelid = $1::oid AND con.contype IN ('p', 'u', 'c', 'f', 'x') AND con.conislocal \
             ORDER BY CASE con.contype WHEN 'p' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 2 WHEN 'x' THEN 3 ELSE 4 END, \
                      con.conname",
            &[Some(&oid)],
        )
        .await?;

    let indexes = conn
        .query_text(
            "SELECT pg_get_indexdef(x.indexrelid) FROM pg_index x \
             WHERE x.indrelid = $1::oid AND NOT EXISTS ( \
                 SELECT 1 FROM pg_constraint con WHERE con.conindid = x.indexrelid AND con.contype IN ('p', 'u', 'x') \
             ) \
             ORDER BY x.indexrelid",
            &[Some(&oid)],
        )
        .await?;

    let target = qualified(schema, table);
    let mut head = String::from("CREATE ");
    if unlogged {
        head.push_str("UNLOGGED ");
    }
    if foreign {
        head.push_str("FOREIGN ");
    }
    head.push_str("TABLE ");
    head.push_str(&target);

    let constraint_lines: Vec<String> = constraints
        .iter()
        .map(|row| format!("CONSTRAINT {} {}", quote_identifier(&name(row, 0)), name(row, 1)))
        .collect();

    let mut create = match (&partition_of, &partition_bound) {
        (Some(parent), Some(bound)) if constraint_lines.is_empty() => format!("{head} PARTITION OF {parent}\n{bound}"),
        (Some(parent), Some(bound)) => format!(
            "{head} PARTITION OF {parent} (\n    {}\n)\n{bound}",
            constraint_lines.join(",\n    ")
        ),
        _ => {
            let mut lines: Vec<String> = columns.iter().map(column_line).collect();
            lines.extend(constraint_lines);
            format!("{head} (\n    {}\n)", lines.join(",\n    "))
        }
    };

    if let Some(key) = partition_key {
        create.push_str(&format!("\nPARTITION BY {key}"));
    }
    if let Some(server) = server {
        create.push_str(&format!("\nSERVER {}", quote_identifier(&server)));
    }

    let mut statements = vec![create];
    statements.extend(indexes.iter().filter_map(|row| text(row, 0)));
    if let Some(comment) = table_comment {
        statements.push(format!("COMMENT ON TABLE {target} IS {}", dialect.string_literal(&comment)));
    }
    for column in &columns {
        if let Some(comment) = &column.comment {
            statements.push(format!(
                "COMMENT ON COLUMN {target}.{} IS {}",
                quote_identifier(&column.name),
                dialect.string_literal(comment)
            ));
        }
    }

    Ok(statements.join(";\n\n"))
}

#[cfg(test)]
mod tests {
    use super::{TableColumn, column_line};

    fn column(identity: &str, generated: &str, expression: Option<&str>) -> TableColumn {
        TableColumn {
            name: "total".to_string(),
            data_type: "integer".to_string(),
            not_null: true,
            expression: expression.map(Into::into),
            identity: identity.to_string(),
            generated: generated.to_string(),
            collation: None,
            comment: None,
        }
    }

    #[test]
    fn writes_column_lines() {
        assert_eq!(column_line(&column("", "", Some("0"))), r#""total" integer DEFAULT 0 NOT NULL"#);
        assert_eq!(
            column_line(&column("d", "", None)),
            r#""total" integer GENERATED BY DEFAULT AS IDENTITY NOT NULL"#
        );
        assert_eq!(
            column_line(&column("", "s", Some("(a * b)"))),
            r#""total" integer GENERATED ALWAYS AS ((a * b)) STORED NOT NULL"#
        );
    }
}
