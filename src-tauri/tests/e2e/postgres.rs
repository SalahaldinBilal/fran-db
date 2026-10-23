use fran_db_lib::db::types::{ColumnChoices, ObjectKind, ValueCategory};

use crate::servers::Flavor;
use crate::support::{Case, Ctx, TestResult, cases, texts};

pub fn all() -> Vec<Case> {
    cases![Some(Flavor::Postgres);
        formats_postgres_values,
        reads_enum_choices,
        runs_dollar_quoted_bodies,
        switches_databases,
        reports_transactional_ddl,
    ]
}

const TYPED_TABLE: &str = "
SET TIME ZONE 'UTC';
CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');
CREATE TABLE typed (
  id integer PRIMARY KEY,
  flag boolean,
  raw bytea,
  happened timestamptz,
  doc jsonb,
  amount numeric(8,3),
  ratio double precision,
  feeling mood,
  tags text[],
  uid uuid
);
INSERT INTO typed VALUES
  (1, true, '\\xdeadbeef', '2024-02-29 13:45:01.25+00', '{\"a\": 1}', 12.5, 0.5, 'happy', '{x,y}', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11');
";

async fn formats_postgres_values(ctx: &Ctx) -> TestResult {
    ctx.run(TYPED_TABLE).await?;
    let set = ctx.query("SELECT * FROM typed").await?;

    let expected = [
        ("flag", ValueCategory::Boolean, "true"),
        ("raw", ValueCategory::Binary, "0xDEADBEEF"),
        ("happened", ValueCategory::Temporal, "2024-02-29 13:45:01.25+00"),
        ("doc", ValueCategory::Json, "{\"a\": 1}"),
        ("amount", ValueCategory::Decimal, "12.500"),
        ("ratio", ValueCategory::Float, "0.5"),
        ("feeling", ValueCategory::Text, "happy"),
        ("tags", ValueCategory::Text, "{x,y}"),
        ("uid", ValueCategory::Text, "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"),
    ];
    for (name, category, value) in expected {
        let index = set.columns.iter().position(|column| column.name == name).ok_or(name)?;
        assert_eq!(set.columns[index].category, category, "category of {name}");
        assert_eq!(set.rows[0][index].as_deref(), Some(value), "value of {name}");
    }

    let feeling = set.columns.iter().find(|column| column.name == "feeling").ok_or("no feeling column")?;
    assert!(feeling.has_choices);
    assert_eq!(feeling.table.as_deref(), Some("typed"));
    assert_eq!(feeling.source_name.as_deref(), Some("feeling"));
    Ok(())
}

async fn reads_enum_choices(ctx: &Ctx) -> TestResult {
    ctx.run(TYPED_TABLE).await?;

    let feeling = ctx.session.column_choices(&ctx.ns, "typed", "feeling").await?;
    assert_eq!(
        feeling,
        Some(ColumnChoices {
            multiple: false,
            values: vec!["sad".to_string(), "ok".to_string(), "happy".to_string()],
        })
    );
    assert_eq!(ctx.session.column_choices(&ctx.ns, "typed", "amount").await?, None);
    Ok(())
}

async fn runs_dollar_quoted_bodies(ctx: &Ctx) -> TestResult {
    let outcomes = ctx
        .run(
            "CREATE FUNCTION add_one(x integer) RETURNS integer LANGUAGE plpgsql AS $body$
BEGIN
  RETURN x + 1; -- a comment; with a semicolon
END
$body$;
SELECT add_one(41) AS answer;",
        )
        .await?;
    assert_eq!(outcomes.len(), 2);
    assert_eq!(texts(&outcomes[1].result_sets[0], "answer"), ["42"]);

    let objects = ctx.session.list_objects(&ctx.ns).await?;
    let function = objects
        .iter()
        .find(|object| object.name == "add_one")
        .ok_or("add_one isn't listed")?;
    assert_eq!(function.kind, ObjectKind::Function);
    assert_eq!(function.signature.as_deref(), Some("x integer"));
    Ok(())
}

async fn switches_databases(ctx: &Ctx) -> TestResult {
    assert_eq!(ctx.session.catalog().as_deref(), Some("postgres"));
    assert!(ctx.session.list_catalogs().await?.iter().any(|catalog| catalog == "postgres"));

    let database = format!("{}_db", ctx.ns);
    ctx.run_fresh(&format!("CREATE DATABASE \"{database}\"")).await?;

    let switched = ctx.session.switch_catalog(&database).await;
    let schemas = match &switched {
        Ok(()) => Some(ctx.session.list_databases().await),
        Err(_) => None,
    };
    let catalog = ctx.session.catalog();
    ctx.session.switch_catalog("postgres").await?;
    ctx.run_fresh(&format!("DROP DATABASE \"{database}\"")).await?;

    switched?;
    assert_eq!(catalog.as_deref(), Some(database.as_str()));
    let schemas = schemas.ok_or("no schema list")??;
    assert!(schemas.iter().any(|schema| schema.name == "public"));
    assert!(!schemas.iter().any(|schema| schema.name == ctx.ns));
    Ok(())
}

async fn reports_transactional_ddl(ctx: &Ctx) -> TestResult {
    let info = ctx.session.server_info();
    assert!(info.transactional_ddl);
    assert!(info.atomic_ddl);
    Ok(())
}
