use std::time::{Duration, Instant};

use fran_db_lib::db::session::ContextAccess;
use fran_db_lib::db::types::{
    CellEdit, ColumnDefinition, ColumnDesign, FilterOp, FilterValue, ForeignKeyDesign, IndexColumnDesign, IndexDefinition,
    IndexDesign, IndexKind, ObjectKind, ReadQuery, RowChange, RowFilter, RowOrder, SqlTemplate, TableDesign, ValueCategory,
};

use crate::support::{Case, Ctx, TestResult, cases, texts, values};

pub fn all() -> Vec<Case> {
    cases![None;
        reports_server_info,
        lists_databases_objects_and_columns,
        describes_table_structure,
        create_sql_rebuilds_the_table,
        select_template_runs,
        reads_table_rows,
        reads_primary_keys,
        stops_at_the_first_failed_statement,
        counts_affected_rows,
        applies_row_edits_in_one_transaction,
        rolls_back_a_failed_batch,
        cancels_a_running_query,
        keeps_state_per_context,
        designer_alters_a_table,
        designer_creates_a_table,
        read_only_contexts_refuse_writes,
        runs_generated_reads,
        lists_foreign_keys_both_ways,
    ]
}

async fn read_only_contexts_refuse_writes(ctx: &Ctx) -> TestResult {
    let reader = ctx
        .session
        .open_context(Some(ctx.ns.clone()), ContextAccess::ReadOnly)
        .await?;
    let int_type = ctx.pick("INT", "integer");
    let attempts = [
        "DELETE FROM items".to_string(),
        "UPDATE items SET name = 'x'".to_string(),
        format!("CREATE TABLE made_by_reader (x {int_type})"),
    ];

    let mut refused = Vec::new();
    for sql in &attempts {
        let outcomes = ctx.session.execute(reader, sql, 10).await?;
        refused.push(outcomes[0].error.is_some());
    }
    let read = ctx.session.execute(reader, "SELECT name FROM items", 10).await?;
    ctx.session.close_context(reader).await;

    assert_eq!(refused, [true, true, true], "a read-only context ran a write");
    assert!(read[0].error.is_none(), "a read-only context refused a read");
    let count = ctx.query("SELECT COUNT(*) AS n FROM items").await?;
    assert_eq!(texts(&count, "n"), ["3"]);
    Ok(())
}

async fn runs_generated_reads(ctx: &Ctx) -> TestResult {
    let rows = ctx.session.read_sql(
        &ctx.ns,
        &ReadQuery::Rows {
            table: "items".to_string(),
            columns: vec!["name".to_string()],
            filters: vec![RowFilter {
                column: "price".to_string(),
                op: FilterOp::Gt,
                values: vec![FilterValue::Number("1".to_string())],
            }],
            order: vec![RowOrder {
                column: "name".to_string(),
                descending: true,
            }],
            limit: 10,
            offset: 0,
        },
    )?;
    assert_eq!(texts(&ctx.query(&rows).await?, "name"), ["cherry", "apple"]);

    let count = ctx.session.read_sql(
        &ctx.ns,
        &ReadQuery::Count {
            table: "items".to_string(),
            filters: vec![RowFilter {
                column: "note".to_string(),
                op: FilterOp::IsNull,
                values: Vec::new(),
            }],
        },
    )?;
    assert_eq!(texts(&ctx.query(&count).await?, "count"), ["1"]);

    let explain = ctx.session.explain_sql("SELECT * FROM items WHERE id = 1");
    assert!(!ctx.query(&explain).await?.rows.is_empty());
    Ok(())
}

async fn lists_foreign_keys_both_ways(ctx: &Ctx) -> TestResult {
    let int_type = ctx.pick("INT", "integer");
    ctx.run(&format!(
        "CREATE TABLE orders (id {int_type} PRIMARY KEY, item_id {int_type}, \
         CONSTRAINT orders_item FOREIGN KEY (item_id) REFERENCES items (id))"
    ))
    .await?;

    let keys = ctx.session.foreign_keys(&ctx.ns).await?;
    let key = keys
        .iter()
        .find(|found| found.table == "orders")
        .ok_or("the orders key wasn't listed")?;
    assert_eq!(key.database, ctx.ns);
    assert_eq!(key.key.columns, ["item_id"]);
    assert_eq!(key.key.referenced_table, "items");
    assert_eq!(key.key.referenced_columns, ["id"]);
    Ok(())
}

fn column(name: &str, data_type: &str) -> ColumnDesign {
    ColumnDesign {
        original_name: None,
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
        original_name: None,
        name: name.to_string(),
        kind,
        columns: columns
            .iter()
            .map(|name| IndexColumnDesign {
                name: name.to_string(),
                length: None,
            })
            .collect(),
        method: None,
        constraint: false,
    }
}

