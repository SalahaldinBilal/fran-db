use std::sync::Arc;

use async_trait::async_trait;

use crate::db::types::BatchOutcome;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// The source fields are None for expressions, and for backends that can't tell.
#[derive(Debug, Clone, Default)]
pub struct RawColumn {
    pub name: String,
    pub decl_type: Option<String>,
    pub schema: Option<String>,
    pub table: Option<String>,
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RawResult {
    pub columns: Vec<RawColumn>,
    pub rows: Vec<Vec<SqlValue>>,
    pub truncated: bool,
    pub changes: u64,
    /// Set only when the statement inserted a row.
    pub last_insert_id: Option<i64>,
    pub server_ms: Option<f64>,
}

#[async_trait]
pub trait SqliteConnection: Send + Sync {
    /// Runs exactly one statement.
    async fn query(&self, sql: &str, params: &[SqlValue], row_limit: usize) -> AppResult<RawResult>;

    fn interrupt(&self) {}

    /// Backends without BEGIN and COMMIT across calls override this with their own batch.
    async fn run_batch(&self, statements: &[String], transactional: bool) -> BatchOutcome {
        let failure = |executed: usize, failed_index: Option<usize>, error: AppError, rolled_back: bool| BatchOutcome {
            executed,
            failed_index,
            error: Some(error),
            rolled_back,
        };

        if transactional && let Err(error) = self.query("BEGIN", &[], 0).await {
            return failure(0, None, error, false);
        }

        for (index, statement) in statements.iter().enumerate() {
            if let Err(error) = self.query(statement, &[], 0).await {
                let rolled_back = transactional && self.query("ROLLBACK", &[], 0).await.is_ok();
                return failure(index, Some(index), error, rolled_back);
            }
        }

        if transactional && let Err(error) = self.query("COMMIT", &[], 0).await {
            let rolled_back = self.query("ROLLBACK", &[], 0).await.is_ok();
            return failure(statements.len(), None, error, rolled_back);
        }

        BatchOutcome {
            executed: statements.len(),
            failed_index: None,
            error: None,
            rolled_back: false,
        }
    }
}

#[async_trait]
pub trait SqliteConnector: Send + Sync {
    fn product(&self) -> &'static str;

    async fn connect(&self) -> AppResult<Arc<dyn SqliteConnection>>;

    fn is_internal_table(&self, name: &str) -> bool {
        name.starts_with("sqlite_")
    }
}
