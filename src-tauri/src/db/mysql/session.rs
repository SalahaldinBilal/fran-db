use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use mysql_async::{
    ClientIdentity, Compression, Conn, DriverError, Opts, OptsBuilder, SslOpts, prelude::Queryable,
};
use tokio::sync::Mutex;

use super::classify;
use super::design;
use super::metadata;
use super::options::{MysqlConnectMethod, MysqlOptions, PASSWORD_SLOT, SslMode};
use super::results::{milliseconds, run_statement};
use super::splitter;
use super::sqlgen::{self, Dialect};
use crate::connections::SecretBag;
use crate::db::classify::ClassifiedStatement;
use crate::db::read_query;
use crate::db::session::{ContextAccess, ContextId, DatabaseSession, OutcomeSink};
use crate::db::types::{
    BatchOutcome, CatalogColumn, ColumnChoices, ColumnMeta, DatabaseInfo, DatabaseSize, DesignOptions, ObjectInfo, ObjectKind, ObjectStructure,
    ReadQuery, RowChange, ServerInfo, SqlTemplate, StatementOutcome, TableDesign, TableForeignKey,
};
use crate::error::{AppError, AppResult, ErrorDetail};
use crate::forward::LocalForward;
#[cfg(windows)]
use crate::forward::open_named_pipe;
use crate::ssh::{SshTunnel, known_hosts::KnownHosts};

const PING_AFTER_IDLE: Duration = Duration::from_secs(60);

enum Endpoint {
    Tcp { host: String, port: u16 },
    #[cfg_attr(windows, allow(dead_code))]
    Socket(String),
}

/// Whatever keeps the endpoint reachable. Dropping it closes the forward.
#[allow(dead_code)]
enum Transport {
    Ssh(SshTunnel),
    Forward(LocalForward),
}

struct LiveConn {
    conn: Conn,
    last_used: Instant,
}

struct QueryContext {
    database: Option<String>,
    read_only: bool,
    slot: Mutex<Option<LiveConn>>,
    connection_id: AtomicU32,
}

pub struct MysqlSession {
    opts: Opts,
    connect_timeout: Duration,
    server: ServerInfo,
    dialect: Dialect,
    mariadb: bool,
    meta: Mutex<Option<LiveConn>>,
    contexts: StdMutex<HashMap<ContextId, Arc<QueryContext>>>,
    next_context: AtomicU32,
    transport: StdMutex<Option<Transport>>,
}

pub fn db_error(error: mysql_async::Error) -> AppError {
    match error {
        mysql_async::Error::Server(server) => AppError::new(
            server.message.clone(),
            ErrorDetail::Database {
                code: Some(server.code),
                sql_state: Some(server.state.clone()),
            },
        ),
        other => AppError::new(
            other.to_string(),
            ErrorDetail::Database {
                code: None,
                sql_state: None,
            },
        ),
    }
}

fn is_server_error(error: &mysql_async::Error) -> bool {
    matches!(error, mysql_async::Error::Server(_))
}

async fn open_conn(opts: Opts, timeout: Duration) -> Result<Conn, ConnectFailure> {
    match tokio::time::timeout(timeout, Conn::new(opts)).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(error)) => Err(ConnectFailure::Driver(error)),
        Err(_) => Err(ConnectFailure::Timeout),
    }
}

enum ConnectFailure {
    Driver(mysql_async::Error),
    Timeout,
}

