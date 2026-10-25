use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Instant,
};

use async_trait::async_trait;

use super::connection::{RawResult, SqliteConnection, SqliteConnector};
use super::design;
use super::local::LocalConnector;
use super::metadata;
use super::options::SqliteOptions;
use super::results::{self, Timing, milliseconds};
use super::splitter;
use super::sqlgen;
use crate::db::session::{ContextId, DatabaseSession};
use crate::db::types::{
    BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, DatabaseInfo, DesignOptions, ObjectInfo, ObjectKind,
    ObjectStructure, ResultSet, RowChange, ServerInfo, SqlTemplate, StatementOutcome, TableDesign,
};
use crate::error::{AppError, AppResult};

pub struct SqliteSession {
    connector: Box<dyn SqliteConnector>,
    server: ServerInfo,
    meta: Arc<dyn SqliteConnection>,
    contexts: StdMutex<HashMap<ContextId, Arc<dyn SqliteConnection>>>,
    next_context: AtomicU32,
}

impl SqliteSession {
    pub async fn connect(options: &SqliteOptions) -> AppResult<Self> {
        Self::open(Box::new(LocalConnector::new(options)?)).await
    }

    pub async fn open(connector: Box<dyn SqliteConnector>) -> AppResult<Self> {
        let meta = connector.connect().await?;
        let version = metadata::rows(meta.as_ref(), "SELECT sqlite_version()", &[])
            .await?
            .first()
            .and_then(|row| metadata::text(row, 0))
            .ok_or_else(|| AppError::general("The database didn't report its version"))?;

        Ok(Self {
            server: ServerInfo {
                product: connector.product().to_string(),
                version,
                atomic_ddl: true,
                transactional_ddl: true,
            },
            connector,
            meta,
            contexts: StdMutex::new(HashMap::new()),
            next_context: AtomicU32::new(1),
        })
    }

    fn meta(&self) -> &dyn SqliteConnection {
        self.meta.as_ref()
    }

    fn is_internal(&self) -> impl Fn(&str) -> bool + Sync + '_ {
        |name: &str| self.connector.is_internal_table(name)
    }

    fn context(&self, id: ContextId) -> AppResult<Arc<dyn SqliteConnection>> {
        self.contexts
            .lock()
            .map_err(|_| AppError::general("Context lock poisoned"))?
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::not_found("Query context not found"))
    }

    /// When the tables can't be read the columns stay read only, and the note says why.
    async fn columns_for(&self, raw: &RawResult) -> (Vec<ColumnMeta>, Option<String>) {
        if raw.columns.is_empty() {
            return (Vec::new(), None);
        }
        match results::describe(self.meta(), raw, None).await {
            Ok(columns) => (columns, None),
            Err(error) => (
                results::basic_columns(raw),
                Some(format!("Column details couldn't be loaded: {}", error.message)),
            ),
        }
    }
}

#[async_trait]
impl DatabaseSession for SqliteSession {
    fn server_info(&self) -> ServerInfo {
        self.server.clone()
    }

    async fn list_databases(&self) -> AppResult<Vec<DatabaseInfo>> {
        metadata::list_databases(self.meta()).await
    }

    async fn list_objects(&self, database: &str) -> AppResult<Vec<ObjectInfo>> {
        metadata::list_objects(self.meta(), database, &self.is_internal()).await
    }

    async fn list_columns(&self, database: &str) -> AppResult<Vec<CatalogColumn>> {
        metadata::list_columns(self.meta(), database, &self.is_internal()).await
    }

    async fn object_structure(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        _signature: Option<&str>,
    ) -> AppResult<ObjectStructure> {
        metadata::object_structure(self.meta(), database, name, kind).await
    }

    async fn object_sql(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        _signature: Option<&str>,
        template: SqlTemplate,
    ) -> AppResult<String> {
        metadata::object_sql(self.meta(), database, name, kind, template).await
    }

    async fn database_sql(&self, database: &str, template: SqlTemplate) -> AppResult<String> {
        metadata::database_sql(self.meta(), database, template).await
    }

    async fn fetch_rows(
        &self,
        database: &str,
        table: &str,
        offset: u64,
        limit: u64,
        search: Option<&str>,
    ) -> AppResult<ResultSet> {
        metadata::fetch_rows(self.meta(), database, table, offset, limit, search).await
    }

    async fn primary_key(&self, database: &str, table: &str) -> AppResult<Vec<String>> {
        metadata::primary_key(self.meta(), database, table).await
    }