type ColumnSummary = (String, String, bool, Option<String>, bool, Option<String>);

fn column_summary(column: &ColumnDefinition) -> ColumnSummary {
    (
        column.name.clone(),
        column.data_type.clone(),
        column.nullable,
        column.default_value.clone(),
        column.primary_key,
        column.extra.clone(),
    )
}

fn index_summary(index: &IndexDefinition) -> (String, bool, bool, Vec<String>) {
    (index.name.clone(), index.primary, index.unique, index.columns.clone())
}

async fn reports_server_info(ctx: &Ctx) -> TestResult {
    let info = ctx.session.server_info();
    assert_eq!(info.product, ctx.spec.product);
    assert!(
        info.version.starts_with(ctx.spec.tag),
        "server version {} isn't {}",
        info.version,
        ctx.spec.tag
    );
    Ok(())
}

async fn lists_databases_objects_and_columns(ctx: &Ctx) -> TestResult {
    let databases = ctx.session.list_databases().await?;
    let ours = databases
        .iter()
        .find(|database| database.name == ctx.ns)
        .ok_or("the test namespace isn't listed")?;
    assert!(!ours.system);

    let objects = ctx.session.list_objects(&ctx.ns).await?;
    let relations: Vec<(&str, ObjectKind)> = objects
        .iter()
        .filter(|object| matches!(object.kind, ObjectKind::Table | ObjectKind::View))
        .map(|object| (object.name.as_str(), object.kind))
        .collect();
    assert_eq!(relations, [("cheap_items", ObjectKind::View), ("items", ObjectKind::Table)]);

    let columns = ctx.session.list_columns(&ctx.ns).await?;
    let item_columns: Vec<&str> = columns
        .iter()
        .filter(|column| column.table == "items")
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(item_columns, ["id", "name", "price", "note"]);
    Ok(())
}

async fn describes_table_structure(ctx: &Ctx) -> TestResult {
    let structure = ctx.session.object_structure(&ctx.ns, "items", ObjectKind::Table, None).await?;

    let names: Vec<&str> = structure.columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "price", "note"]);
    assert!(structure.columns[0].primary_key);
    assert!(!structure.columns[1].nullable);
    assert!(structure.columns[2].nullable);

    let primary = structure.indexes.iter().find(|index| index.primary).ok_or("no primary index")?;
    assert_eq!(primary.columns, ["id"]);
    assert!(structure.ddl.as_deref().is_some_and(|ddl| ddl.contains("items")));
    Ok(())
}

async fn create_sql_rebuilds_the_table(ctx: &Ctx) -> TestResult {
    let before = ctx.session.object_structure(&ctx.ns, "items", ObjectKind::Table, None).await?;
    let create = ctx
        .session
        .object_sql(&ctx.ns, "items", ObjectKind::Table, None, SqlTemplate::Create)
        .await?;

    ctx.run("DROP VIEW cheap_items; DROP TABLE items").await?;
    ctx.run(&create).await?;

    let after = ctx.session.object_structure(&ctx.ns, "items", ObjectKind::Table, None).await?;
    let columns = |structure: &fran_db_lib::db::types::ObjectStructure| {
        structure.columns.iter().map(column_summary).collect::<Vec<_>>()
    };
    let indexes = |structure: &fran_db_lib::db::types::ObjectStructure| {
        structure.indexes.iter().map(index_summary).collect::<Vec<_>>()
    };
    assert_eq!(columns(&after), columns(&before));
    assert_eq!(indexes(&after), indexes(&before));
    Ok(())
}

async fn select_template_runs(ctx: &Ctx) -> TestResult {
    let select = ctx
        .session
        .object_sql(&ctx.ns, "items", ObjectKind::Table, None, SqlTemplate::Select)
        .await?;
    let rows = ctx.query(&select).await?;
    assert_eq!(texts(&rows, "name"), ["apple", "banana", "cherry"]);
    Ok(())
}

async fn reads_table_rows(ctx: &Ctx) -> TestResult {
    let all = ctx.query("SELECT * FROM items ORDER BY id").await?;
    assert_eq!(texts(&all, "name"), ["apple", "banana", "cherry"]);
    assert_eq!(values(&all, "note")[1], None);
    assert_eq!(values(&all, "price")[1].as_deref(), Some("0.25"));

    let categories: Vec<ValueCategory> = all.columns.iter().map(|column| column.category).collect();
    assert_eq!(
        categories,
        [ValueCategory::Integer, ValueCategory::Text, ValueCategory::Decimal, ValueCategory::Text]
    );

    let id = &all.columns[0];
    assert!(id.primary_key);
    assert_eq!(id.table.as_deref(), Some("items"));
    assert_eq!(id.source_name.as_deref(), Some("id"));
    Ok(())
}