impl From<ConnectFailure> for AppError {
    fn from(failure: ConnectFailure) -> Self {
        match failure {
            ConnectFailure::Driver(error) => db_error(error),
            ConnectFailure::Timeout => AppError::new(
                "Timed out connecting to the server",
                ErrorDetail::Database {
                    code: None,
                    sql_state: None,
                },
            ),
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// mysql_async has named pipes switched off, so on Windows the pipe is relayed through a local port.
#[cfg(windows)]
async fn local_socket_endpoint(raw: &str) -> AppResult<(Endpoint, Option<Transport>)> {
    let name = non_empty(raw).ok_or_else(|| AppError::general("Pipe name is empty"))?;
    let path = if name.starts_with(r"\\") {
        name
    } else {
        format!(r"\\.\pipe\{name}")
    };

    drop(
        open_named_pipe(&path)
            .await
            .map_err(|error| AppError::general(format!("Could not open pipe {path}: {error}")))?,
    );

    let forward = LocalForward::start(0, move |_| {
        let path = path.clone();
        async move { open_named_pipe(&path).await }
    })
    .await?;

    Ok((
        Endpoint::Tcp {
            host: "127.0.0.1".to_string(),
            port: forward.port,
        },
        Some(Transport::Forward(forward)),
    ))
}

#[cfg(not(windows))]
async fn local_socket_endpoint(raw: &str) -> AppResult<(Endpoint, Option<Transport>)> {
    let path = non_empty(raw).ok_or_else(|| AppError::general("Socket path is empty"))?;
    Ok((Endpoint::Socket(path), None))
}

fn ssl_opts(options: &MysqlOptions, tunneled: bool) -> AppResult<Option<SslOpts>> {
    let ssl = &options.ssl;
    if ssl.mode == SslMode::Disabled || options.method == MysqlConnectMethod::Socket {
        return Ok(None);
    }

    let mut opts = SslOpts::default();

    if let Some(ca_file) = non_empty(&ssl.ca_file) {
        opts = opts.with_root_certs(vec![PathBuf::from(ca_file).into()]);
    }

    match (non_empty(&ssl.cert_file), non_empty(&ssl.key_file)) {
        (Some(cert), Some(key)) => {
            opts = opts.with_client_identity(Some(ClientIdentity::new(
                PathBuf::from(cert).into(),
                PathBuf::from(key).into(),
            )));
        }
        (None, None) => {}
        _ => {
            return Err(AppError::general(
                "SSL needs both a client certificate and a key file, or neither",
            ));
        }
    }

    opts = match ssl.mode {
        SslMode::Preferred | SslMode::Required => opts
            .with_danger_accept_invalid_certs(true)
            .with_danger_skip_domain_validation(true),
        SslMode::VerifyCa => opts.with_danger_skip_domain_validation(true),
        SslMode::VerifyIdentity | SslMode::Disabled => opts,
    };

    if tunneled {
        opts = opts.with_danger_tls_hostname_override(Some(options.host.clone()));
    }

    Ok(Some(opts))
}

fn build_opts(options: &MysqlOptions, endpoint: &Endpoint, password: &str, ssl: Option<SslOpts>) -> Opts {
    let init: Vec<String> = splitter::split(&options.init_sql)
        .into_iter()
        .map(|statement| statement.text)
        .collect();

    let builder = OptsBuilder::default()
        .user(Some(options.user.clone()))
        .pass(Some(password.to_string()))
        .db_name(non_empty(&options.default_schema))
        .prefer_socket(false)
        .ssl_opts(ssl)
        .compression(options.compression.then(Compression::fast))
        .enable_cleartext_plugin(options.cleartext_auth)
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .init(init);

    let builder = match endpoint {
        Endpoint::Tcp { host, port } => builder.ip_or_hostname(host.clone()).tcp_port(*port),
        Endpoint::Socket(path) => builder.socket(Some(path.clone())),
    };

    Opts::from(builder)
}

impl MysqlSession {
    pub async fn connect(
        options: &MysqlOptions,
        secrets: &SecretBag,
        known_hosts: &KnownHosts,
    ) -> AppResult<Self> {
        let connect_timeout = Duration::from_secs(u64::from(options.connect_timeout_secs.max(1)));
        let password = secrets
            .get(PASSWORD_SLOT)
            .ok_or_else(|| AppError::secrets_required(vec![MysqlOptions::password_slot()]))?;

        let (endpoint, transport) = match options.method {
            MysqlConnectMethod::Tcp => (
                Endpoint::Tcp {
                    host: options.host.trim().to_string(),
                    port: options.port,
                },
                None,
            ),
            MysqlConnectMethod::Socket => local_socket_endpoint(&options.socket_path).await?,
            MysqlConnectMethod::SshTunnel => {
                let tunnel = SshTunnel::open(
                    &options.ssh,
                    secrets,
                    known_hosts,
                    options.host.trim(),
                    options.port,
                    0,
                    connect_timeout,
                )
                .await?;
                (
                    Endpoint::Tcp {
                        host: "127.0.0.1".to_string(),
                        port: tunnel.local_port(),
                    },
                    Some(Transport::Ssh(tunnel)),
                )
            }
        };

        let tunneled = matches!(transport, Some(Transport::Ssh(_)));
        let opts = build_opts(options, &endpoint, password, ssl_opts(options, tunneled)?);

        let (opts, mut conn) = match open_conn(opts.clone(), connect_timeout).await {
            Ok(conn) => (opts, conn),
            Err(ConnectFailure::Driver(mysql_async::Error::Driver(DriverError::NoClientSslFlagFromServer)))
                if options.ssl.mode == SslMode::Preferred =>
            {
                let plain = build_opts(options, &endpoint, password, None);
                let conn = open_conn(plain.clone(), connect_timeout).await?;
                (plain, conn)
            }
            Err(failure) => return Err(failure.into()),
        };

        let server = metadata::server_info(&mut conn).await.map_err(db_error)?;
        let dialect = metadata::dialect(&mut conn).await.map_err(db_error)?;
        let mariadb = server.product == "MariaDB";

        Ok(Self {
            opts,
            connect_timeout,
            server,
            dialect,
            mariadb,
            meta: Mutex::new(Some(LiveConn {
                conn,
                last_used: Instant::now(),
            })),
            contexts: StdMutex::new(HashMap::new()),
            next_context: AtomicU32::new(1),
            transport: StdMutex::new(transport),
        })
    }

    async fn ensure_live<'a>(
        &self,
        slot: &'a mut Option<LiveConn>,
        database: Option<&str>,
    ) -> AppResult<&'a mut LiveConn> {
        self.ensure_live_as(slot, database, false).await
    }

    /// A read-only connection is set read-only before anything runs on it, on every reconnect too.
    async fn ensure_live_as<'a>(
        &self,
        slot: &'a mut Option<LiveConn>,
        database: Option<&str>,
        read_only: bool,
    ) -> AppResult<&'a mut LiveConn> {
        let needs_new = match slot {
            Some(live) => live.last_used.elapsed() > PING_AFTER_IDLE && live.conn.ping().await.is_err(),
            None => true,
        };