    async fn column_choices(&self, _database: &str, _table: &str, _column: &str) -> AppResult<Option<ColumnChoices>> {
        Ok(None)
    }

    fn row_changes_sql(
        &self,
        database: &str,
        table: &str,
        columns: &[ColumnMeta],
        changes: &[RowChange],
    ) -> AppResult<Vec<String>> {
        sqlgen::row_changes_sql(database, table, columns, changes)
    }

    async fn execute_batch(
        &self,
        _database: Option<&str>,
        statements: &[String],
        transactional: bool,
    ) -> AppResult<BatchOutcome> {
        let conn = self.connector.connect().await?;
        let mut outcome = conn.run_batch(statements, transactional).await;
        if let Some(error) = outcome.error.take() {
            outcome.error = Some(metadata::explain_missing_function(self.meta(), error).await);
        }
        Ok(outcome)
    }

    async fn table_design(&self, database: &str, table: &str) -> AppResult<TableDesign> {
        design::load_design(self.meta(), database, table)
            .await?
            .ok_or_else(|| AppError::not_found(format!("Table {table} not found")))
    }

    async fn design_options(&self) -> AppResult<DesignOptions> {
        Ok(design::design_options())
    }

    fn table_design_sql(
        &self,
        database: &str,
        original: Option<&TableDesign>,
        edited: &TableDesign,
    ) -> AppResult<Vec<String>> {
        match original {
            Some(original) => design::alter_sql(database, original, edited),
            None => design::create_sql(database, edited),
        }
    }

    /// Unqualified names resolve across every attached database, so a context doesn't need one picked.
    async fn open_context(&self, _database: Option<String>) -> AppResult<ContextId> {
        let conn = self.connector.connect().await?;
        let id = self.next_context.fetch_add(1, Ordering::SeqCst);
        self.contexts
            .lock()
            .map_err(|_| AppError::general("Context lock poisoned"))?
            .insert(id, conn);
        Ok(id)
    }

    async fn close_context(&self, id: ContextId) {
        if let Ok(mut contexts) = self.contexts.lock() {
            contexts.remove(&id);
        }
    }

    async fn execute(&self, id: ContextId, sql: &str, row_limit: usize) -> AppResult<Vec<StatementOutcome>> {
        let conn = self.context(id)?;
        let mut outcomes = Vec::new();

        for statement in splitter::split(sql) {
            let started = Instant::now();
            let result = conn.query(&statement.text, &[], row_limit).await;
            let first_reply = Instant::now();

            match result {
                Ok(raw) => {
                    let (columns, lookup_note) = self.columns_for(&raw).await;
                    let timing = Timing {
                        started,
                        first_reply,
                        finished: Instant::now(),
                    };
                    outcomes.push(results::outcome(&statement.text, &raw, columns, lookup_note, timing));
                }
                Err(error) => {
                    let elapsed_ms = milliseconds(first_reply - started);
                    outcomes.push(StatementOutcome {
                        sql: statement.text,
                        result_sets: Vec::new(),
                        affected_rows: 0,
                        last_insert_id: None,
                        warnings: 0,
                        info: None,
                        elapsed_ms,
                        execution_ms: elapsed_ms,
                        network_ms: None,
                        fetch_ms: 0.0,
                        error: Some(metadata::explain_missing_function(self.meta(), error).await),
                    });
                    break;
                }
            }
        }

        Ok(outcomes)
    }

    async fn cancel(&self, id: ContextId) -> AppResult<()> {
        self.context(id)?.interrupt();
        Ok(())
    }

    async fn close(&self) {
        if let Ok(mut contexts) = self.contexts.lock() {
            contexts.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::CellEdit;

    struct TempFile(std::path::PathBuf);

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    async fn session() -> (SqliteSession, TempFile) {
        let path = std::env::temp_dir().join(format!("fran-db-{}.sqlite", uuid::Uuid::new_v4()));
        let options = SqliteOptions {
            path: path.to_string_lossy().into_owned(),
            create_if_missing: true,
            ..Default::default()
        };
        (SqliteSession::connect(&options).await.unwrap(), TempFile(path))
    }

    const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT COLLATE NOCASE NOT NULL, \
                          score REAL, avatar BLOB, label TEXT GENERATED ALWAYS AS (upper(email)) VIRTUAL); \
                          CREATE UNIQUE INDEX users_email ON users (email); \
                          CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users ON DELETE CASCADE); \
                          CREATE TRIGGER touch AFTER INSERT ON posts BEGIN UPDATE users SET score = 1 WHERE id = new.user_id; END; \
                          INSERT INTO users (email, score, avatar) VALUES ('a@x.io', 2.0, X'BEEF'), ('b@x.io', NULL, NULL)";

