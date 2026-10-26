use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex, RwLock,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::client::{CancelKey, PgConnection, PgError, RawResult};
use super::connect::Connector;
use super::design::{self, Server};
use super::metadata;
use super::options::PostgresOptions;
use super::results::{self, Timing, milliseconds};
use super::splitter;
use super::sqlgen::{self, Dialect};
use super::types::TypeCache;
use crate::connections::SecretBag;
use crate::db::session::{ContextId, DatabaseSession};
use crate::db::types::{
    BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, DatabaseInfo, DatabaseSize, DesignOptions, ObjectInfo, ObjectKind,
    ObjectStructure, RowChange, ServerInfo, SqlTemplate, StatementOutcome, TableDesign,
};
use crate::error::{AppError, AppResult};
use crate::ssh::{SshTunnel, known_hosts::KnownHosts};

const PING_AFTER_IDLE: Duration = Duration::from_secs(60);

struct LiveConn {
    conn: PgConnection,
    last_used: Instant,
}

struct QueryContext {
    schema: Option<String>,
    slot: Mutex<Option<LiveConn>>,
    cancel_key: StdMutex<Option<CancelKey>>,
}

impl QueryContext {
    fn remember_key(&self, key: Option<CancelKey>) {
        if let Ok(mut current) = self.cancel_key.lock() {
            *current = key;
        }
    }

    fn key(&self) -> Option<CancelKey> {
        self.cancel_key.lock().ok().and_then(|key| *key)
    }
}

/// The database the session works in. Replaced as a whole when the user switches databases.
struct Target {
    connector: Connector,
    dialect: Dialect,
}

pub struct PostgresSession {
    target: RwLock<Arc<Target>>,
    server: ServerInfo,
    major_version: u32,
    types: TypeCache,
    meta: Mutex<Option<LiveConn>>,
    contexts: StdMutex<HashMap<ContextId, Arc<QueryContext>>>,
    next_context: AtomicU32,
    tunnel: StdMutex<Option<SshTunnel>>,
}

/// Drops a connection that failed below the SQL level so the next call reconnects.
fn settle<T>(slot: &mut Option<LiveConn>, result: Result<T, PgError>) -> AppResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if error.connection_lost() {
                *slot = None;
            }
            Err(error.into())
        }
    }
}

fn dialect_of(conn: &PgConnection) -> Dialect {
    Dialect {
        standard_conforming_strings: conn.parameter("standard_conforming_strings") != Some("off"),
    }
}

async fn run_batch(conn: &mut PgConnection, statements: &[String], transactional: bool) -> BatchOutcome {
    let failure = |executed: usize, failed_index: Option<usize>, error: PgError, rolled_back: bool| BatchOutcome {
        executed,
        failed_index,
        error: Some(error.into()),
        rolled_back,
    };

    if transactional && let Err(error) = conn.execute("BEGIN").await {
        return failure(0, None, error, false);
    }

    for (index, statement) in statements.iter().enumerate() {
        if let Err(error) = conn.execute(statement).await {
            let rolled_back = transactional && conn.execute("ROLLBACK").await.is_ok();
            return failure(index, Some(index), error, rolled_back);
        }
    }

    if transactional && let Err(error) = conn.execute("COMMIT").await {
        let rolled_back = conn.execute("ROLLBACK").await.is_ok();
        return failure(statements.len(), None, error, rolled_back);
    }

    BatchOutcome {
        executed: statements.len(),
        failed_index: None,
        error: None,
        rolled_back: false,
    }
}

