use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
    /// Each schema statement is applied completely or not at all.
    pub atomic_ddl: bool,
    /// Schema statements can run inside a transaction and roll back with it.
    pub transactional_ddl: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    pub system: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Sequence,
    Procedure,
    Function,
    Trigger,
    Event,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub name: String,
    pub kind: ObjectKind,
    /// Tells apart objects that share a name: argument types of an overloaded routine, or a trigger's table.
    pub signature: Option<String>,
    pub detail: Option<String>,
    pub row_estimate: Option<u64>,
    pub size_bytes: Option<u64>,
    pub comment: Option<String>,
}

/// How the frontend should render and align a value. Values themselves always travel as text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ValueCategory {
    Integer,
    Decimal,
    Float,
    Boolean,
    Text,
    Binary,
    Temporal,
    Json,
    Bit,
    Spatial,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
    pub category: ValueCategory,
    pub nullable: bool,
    pub primary_key: bool,
    pub database: Option<String>,
    pub table: Option<String>,
    /// The column's name in `table`, set only when the value comes straight from a table column.
    pub source_name: Option<String>,
    /// The column only takes values from a fixed list, loaded with `column_choices`.
    #[serde(default)]
    pub has_choices: bool,
}

/// The values an ENUM-like column accepts. `multiple` means a value holds any number of them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnChoices {
    pub multiple: bool,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementOutcome {
    pub sql: String,
    pub result_sets: Vec<ResultSet>,
    pub affected_rows: u64,
    pub last_insert_id: Option<u64>,
    pub warnings: u16,
    pub info: Option<String>,
    pub elapsed_ms: f64,
    /// Time to the server's first reply minus `network_ms`, so roughly the server's own work.
    pub execution_ms: f64,
    /// One round trip to the server, measured with a ping just before the statements ran.
    /// The same value on every outcome of a run.
    pub network_ms: Option<f64>,
    /// Reading and converting rows after the first reply.
    pub fetch_ms: f64,
    pub error: Option<AppError>,
}

/// Offsets are UTF-16 code units so the editor can use them directly.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatementRange {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub primary_key: bool,
    pub extra: Option<String>,
    pub comment: Option<String>,
    pub collation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDefinition {
    pub name: String,
    pub primary: bool,
    pub unique: bool,
    pub columns: Vec<String>,
    pub index_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyDefinition {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_database: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: Option<String>,
    pub on_delete: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectStructure {
    pub columns: Vec<ColumnDefinition>,
    pub indexes: Vec<IndexDefinition>,
    pub foreign_keys: Vec<ForeignKeyDefinition>,
    pub ddl: Option<String>,
}

/// One column of every table and view in a database, for completion and linting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogColumn {
    pub table: String,
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SqlTemplate {
    Create,
    Drop,
    Select,
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellEdit {
    pub column: usize,
    pub value: Option<String>,
}

/// `key` is the row as it was loaded, used to find it again by its primary key.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RowChange {
    Insert {
        values: Vec<CellEdit>,
    },
    Update {
        key: Vec<Option<String>>,
        values: Vec<CellEdit>,
    },
    Delete {
        key: Vec<Option<String>>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchOutcome {
    pub executed: usize,
    pub failed_index: Option<usize>,
    pub error: Option<AppError>,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IndexKind {
    Primary,
    Unique,
    Index,
    Fulltext,
    Spatial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnDesign {
    pub name: String,
    pub length: Option<u32>,
}

/// `original_name` ties an edited item back to what exists on the server. None means it's new.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDesign {
    pub original_name: Option<String>,
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// Raw SQL, quotes included, exactly as it should follow DEFAULT.
    pub default_value: Option<String>,
    pub auto_increment: bool,
    pub on_update: Option<String>,
    pub comment: String,
    pub collation: Option<String>,
    pub generated_expression: Option<String>,
    pub generated_stored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexDesign {
    pub original_name: Option<String>,
    pub name: String,
    pub kind: IndexKind,
    pub columns: Vec<IndexColumnDesign>,
    /// Access method such as `gin`. None means the server's default.
    #[serde(default)]
    pub method: Option<String>,
    /// Backs a table constraint, so it's dropped through the table rather than on its own.
    #[serde(default)]
    pub constraint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyDesign {
    pub original_name: Option<String>,
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_database: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TableDesign {
    pub name: String,
    pub comment: String,
    pub engine: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub columns: Vec<ColumnDesign>,
    pub indexes: Vec<IndexDesign>,
    pub foreign_keys: Vec<ForeignKeyDesign>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharsetInfo {
    pub name: String,
    pub default_collation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollationInfo {
    pub name: String,
    pub charset: String,
}

/// Which parts of the table designer apply to this server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignCapabilities {
    pub engines: bool,
    /// A table level character set and collation.
    pub charsets: bool,
    pub column_collations: bool,
    pub on_update: bool,
    pub index_lengths: bool,
    /// Existing columns can be moved. New tables can always be ordered freely.
    pub reorder_columns: bool,
    pub virtual_columns: bool,
    /// UNSIGNED and ZEROFILL on numeric types.
    pub unsigned: bool,
    pub zerofill: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignOptions {
    pub engines: Vec<String>,
    pub default_engine: Option<String>,
    pub charsets: Vec<CharsetInfo>,
    pub collations: Vec<CollationInfo>,
    pub index_methods: Vec<String>,
    pub capabilities: DesignCapabilities,
}