        if needs_new {
            *slot = None;
            let mut conn = open_conn(self.opts_for(database), self.connect_timeout).await?;
            if read_only && let Err(error) = conn.query_drop("SET SESSION TRANSACTION READ ONLY").await {
                let _ = conn.disconnect().await;
                return Err(db_error(error));
            }
            *slot = Some(LiveConn {
                conn,
                last_used: Instant::now(),
            });
        }

        let live = slot
            .as_mut()
            .ok_or_else(|| AppError::general("Connection could not be opened"))?;
        live.last_used = Instant::now();
        Ok(live)
    }

    fn opts_for(&self, database: Option<&str>) -> Opts {
        match database {
            Some(database) => Opts::from(OptsBuilder::from_opts(self.opts.clone()).db_name(Some(database))),
            None => self.opts.clone(),
        }
    }

    fn context(&self, id: ContextId) -> AppResult<Arc<QueryContext>> {
        self.contexts
            .lock()
            .map_err(|_| AppError::general("Context lock poisoned"))?
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::not_found("Query context not found"))
    }
}

async fn run_batch(conn: &mut Conn, statements: &[String], transactional: bool) -> BatchOutcome {
    let failure = |executed: usize, failed_index: Option<usize>, error: mysql_async::Error, rolled_back: bool| BatchOutcome {
        executed,
        failed_index,
        error: Some(db_error(error)),
        rolled_back,
    };

    if transactional && let Err(error) = conn.query_drop("START TRANSACTION").await {
        return failure(0, None, error, false);
    }

    for (index, statement) in statements.iter().enumerate() {
        if let Err(error) = conn.query_drop(statement.as_str()).await {
            let rolled_back = transactional && conn.query_drop("ROLLBACK").await.is_ok();
            return failure(index, Some(index), error, rolled_back);
        }
    }

    if transactional && let Err(error) = conn.query_drop("COMMIT").await {
        let rolled_back = conn.query_drop("ROLLBACK").await.is_ok();
        return failure(statements.len(), None, error, rolled_back);
    }

    BatchOutcome {
        executed: statements.len(),
        failed_index: None,
        error: None,
        rolled_back: false,
    }
}