impl PostgresSession {
    pub async fn connect(options: &PostgresOptions, secrets: &SecretBag, known_hosts: &KnownHosts) -> AppResult<Self> {
        let (connector, tunnel) = Connector::prepare(options, secrets, known_hosts).await?;
        let mut conn = connector.connect(None).await?;

        let version = match conn.parameter("server_version") {
            Some(version) => version.to_string(),
            None => conn
                .query_text("SHOW server_version", &[])
                .await?
                .first()
                .and_then(|row| metadata::text(row, 0))
                .ok_or_else(|| AppError::general("The server didn't report its version"))?,
        };
        let dialect = dialect_of(&conn);

        Ok(Self {
            target: RwLock::new(Arc::new(Target { connector, dialect })),
            major_version: design::major_version(&version),
            server: ServerInfo {
                product: "PostgreSQL".to_string(),
                version,
                atomic_ddl: true,
                transactional_ddl: true,
            },
            types: TypeCache::default(),
            meta: Mutex::new(Some(LiveConn {
                conn,
                last_used: Instant::now(),
            })),
            contexts: StdMutex::new(HashMap::new()),
            next_context: AtomicU32::new(1),
            tunnel: StdMutex::new(tunnel),
        })
    }

    fn target(&self) -> Arc<Target> {
        match self.target.read() {
            Ok(target) => target.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn dialect(&self) -> Dialect {
        self.target().dialect
    }

    fn design_server(&self) -> Server {
        Server {
            dialect: self.dialect(),
            major_version: self.major_version,
        }
    }

    async fn ensure_live<'a>(&self, slot: &'a mut Option<LiveConn>, schema: Option<&str>) -> AppResult<&'a mut LiveConn> {
        let needs_new = match slot {
            Some(live) => live.last_used.elapsed() > PING_AFTER_IDLE && live.conn.ping().await.is_err(),
            None => true,
        };

        if needs_new {
            let target = self.target();
            *slot = Some(LiveConn {
                conn: target.connector.connect(schema).await?,
                last_used: Instant::now(),
            });
        }

        let live = slot
            .as_mut()
            .ok_or_else(|| AppError::general("Connection could not be opened"))?;
        live.last_used = Instant::now();
        Ok(live)
    }

    fn context(&self, id: ContextId) -> AppResult<Arc<QueryContext>> {
        self.contexts
            .lock()
            .map_err(|_| AppError::general("Context lock poisoned"))?
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::not_found("Query context not found"))
    }

    async fn close_contexts(&self) {
        let contexts: Vec<Arc<QueryContext>> = match self.contexts.lock() {
            Ok(mut contexts) => contexts.drain().map(|(_, context)| context).collect(),
            Err(_) => Vec::new(),
        };

        for context in contexts {
            let taken = context.slot.lock().await.take();
            if let Some(live) = taken {
                live.conn.close().await;
            }
        }
    }

    async fn describe_results(&self, raw_results: &[RawResult]) -> AppResult<Vec<Vec<ColumnMeta>>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;

        let mut described = Vec::with_capacity(raw_results.len());
        let mut failure = None;
        for result in raw_results {
            match results::describe(&mut live.conn, &self.types, &result.columns).await {
                Ok(columns) => described.push(columns),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }

        match failure {
            Some(error) => settle(&mut slot, Err(error)),
            None => Ok(described),
        }
    }

    /// Column details from the catalog. When the catalog can't be read the columns stay read only,
    /// and the note says why.
    async fn columns_for(&self, raw_results: &[RawResult]) -> (Vec<Vec<ColumnMeta>>, Option<String>) {
        if raw_results.is_empty() {
            return (Vec::new(), None);
        }
        match self.describe_results(raw_results).await {
            Ok(columns) => (columns, None),
            Err(error) => (
                raw_results.iter().map(|result| results::basic_columns(&result.columns)).collect(),
                Some(format!("Column details couldn't be loaded: {}", error.message)),
            ),
        }
    }
}

#[async_trait]
impl DatabaseSession for PostgresSession {
    fn server_info(&self) -> ServerInfo {
        self.server.clone()
    }

    fn catalog(&self) -> Option<String> {
        Some(self.target().connector.database().to_string())
    }