    #[tokio::test]
    async fn refuses_missing_files() {
        let options = SqliteOptions {
            path: std::env::temp_dir().join("fran-db-missing.sqlite").to_string_lossy().into_owned(),
            ..Default::default()
        };
        assert!(SqliteSession::connect(&options).await.is_err());
    }

    #[tokio::test]
    async fn runs_scripts_and_describes_results() {
        let (session, _file) = session().await;
        let context = session.open_context(None).await.unwrap();

        let outcomes = session.execute(context, SCHEMA, 100).await.unwrap();
        assert_eq!(outcomes.len(), 5);
        assert!(outcomes.iter().all(|outcome| outcome.error.is_none()));
        assert_eq!(outcomes[4].affected_rows, 2);
        assert_eq!(outcomes[4].last_insert_id, Some(2));
        assert_eq!(outcomes[0].affected_rows, 0);

        let outcomes = session
            .execute(context, "SELECT id, email, score, avatar, 1 + 1 AS two FROM users ORDER BY id; SELECT nope", 1)
            .await
            .unwrap();
        assert_eq!(outcomes.len(), 2);
        let result = &outcomes[0].result_sets[0];
        assert!(result.truncated);
        assert_eq!(result.rows, vec![vec![
            Some("1".to_string()),
            Some("a@x.io".to_string()),
            Some("2.0".to_string()),
            Some("0xBEEF".to_string()),
            Some("2".to_string()),
        ]]);
        let id = &result.columns[0];
        assert!(id.primary_key && id.source_name.as_deref() == Some("id") && id.table.as_deref() == Some("users"));
        assert!(!result.columns[1].nullable);
        assert_eq!(result.columns[3].category, crate::db::types::ValueCategory::Binary);
        assert_eq!(result.columns[4].source_name, None);
        assert_eq!(result.columns[4].type_name, "INTEGER");
        assert!(outcomes[1].error.is_some());
    }

    #[tokio::test]
    async fn reads_metadata() {
        let (session, _file) = session().await;
        let context = session.open_context(None).await.unwrap();
        session.execute(context, SCHEMA, 100).await.unwrap();

        let databases = session.list_databases().await.unwrap();
        assert_eq!(databases.iter().map(|database| database.name.as_str()).collect::<Vec<_>>(), vec!["main"]);

        let objects = session.list_objects("main").await.unwrap();
        let names: Vec<_> = objects.iter().map(|object| object.name.as_str()).collect();
        assert_eq!(names, vec!["posts", "touch", "users"]);
        assert_eq!(objects[1].detail.as_deref(), Some("AFTER INSERT on posts"));

        let columns = session.list_columns("main").await.unwrap();
        assert_eq!(columns.len(), 7);

        let structure = session.object_structure("main", "users", ObjectKind::Table, None).await.unwrap();
        assert_eq!(structure.columns[0].extra.as_deref(), Some("AUTOINCREMENT"));
        assert_eq!(structure.columns[1].collation.as_deref(), Some("NOCASE"));
        assert_eq!(
            structure.columns[4].extra.as_deref(),
            Some("GENERATED ALWAYS AS (upper(email)) VIRTUAL")
        );
        assert_eq!(structure.indexes.len(), 2);
        assert!(structure.ddl.unwrap().contains("CREATE UNIQUE INDEX users_email"));

        let posts = session.object_structure("main", "posts", ObjectKind::Table, None).await.unwrap();
        assert_eq!(posts.foreign_keys[0].referenced_columns, vec!["id"]);
        assert_eq!(posts.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));