/// Drops a connection that failed below the SQL level so the next call reconnects.
fn settle<T>(slot: &mut Option<LiveConn>, result: Result<T, mysql_async::Error>) -> AppResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if !is_server_error(&error) {
                *slot = None;
            }
            Err(db_error(error))
        }
    }
}

#[async_trait]
impl DatabaseSession for MysqlSession {
    fn server_info(&self) -> ServerInfo {
        self.server.clone()
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
        _signature: Option<&str>,
    ) -> AppResult<ObjectStructure> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::object_structure(&mut live.conn, database, name, kind).await;
        settle(&mut slot, result)
    }

    async fn object_sql(
        &self,
        database: &str,
        name: &str,
        kind: ObjectKind,
        _signature: Option<&str>,
        template: SqlTemplate,
    ) -> AppResult<String> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        metadata::object_sql(&mut live.conn, database, name, kind, template).await
    }

    async fn database_sql(&self, database: &str, template: SqlTemplate) -> AppResult<String> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        metadata::database_sql(&mut live.conn, database, template).await
    }

    fn row_changes_sql(
        &self,
        database: &str,
        table: &str,
        columns: &[ColumnMeta],
        changes: &[RowChange],
    ) -> AppResult<Vec<String>> {
        sqlgen::row_changes_sql(self.dialect, database, table, columns, changes)
    }

    async fn execute_batch(
        &self,
        database: Option<&str>,
        statements: &[String],
        transactional: bool,
    ) -> AppResult<BatchOutcome> {
        let mut conn = open_conn(self.opts_for(database), self.connect_timeout).await?;
        let outcome = run_batch(&mut conn, statements, transactional).await;
        let _ = conn.disconnect().await;
        Ok(outcome)
    }

    async fn table_design(&self, database: &str, table: &str) -> AppResult<TableDesign> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = design::load_design(&mut live.conn, self.dialect, self.mariadb, database, table).await;
        settle(&mut slot, result)?.ok_or_else(|| AppError::not_found(format!("Table {table} not found")))
    }

    async fn design_options(&self) -> AppResult<DesignOptions> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = design::design_options(&mut live.conn).await;
        settle(&mut slot, result)
    }

    fn table_design_sql(
        &self,
        database: &str,
        original: Option<&TableDesign>,
        design: &TableDesign,
    ) -> AppResult<Vec<String>> {
        match original {
            Some(original) => design::alter_sql(self.dialect, database, original, design),
            None => design::create_sql(self.dialect, database, design),
        }
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

    async fn foreign_keys(&self, database: &str) -> AppResult<Vec<TableForeignKey>> {
        let mut slot = self.meta.lock().await;
        let live = self.ensure_live(&mut slot, None).await?;
        let result = metadata::database_foreign_keys(&mut live.conn, database).await;
        settle(&mut slot, result)
    }

    fn classify(&self, sql: &str) -> Vec<ClassifiedStatement> {
        classify::classify(sql, self.dialect)
    }

    fn read_sql(&self, database: &str, query: &ReadQuery) -> AppResult<String> {
        read_query::read_sql(&self.dialect, database, query)
    }

    fn explain_sql(&self, sql: &str) -> String {
        format!("EXPLAIN {sql}")
    }

    async fn open_context(&self, database: Option<String>, access: ContextAccess) -> AppResult<ContextId> {
        let context = Arc::new(QueryContext {
            database,
            read_only: access == ContextAccess::ReadOnly,
            slot: Mutex::new(None),
            connection_id: AtomicU32::new(0),
        });

        {
            let mut slot = context.slot.lock().await;
            let live = self
                .ensure_live_as(&mut slot, context.database.as_deref(), context.read_only)
                .await?;
            context.connection_id.store(live.conn.id(), Ordering::SeqCst);
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
                let _ = live.conn.disconnect().await;
            }
        }
    }

    async fn execute_streaming(
        &self,
        id: ContextId,
        sql: &str,
        row_limit: usize,
        on_outcome: OutcomeSink<'_>,
    ) -> AppResult<()> {
        let context = self.context(id)?;
        let statements = splitter::split(sql);

        let mut slot = context.slot.lock().await;
        let live = self
            .ensure_live_as(&mut slot, context.database.as_deref(), context.read_only)
            .await?;
        context.connection_id.store(live.conn.id(), Ordering::SeqCst);

        // A ping does no server work, so its time is the network round trip on its own.
        let ping_started = Instant::now();
        let network_ms = live.conn.ping().await.ok().map(|()| milliseconds(ping_started.elapsed()));
        let without_network = |ms: f64| (ms - network_ms.unwrap_or(0.0)).max(0.0);

        let mut lost_connection = false;

        for statement in statements {
            let started = Instant::now();
            let result = run_statement(&mut live.conn, &statement.text, row_limit).await;
            let elapsed_ms = milliseconds(started.elapsed());

            match result {
                Ok(mut outcome) => {
                    outcome.elapsed_ms = elapsed_ms;
                    outcome.execution_ms = without_network(outcome.execution_ms);
                    outcome.network_ms = network_ms;
                    on_outcome(outcome);
                }
                Err(error) => {
                    lost_connection = !is_server_error(&error);
                    on_outcome(StatementOutcome {
                        sql: statement.text,
                        result_sets: Vec::new(),
                        affected_rows: 0,
                        last_insert_id: None,
                        warnings: 0,
                        info: None,
                        elapsed_ms,
                        execution_ms: without_network(elapsed_ms),
                        network_ms,
                        fetch_ms: 0.0,
                        error: Some(db_error(error)),
                    });
                    break;
                }
            }
        }

        live.last_used = Instant::now();
        if lost_connection {
            *slot = None;
        }

        Ok(())
    }

    async fn cancel(&self, id: ContextId) -> AppResult<()> {
        let connection_id = self.context(id)?.connection_id.load(Ordering::SeqCst);
        if connection_id == 0 {
            return Ok(());
        }

        let mut conn = open_conn(self.opts.clone(), self.connect_timeout).await?;
        let result = conn.query_drop(format!("KILL QUERY {connection_id}")).await;
        let _ = conn.disconnect().await;
        result.map_err(db_error)
    }

    async fn close(&self) {
        let contexts: Vec<Arc<QueryContext>> = match self.contexts.lock() {
            Ok(mut contexts) => contexts.drain().map(|(_, context)| context).collect(),
            Err(_) => Vec::new(),
        };

        for context in contexts {
            let taken = context.slot.lock().await.take();
            if let Some(live) = taken {
                let _ = live.conn.disconnect().await;
            }
        }

        let meta = self.meta.lock().await.take();
        if let Some(live) = meta {
            let _ = live.conn.disconnect().await;
        }

        if let Ok(mut transport) = self.transport.lock() {
            transport.take();
        }
    }
}
