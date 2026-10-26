use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::db::types::{FilterOp, ObjectKind};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenSessionArgs {
    #[schemars(description = "Connection id or name, from list_connections")]
    pub connection: String,
    #[schemars(
        description = "On Postgres and D1, the database to open. Elsewhere, the database tools use when you leave theirs out"
    )]
    pub database: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionArgs {
    #[schemars(description = "Session id from open_session")]
    pub session: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListObjectsArgs {
    pub session: String,
    #[schemars(description = "Database, or schema on Postgres")]
    pub database: Option<String>,
    #[schemars(description = "Only objects of this kind")]
    pub kind: Option<ObjectKindArg>,
    #[schemars(description = "Only objects whose name contains this, ignoring case")]
    pub name_contains: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeTableArgs {
    pub session: String,
    #[schemars(description = "Database, or schema on Postgres")]
    pub database: Option<String>,
    pub table: String,
    #[schemars(description = "table (the default), view or materializedView")]
    pub kind: Option<ObjectKindArg>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DefinitionArgs {
    pub session: String,
    pub database: Option<String>,
    pub name: String,
    pub kind: ObjectKindArg,
    #[schemars(description = "Argument types of an overloaded routine, or a trigger's table, as list_objects shows it")]
    pub signature: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchSchemaArgs {
    pub session: String,
    #[schemars(description = "Text to find in table and column names, ignoring case")]
    pub pattern: String,
    #[schemars(description = "Search only this database. All shared ones when left out")]
    pub database: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelationshipsArgs {
    pub session: String,
    pub database: Option<String>,
    #[schemars(description = "Only keys from or to this table")]
    pub table: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKindArg {
    Table,
    View,
    MaterializedView,
    Sequence,
    Procedure,
    Function,
    Trigger,
    Event,
}

impl From<ObjectKindArg> for ObjectKind {
    fn from(kind: ObjectKindArg) -> Self {
        match kind {
            ObjectKindArg::Table => ObjectKind::Table,
            ObjectKindArg::View => ObjectKind::View,
            ObjectKindArg::MaterializedView => ObjectKind::MaterializedView,
            ObjectKindArg::Sequence => ObjectKind::Sequence,
            ObjectKindArg::Procedure => ObjectKind::Procedure,
            ObjectKindArg::Function => ObjectKind::Function,
            ObjectKindArg::Trigger => ObjectKind::Trigger,
            ObjectKindArg::Event => ObjectKind::Event,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FilterOpArg {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
}

impl From<FilterOpArg> for FilterOp {
    fn from(op: FilterOpArg) -> Self {
        match op {
            FilterOpArg::Eq => FilterOp::Eq,
            FilterOpArg::Ne => FilterOp::Ne,
            FilterOpArg::Lt => FilterOp::Lt,
            FilterOpArg::Lte => FilterOp::Lte,
            FilterOpArg::Gt => FilterOp::Gt,
            FilterOpArg::Gte => FilterOp::Gte,
            FilterOpArg::Like => FilterOp::Like,
            FilterOpArg::NotLike => FilterOp::NotLike,
            FilterOpArg::In => FilterOp::In,
            FilterOpArg::NotIn => FilterOp::NotIn,
            FilterOpArg::IsNull => FilterOp::IsNull,
            FilterOpArg::IsNotNull => FilterOp::IsNotNull,
            FilterOpArg::Between => FilterOp::Between,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FilterArg {
    pub column: String,
    pub op: FilterOpArg,
    #[schemars(description = "The value for comparisons and like")]
    pub value: Option<Value>,
    #[schemars(description = "The values for in and notIn, or the two bounds for between")]
    pub values: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OrderArg {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadRowsArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    #[schemars(description = "Columns to return, all when left out")]
    pub columns: Option<Vec<String>>,
    #[schemars(description = "Conditions that all have to match")]
    pub filters: Option<Vec<FilterArg>>,
    pub order_by: Option<Vec<OrderArg>>,
    #[schemars(description = "Rows to return, 100 by default, at most 1000")]
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountRowsArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    pub filters: Option<Vec<FilterArg>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ColumnValuesArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    pub column: String,
    pub filters: Option<Vec<FilterArg>>,
    #[schemars(description = "Distinct values to return, 50 by default, at most 1000")]
    pub limit: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunQueryArgs {
    pub session: String,
    #[schemars(description = "Default database or schema for unqualified names")]
    pub database: Option<String>,
    pub sql: String,
    #[schemars(description = "Rows to return per result, 100 by default, at most 1000")]
    pub max_rows: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqlArgs {
    pub session: String,
    #[schemars(description = "Default database or schema for unqualified names")]
    pub database: Option<String>,
    pub sql: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InsertRowsArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    #[schemars(description = "Rows as objects of column name to value. Columns left out get their defaults")]
    pub rows: Vec<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RowUpdateArg {
    #[schemars(description = "The row's full primary key, column name to value")]
    pub key: Map<String, Value>,
    #[schemars(description = "Column name to new value")]
    pub values: Map<String, Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateRowsArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    pub rows: Vec<RowUpdateArg>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteRowsArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    #[schemars(description = "Full primary keys of the rows to delete, column name to value")]
    pub keys: Vec<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TableArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTableArgs {
    pub session: String,
    pub database: Option<String>,
    #[schemars(description = "A table design in the shape get_table_design returns. originalName stays null everywhere")]
    pub design: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AlterTableArgs {
    pub session: String,
    pub database: Option<String>,
    pub table: String,
    #[schemars(
        description = "The design from get_table_design, edited. Keep originalName on existing columns, indexes and keys so renames aren't read as drop and add"
    )]
    pub design: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DropObjectArgs {
    pub session: String,
    pub database: Option<String>,
    pub name: String,
    pub kind: ObjectKindArg,
    pub signature: Option<String>,
}
