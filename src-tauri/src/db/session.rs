use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;

use super::classify::ClassifiedStatement;
use super::types::{
    BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, DatabaseInfo, DatabaseSize, DesignOptions, ObjectInfo, ObjectKind,
    ObjectStructure, ReadQuery, RowChange, ServerInfo, SqlTemplate, StatementOutcome, TableDesign, TableForeignKey,
};
use crate::error::{AppError, AppResult};

pub type ContextId = u32;

/// A read-only context is held read-only by the server where the server can do that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAccess {
    ReadWrite,
    ReadOnly,
}

pub type OutcomeSink<'a> = &'a (dyn Fn(StatementOutcome) + Send + Sync);

/// Everything outside a driver module talks to an open connection through this trait.
/// A context is a dedicated connection with its own session state, one per query tab.
#[async_trait]
pub trait DatabaseSession: Send + Sync {
    fn server_info(&self) -> ServerInfo;

    /// The database this session works in, for servers where a connection is bound to one database.
    fn catalog(&self) -> Option<String> {
        None
    }

    /// Databases the session can switch to with `switch_catalog`. Empty where that doesn't apply.
    async fn list_catalogs(&self) -> AppResult<Vec<String>> {
        Ok(Vec::new())
    }

    /// Moves the whole session to another database. Open query contexts are closed.
    async fn switch_catalog(&self, _catalog: &str) -> AppResult<()> {
        Err(AppError::general("This connection has no other databases to switch to"))
    }

    async fn list_databases(&self) -> AppResult<Vec<DatabaseInfo>>;

    /// Kept apart from `list_databases` because summing sizes can be slow on big servers.
    async fn database_sizes(&self) -> AppResult<Vec<DatabaseSize>>;

    async fn list_objects(&self, database: &str) -> AppResult<Vec<ObjectInfo>>;

    async fn list_columns(&self, database: &str) -> AppResult<Vec<CatalogColumn>>;

    async fn object_structure(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        signature: Option<&str>,
    ) -> AppResult<ObjectStructure>;

    async fn object_sql(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        signature: Option<&str>,
        template: SqlTemplate,
    ) -> AppResult<String>;

    async fn database_sql(&self, database: &str, template: SqlTemplate) -> AppResult<String>;

    /// Column names of the table's primary key, empty when it has none or isn't a base table.
    async fn primary_key(&self, database: &str, table: &str) -> AppResult<Vec<String>>;

    /// The allowed values of an ENUM or SET style column, None for any other column.
    async fn column_choices(&self, database: &str, table: &str, column: &str) -> AppResult<Option<ColumnChoices>>;

    /// Every foreign key held by a table in `database` or pointing into it.
    async fn foreign_keys(&self, database: &str) -> AppResult<Vec<TableForeignKey>>;

    /// Splits `sql` into statements and tells which ones only read, by this session's quoting rules.
    fn classify(&self, sql: &str) -> Vec<ClassifiedStatement>;

    fn read_sql(&self, database: &str, query: &ReadQuery) -> AppResult<String>;

    /// The statement that shows how the server would run `sql`, without running it.
    fn explain_sql(&self, sql: &str) -> String;

    fn row_changes_sql(
        &self,
        database: &str,
        table: &str,
        columns: &[ColumnMeta],
        changes: &[RowChange],
    ) -> AppResult<Vec<String>>;

    /// Runs statements on a fresh connection, in a transaction when asked, stopping at the first failure.
    async fn execute_batch(
        &self,
        database: Option<&str>,
        statements: &[String],
        transactional: bool,
    ) -> AppResult<BatchOutcome>;

    async fn table_design(&self, database: &str, table: &str) -> AppResult<TableDesign>;

    async fn design_options(&self) -> AppResult<DesignOptions>;

    fn table_design_sql(
        &self,
        database: &str,
        original: Option<&TableDesign>,
        design: &TableDesign,
    ) -> AppResult<Vec<String>>;

    async fn open_context(&self, database: Option<String>, access: ContextAccess) -> AppResult<ContextId>;

    async fn close_context(&self, context: ContextId);

    /// Runs every statement in `sql` in order and stops at the first failure,
    /// which is reported on that statement's outcome.
    async fn execute(&self, context: ContextId, sql: &str, row_limit: usize) -> AppResult<Vec<StatementOutcome>> {
        let outcomes = Mutex::new(Vec::new());
        self.execute_streaming(context, sql, row_limit, &|outcome| {
            if let Ok(mut outcomes) = outcomes.lock() {
                outcomes.push(outcome);
            }
        })
        .await?;
        Ok(outcomes.into_inner().unwrap_or_else(PoisonError::into_inner))
    }

    /// `execute`, handing each statement's outcome to `on_outcome` as soon as it finishes.
    async fn execute_streaming(
        &self,
        context: ContextId,
        sql: &str,
        row_limit: usize,
        on_outcome: OutcomeSink<'_>,
    ) -> AppResult<()>;

    async fn cancel(&self, context: ContextId) -> AppResult<()>;

    async fn close(&self);
}