    async fn list_catalogs(&self) -> AppResult<Vec<String>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::list_catalogs(&mut live.conn).await;
        settle(&mut slot, result)
    }

    async fn switch_catalog(&self, catalog: &str) -> AppResult<()> {
        let connector = self.target().connector.with_database(catalog);
        let conn = connector.connect(None).await?;
        let dialect = dialect_of(&conn);

        self.close_contexts().await;

        let mut meta = self.meta.lock().await;
        if let Some(old) = meta.take() {
            old.conn.close().await;
        }
        *meta = Some(LiveConn {
            conn,
            last_used: Instant::now(),
        });

        if let Ok(mut types) = self.types.lock() {
            types.clear();
        }
        let next = Arc::new(Target { connector, dialect });
        match self.target.write() {
            Ok(mut target) => *target = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
        Ok(())
    }

    async fn list_databases(&self) -> AppResult<Vec<DatabaseInfo>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::list_databases(&mut live.conn).await;
        settle(&mut slot, result)
    }

    async fn database_sizes(&self) -> AppResult<Vec<DatabaseSize>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::database_sizes(&mut live.conn).await;
        settle(&mut slot, result)
    }

    async fn list_objects(&self, database: &str) -> AppResult<Vec<ObjectInfo>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::list_objects(&mut live.conn, database).await;
        settle(&mut slot, result)
    }

    async fn list_columns(&self, database: &str) -> AppResult<Vec<CatalogColumn>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::list_columns(&mut live.conn, database).await;
        settle(&mut slot, result)
    }

    async fn object_structure(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        signature: Option<&str>,
    ) -> AppResult<ObjectStructure> {
        let dialect = self.dialect();
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::object_structure(&mut live.conn, dialect, database, name, kind, signature).await;
        settle(&mut slot, result)
    }

    async fn object_sql(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        signature: Option<&str>,
        template: SqlTemplate,
    ) -> AppResult<String> {
        let dialect = self.dialect();
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::object_sql(&mut live.conn, dialect, database, name, kind, signature, template).await;
        settle(&mut slot, result)
    }

    async fn database_sql(&self, database: &str, template: SqlTemplate) -> AppResult<String> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::database_sql(&mut live.conn, database, template).await;
        settle(&mut slot, result)
    }

    async fn primary_key(&self, database: &str, table: &str) -> AppResult<Vec<String>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::primary_key(&mut live.conn, database, table).await;
        settle(&mut slot, result)
    }

    async fn column_choices(&self, database: &str, table: &str, column: &str) -> AppResult<Option<ColumnChoices>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::column_choices(&mut live.conn, database, table, column).await;
        settle(&mut slot, result)
    }

    fn row_changes_sql(
        &self,
        database: &str,
        table: &str,
        columns: &[ColumnMeta],
        changes: &[RowChange],
    ) -> AppResult<Vec<String>> {
        sqlgen::row_changes_sql(self.dialect(), database, table, columns, changes)
    }

    async fn execute_batch(
        &self,
        database: Option<&str>,
        statements: &[String],
        transactional: bool,
    ) -> AppResult<BatchOutcome> {
        let target = self.target();
        let mut conn = target.connector.connect(database).await?;
        let outcome = run_batch(&mut conn, statements, transactional).await;
        conn.close().await;
        Ok(outcome)
    }

    async fn table_design(&self, database: &str, table: &str) -> AppResult<TableDesign> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = design::load_design(&mut live.conn, database, table).await;
        settle(&mut slot, result)?.ok_or_else(|| AppError::not_found(format!("Table {table} not found")))
    }

    async fn design_options(&self) -> AppResult<DesignOptions> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = design::design_options(&mut live.conn, self.major_version).await;
        settle(&mut slot, result)
    }

    fn table_design_sql(
        &self,
        database: &str,
        original: Option<&TableDesign>,
        edited: &TableDesign,
    ) -> AppResult<Vec<String>> {
        match original {
            Some(original) => design::alter_sql(self.design_server(), database, original, edited),
            None => design::create_sql(self.design_server(), database, edited),
        }
    }

    async fn open_context(&self, database: Option<String>) -> AppResult<ContextId> {
        let context = Arc::new(QueryContext {
            schema: database,
            slot: Mutex::new(None),
            cancel_key: StdMutex::new(None),
        });

        {
            let mut slot = context.slot.lock().await;
            let live = self.ensure_live(&mut slot, context.schema.as_deref()).await?;
            context.remember_key(live.conn.cancel_key());
        }

        let id = self.next_context.fetch_add(1, Ordering::SeqCst);
        self.contexts
            .lock()
            .map_err(|_| AppError::general("Context lock poisoned"))?
            .insert(id, context);
        Ok(id)
    }

    async fn close_context(&self, id: ContextId) {
        let context = self.contexts.lock().ok().and_then(|mut contexts| contexts.remove(&id));

        if let Some(context) = context {
            let taken = context.slot.lock().await.take();
            if let Some(live) = taken {
                live.conn.close().await;
            }
        }
    }

    async fn execute(&self, id: ContextId, sql: &str, row_limit: usize) -> AppResult<Vec<StatementOutcome>> {
        let context = self.context(id)?;
        let statements = splitter::split(sql);

        let mut slot = context.slot.lock().await;
        let live = self.ensure_live(&mut slot, context.schema.as_deref()).await?;
        context.remember_key(live.conn.cancel_key());

        // An empty query does no server work, so its time is the network round trip on its own.
        let ping_started = Instant::now();
        let network_ms = live.conn.ping().await.ok().map(|()| milliseconds(ping_started.elapsed()));
        let without_network = |ms: f64| (ms - network_ms.unwrap_or(0.0)).max(0.0);
        live.conn.take_notices();

        let mut outcomes = Vec::with_capacity(statements.len());
        let mut lost_connection = false;

        for statement in statements {
            let started = Instant::now();
            let result = live.conn.simple_query(&statement.text, row_limit).await;
            let finished = Instant::now();
            let notices = live.conn.take_notices();

            match result {
                Ok(raw) => {
                    let (columns, lookup_note) = self.columns_for(&raw.results).await;
                    let mut outcome = results::outcome(
                        &statement.text,
                        raw,
                        columns,
                        &notices,
                        lookup_note,
                        Timing { started, finished },
                    );
                    outcome.execution_ms = without_network(outcome.execution_ms);
                    outcome.network_ms = network_ms;
                    outcomes.push(outcome);
                }
                Err(error) => {
                    lost_connection = error.connection_lost();
                    let elapsed_ms = milliseconds(finished - started);
                    let info = results::notice_text(&notices);
                    outcomes.push(StatementOutcome {
                        sql: statement.text,
                        result_sets: Vec::new(),
                        affected_rows: 0,
                        last_insert_id: None,
                        warnings: results::warning_count(&notices),
                        info: (!info.is_empty()).then(|| info.join("\n")),
                        elapsed_ms,
                        execution_ms: without_network(elapsed_ms),
                        network_ms,
                        fetch_ms: 0.0,
                        error: Some(error.into()),
                    });
                    break;
                }
            }
        }

        live.last_used = Instant::now();
        if lost_connection {
            *slot = None;
        }

        Ok(outcomes)
    }

    async fn cancel(&self, id: ContextId) -> AppResult<()> {
        let Some(key) = self.context(id)?.key() else {
            return Ok(());
        };
        let target = self.target();
        target.connector.cancel(key).await.map_err(Into::into)
    }

    async fn close(&self) {
        self.close_contexts().await;

        let meta = self.meta.lock().await.take();
        if let Some(live) = meta {
            live.conn.close().await;
        }

        if let Ok(mut tunnel) = self.tunnel.lock() {
            tunnel.take();
        }
    }
}