async fn reads_primary_keys(ctx: &Ctx) -> TestResult {
    assert_eq!(ctx.session.primary_key(&ctx.ns, "items").await?, ["id"]);
    assert!(ctx.session.primary_key(&ctx.ns, "cheap_items").await?.is_empty());
    Ok(())
}

async fn stops_at_the_first_failed_statement(ctx: &Ctx) -> TestResult {
    let outcomes = ctx
        .session
        .execute(ctx.context, "SELECT 1 AS one; SELECT * FROM missing_table; SELECT 2 AS two", 1000)
        .await?;

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes[0].error.is_none());
    assert_eq!(outcomes[0].result_sets[0].rows, [[Some("1".to_string())]]);
    assert!(outcomes[1].error.is_some());
    Ok(())
}

async fn counts_affected_rows(ctx: &Ctx) -> TestResult {
    let outcomes = ctx.run("UPDATE items SET price = price + 1 WHERE price < 5").await?;
    assert_eq!(outcomes[0].affected_rows, 2);
    Ok(())
}

async fn applies_row_edits_in_one_transaction(ctx: &Ctx) -> TestResult {
    let loaded = ctx.query("SELECT * FROM items").await?;
    let position = |name: &str| loaded.columns.iter().position(|column| column.name == name).unwrap();
    let row = |name: &str| {
        loaded
            .rows
            .iter()
            .find(|row| row[position("name")].as_deref() == Some(name))
            .unwrap()
            .clone()
    };

    let changes = vec![
        RowChange::Update {
            key: row("banana"),
            values: vec![CellEdit {
                column: position("price"),
                value: Some("0.30".to_string()),
            }],
        },
        RowChange::Delete { key: row("cherry") },
        RowChange::Insert {
            values: vec![
                CellEdit {
                    column: position("name"),
                    value: Some("date".to_string()),
                },
                CellEdit {
                    column: position("price"),
                    value: Some("3.00".to_string()),
                },
            ],
        },
    ];

    let statements = ctx.session.row_changes_sql(&ctx.ns, "items", &loaded.columns, &changes)?;
    let outcome = ctx.session.execute_batch(Some(&ctx.ns), &statements, true).await?;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.executed, statements.len());

    let reloaded = ctx.query("SELECT name, price FROM items ORDER BY name").await?;
    assert_eq!(texts(&reloaded, "name"), ["apple", "banana", "date"]);
    assert_eq!(texts(&reloaded, "price"), ["1.50", "0.30", "3.00"]);
    Ok(())
}

async fn rolls_back_a_failed_batch(ctx: &Ctx) -> TestResult {
    let statements = [
        "INSERT INTO items (name) VALUES ('elderberry')".to_string(),
        "INSERT INTO items (name) VALUES (NULL)".to_string(),
    ];
    let outcome = ctx.session.execute_batch(Some(&ctx.ns), &statements, true).await?;

    assert_eq!(outcome.failed_index, Some(1));
    assert!(outcome.error.is_some());
    assert!(outcome.rolled_back);

    let remaining = ctx.query("SELECT name FROM items ORDER BY name").await?;
    assert_eq!(texts(&remaining, "name"), ["apple", "banana", "cherry"]);
    Ok(())
}

async fn cancels_a_running_query(ctx: &Ctx) -> TestResult {
    let sleep = ctx.pick("SELECT SLEEP(30)", "SELECT pg_sleep(30)");
    let started = Instant::now();

    let query = ctx.session.execute(ctx.context, sleep, 1000);
    // Retries in case the first cancel lands before the query has started.
    let cancel = async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        loop {
            if let Err(error) = ctx.session.cancel(ctx.context).await {
                return error;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    };

    tokio::select! {
        outcomes = query => { outcomes?; }
        error = cancel => return Err(error.into()),
    }

    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the query ran for {:?} after being cancelled",
        started.elapsed()
    );
    let after = ctx.query("SELECT 1 AS one").await?;
    assert_eq!(texts(&after, "one"), ["1"]);
    Ok(())
}

async fn keeps_state_per_context(ctx: &Ctx) -> TestResult {
    ctx.run("CREATE TEMPORARY TABLE scratch (x INT)").await?;

    let other = ctx
        .session
        .open_context(Some(ctx.ns.clone()), ContextAccess::ReadWrite)
        .await?;
    let outcomes = ctx.session.execute(other, "SELECT * FROM scratch", 1000).await;
    ctx.session.close_context(other).await;

    assert!(
        outcomes?[0].error.is_some(),
        "a temporary table from one context was visible in another"
    );
    ctx.run("SELECT * FROM scratch").await?;
    Ok(())
}

