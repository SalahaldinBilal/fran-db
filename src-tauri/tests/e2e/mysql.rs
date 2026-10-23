use fran_db_lib::db::types::{ColumnChoices, ObjectKind, ValueCategory};

use crate::servers::Flavor;
use crate::support::{Case, Ctx, TestResult, cases};

pub fn all() -> Vec<Case> {
    cases![Some(Flavor::Mysql);
        formats_mysql_values,
        reads_enum_and_set_choices,
        runs_delimiter_blocks,
        reports_last_insert_id,
    ]
}

const TYPED_TABLE: &str = "
CREATE TABLE typed (
  id INT PRIMARY KEY,
  bits BIT(8),
  raw VARBINARY(8),
  happened DATETIME(6),
  span TIME,
  yr YEAR,
  doc JSON,
  size ENUM('small', 'it''s big'),
  tags SET('a', 'b', 'c'),
  amount DECIMAL(8,3),
  ratio DOUBLE
);
INSERT INTO typed VALUES
  (1, b'101', 0xDEADBEEF, '2024-02-29 13:45:01.250000', '-26:15:00', 2024, '{\"a\": 1}', 'it''s big', 'a,c', 12.5, 0.5);
";

async fn formats_mysql_values(ctx: &Ctx) -> TestResult {
    ctx.run(TYPED_TABLE).await?;
    let set = ctx.query("SELECT * FROM typed").await?;

    let expected = [
        ("bits", ValueCategory::Bit, "5"),
        ("raw", ValueCategory::Binary, "0xDEADBEEF"),
        ("happened", ValueCategory::Temporal, "2024-02-29 13:45:01.250000"),
        ("span", ValueCategory::Temporal, "-26:15:00"),
        ("yr", ValueCategory::Integer, "2024"),
        ("doc", ValueCategory::Json, "{\"a\": 1}"),
        ("size", ValueCategory::Text, "it's big"),
        ("tags", ValueCategory::Text, "a,c"),
        ("amount", ValueCategory::Decimal, "12.500"),
        ("ratio", ValueCategory::Float, "0.5"),
    ];
    for (name, category, value) in expected {
        let index = set.columns.iter().position(|column| column.name == name).ok_or(name)?;
        assert_eq!(set.columns[index].category, category, "category of {name}");
        assert_eq!(set.rows[0][index].as_deref(), Some(value), "value of {name}");
    }

    let has_choices: Vec<&str> = set
        .columns
        .iter()
        .filter(|column| column.has_choices)
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(has_choices, ["size", "tags"]);
    Ok(())
}

async fn reads_enum_and_set_choices(ctx: &Ctx) -> TestResult {
    ctx.run(TYPED_TABLE).await?;

    let size = ctx.session.column_choices(&ctx.ns, "typed", "size").await?;
    assert_eq!(
        size,
        Some(ColumnChoices {
            multiple: false,
            values: vec!["small".to_string(), "it's big".to_string()],
        })
    );

    let tags = ctx.session.column_choices(&ctx.ns, "typed", "tags").await?;
    assert_eq!(tags.map(|choices| choices.multiple), Some(true));
    assert_eq!(ctx.session.column_choices(&ctx.ns, "typed", "amount").await?, None);
    Ok(())
}

async fn runs_delimiter_blocks(ctx: &Ctx) -> TestResult {
    let outcomes = ctx
        .run(
            "DELIMITER //
CREATE PROCEDURE two_sets()
BEGIN
  SELECT 1 AS first_set;
  SELECT 2 AS second_set;
END //
DELIMITER ;
CALL two_sets();",
        )
        .await?;

    let call = outcomes.last().ok_or("no outcomes")?;
    assert_eq!(call.result_sets.len(), 2);

    let objects = ctx.session.list_objects(&ctx.ns).await?;
    assert!(
        objects
            .iter()
            .any(|object| object.name == "two_sets" && object.kind == ObjectKind::Procedure)
    );
    let create = ctx
        .session
        .object_sql(&ctx.ns, "two_sets", ObjectKind::Procedure, None, fran_db_lib::db::types::SqlTemplate::Create)
        .await?;
    assert!(create.contains("SELECT 2 AS second_set"));
    Ok(())
}

async fn reports_last_insert_id(ctx: &Ctx) -> TestResult {
    let outcomes = ctx.run("INSERT INTO items (name) VALUES ('fig')").await?;
    assert_eq!(outcomes[0].last_insert_id, Some(4));
    assert!(!ctx.session.server_info().transactional_ddl);
    Ok(())
}