        assert_eq!(session.primary_key("main", "users").await.unwrap(), vec!["id"]);
        let insert = session.object_sql("main", "posts", ObjectKind::Table, None, SqlTemplate::Insert).await.unwrap();
        assert!(insert.starts_with("INSERT INTO \"main\".\"posts\""));
    }

    #[tokio::test]
    async fn edits_untyped_columns() {
        let (session, _file) = session().await;
        let context = session.open_context(None).await.unwrap();
        session
            .execute(context, "CREATE TABLE t (id PRIMARY KEY, v); INSERT INTO t VALUES (1, 'a'), ('x', 2)", 100)
            .await
            .unwrap();

        let page = session.fetch_rows("main", "t", 0, 50, None).await.unwrap();
        assert_eq!(page.columns[0].category, crate::db::types::ValueCategory::Other);
        let changes = vec![
            RowChange::Update {
                key: page.rows[0].clone(),
                values: vec![CellEdit { column: 1, value: Some("7".into()) }],
            },
            RowChange::Delete { key: page.rows[1].clone() },
        ];
        let statements = session.row_changes_sql("main", "t", &page.columns, &changes).unwrap();
        let outcome = session.execute_batch(None, &statements, true).await.unwrap();
        assert!(outcome.error.is_none(), "{:?}", outcome.error);

        let outcomes = session.execute(context, "SELECT id, v, typeof(v) FROM t", 100).await.unwrap();
        assert_eq!(outcomes[0].result_sets[0].rows, vec![vec![
            Some("1".to_string()),
            Some("7".to_string()),
            Some("integer".to_string()),
        ]]);
    }

    #[tokio::test]
    async fn edits_rows_and_tables() {
        let (session, _file) = session().await;
        let context = session.open_context(None).await.unwrap();
        session.execute(context, SCHEMA, 100).await.unwrap();

        let page = session.fetch_rows("main", "users", 0, 50, Some("B@X")).await.unwrap();
        assert_eq!(page.rows.len(), 1);
        let changes = vec![RowChange::Update {
            key: page.rows[0].clone(),
            values: vec![CellEdit { column: 2, value: Some("7.5".into()) }],
        }];
        let statements = session.row_changes_sql("main", "users", &page.columns, &changes).unwrap();
        let outcome = session.execute_batch(None, &statements, true).await.unwrap();
        assert!(outcome.error.is_none());
        let page = session.fetch_rows("main", "users", 0, 50, Some("7.5")).await.unwrap();
        assert_eq!(page.rows[0][2].as_deref(), Some("7.5"));

        let original = session.table_design("main", "users").await.unwrap();
        assert!(original.columns[0].auto_increment);
        assert_eq!(original.columns[4].generated_expression.as_deref(), Some("upper(email)"));
        let mut edited = original.clone();
        edited.columns[2].name = "points".to_string();
        let statements = session.table_design_sql("main", Some(&original), &edited).unwrap();
        let outcome = session.execute_batch(None, &statements, true).await.unwrap();
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        let reloaded = session.table_design("main", "users").await.unwrap();
        assert_eq!(reloaded.columns[2].name, "points");
        assert_eq!(reloaded, TableDesign { columns: reloaded.columns.clone(), ..original.clone() });

        let failing = vec!["DELETE FROM posts".to_string(), "INSERT INTO nope VALUES (1)".to_string()];
        let outcome = session.execute_batch(None, &failing, true).await.unwrap();
        assert_eq!(outcome.failed_index, Some(1));
        assert!(outcome.rolled_back);
    }
}

#[cfg(test)]
mod app_function_tests {
    use super::*;
    use rusqlite::functions::FunctionFlags;

    #[tokio::test]
    async fn explains_functions_only_the_creating_app_has() {
        let path = std::env::temp_dir().join(format!("fran-db-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.create_scalar_function("app_fold", 1, FunctionFlags::SQLITE_DETERMINISTIC, |context| {
                Ok(context.get::<String>(0)?.to_lowercase())
            })
            .unwrap();
            conn.execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, note TEXT); \
                 CREATE INDEX t_fold ON t (app_fold(name)); \
                 INSERT INTO t (name) VALUES ('A')",
            )
            .unwrap();
        }

        let options = SqliteOptions {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let session = SqliteSession::connect(&options).await.unwrap();

        let outcome = session
            .execute_batch(None, &["UPDATE t SET name = 'b' WHERE id = 1".to_string()], true)
            .await
            .unwrap();
        let message = outcome.error.unwrap().message;
        assert_eq!(
            message,
            "This change needs app_fold(), which the index t_fold on t uses. Only the app that created this \
             database has that function, so make the change there."
        );

        let context = session.open_context(None).await.unwrap();
        let outcomes = session.execute(context, "UPDATE t SET note = 'fine'; SELECT nope()", 10).await.unwrap();
        assert!(outcomes[0].error.is_none());
        let message = &outcomes[1].error.as_ref().unwrap().message;
        assert!(message.starts_with("no such function: nope") && !message.contains("is used by"));

        drop(session);
        let _ = std::fs::remove_file(&path);
    }
}