async fn designer_alters_a_table(ctx: &Ctx) -> TestResult {
    let original = ctx.session.table_design(&ctx.ns, "items").await?;
    let unchanged = ctx.session.table_design_sql(&ctx.ns, Some(&original), &original)?;
    assert!(unchanged.is_empty(), "an unchanged design produced SQL: {unchanged:?}");

    let int_type = ctx.pick("int", "integer");
    let mut edited = original.clone();
    let note = edited
        .columns
        .iter_mut()
        .find(|column| column.name == "note")
        .ok_or("no note column")?;
    note.name = "details".to_string();
    edited.columns.push(ColumnDesign {
        nullable: false,
        default_value: Some("0".to_string()),
        comment: "units on hand".to_string(),
        ..column("stock", int_type)
    });
    edited.indexes.push(index("items_name_idx", IndexKind::Index, &["name"]));

    let statements = ctx.session.table_design_sql(&ctx.ns, Some(&original), &edited)?;
    let transactional = ctx.session.server_info().transactional_ddl;
    let outcome = ctx.session.execute_batch(Some(&ctx.ns), &statements, transactional).await?;
    assert!(outcome.error.is_none(), "{:?}\n{statements:#?}", outcome.error);

    let reloaded = ctx.session.table_design(&ctx.ns, "items").await?;
    let names: Vec<&str> = reloaded.columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "price", "details", "stock"]);

    let stock = &reloaded.columns[4];
    assert!(stock.data_type.eq_ignore_ascii_case(int_type), "stock is {}", stock.data_type);
    assert!(!stock.nullable);
    assert_eq!(stock.default_value.as_deref().map(|value| value.trim_matches('\'')), Some("0"));
    assert_eq!(stock.comment, "units on hand");

    let name_index = reloaded
        .indexes
        .iter()
        .find(|index| index.name == "items_name_idx")
        .ok_or("the new index is missing")?;
    assert_eq!(name_index.kind, IndexKind::Index);
    assert_eq!(name_index.columns[0].name, "name");
    Ok(())
}

async fn designer_creates_a_table(ctx: &Ctx) -> TestResult {
    let int_type = ctx.pick("int", "integer");
    let design = TableDesign {
        name: "gadgets".to_string(),
        comment: "made by the designer".to_string(),
        engine: None,
        charset: None,
        collation: None,
        columns: vec![
            ColumnDesign {
                nullable: false,
                auto_increment: true,
                ..column("id", int_type)
            },
            ColumnDesign {
                nullable: false,
                default_value: Some("'none'".to_string()),
                ..column("label", "varchar(50)")
            },
            column("item_id", int_type),
        ],
        indexes: vec![index("PRIMARY", IndexKind::Primary, &["id"])],
        foreign_keys: vec![ForeignKeyDesign {
            original_name: None,
            name: "gadgets_item_fk".to_string(),
            columns: vec!["item_id".to_string()],
            referenced_database: ctx.ns.clone(),
            referenced_table: "items".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "CASCADE".to_string(),
            on_delete: "CASCADE".to_string(),
        }],
    };

    let statements = ctx.session.table_design_sql(&ctx.ns, None, &design)?;
    let transactional = ctx.session.server_info().transactional_ddl;
    let outcome = ctx.session.execute_batch(Some(&ctx.ns), &statements, transactional).await?;
    assert!(outcome.error.is_none(), "{:?}\n{statements:#?}", outcome.error);

    let created = ctx.session.table_design(&ctx.ns, "gadgets").await?;
    assert_eq!(created.comment, "made by the designer");
    let names: Vec<&str> = created.columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(names, ["id", "label", "item_id"]);
    assert!(created.columns[0].auto_increment);
    assert!(
        created.columns[1].default_value.as_deref().is_some_and(|value| value.contains("none")),
        "label default is {:?}",
        created.columns[1].default_value
    );

    let primary = created
        .indexes
        .iter()
        .find(|index| index.kind == IndexKind::Primary)
        .ok_or("no primary key")?;
    assert_eq!(primary.columns[0].name, "id");

    let key = created.foreign_keys.first().ok_or("no foreign key")?;
    assert_eq!(key.name, "gadgets_item_fk");
    assert_eq!(key.referenced_table, "items");
    assert_eq!(key.on_delete, "CASCADE");

    ctx.run("INSERT INTO gadgets (label, item_id) VALUES ('widget', 1); DELETE FROM items WHERE id = 1").await?;
    let left = ctx.query("SELECT COUNT(*) AS n FROM gadgets").await?;
    assert_eq!(texts(&left, "n"), ["0"], "ON DELETE CASCADE didn't remove the gadget");
    Ok(())
}
