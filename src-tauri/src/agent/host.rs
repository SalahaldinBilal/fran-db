use std::{
    collections::HashMap,
    io::ErrorKind,
    net::Ipv4Addr,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, RwLock},
    time::Duration,
};

use rmcp::{
    RoleServer,
    service::Peer,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::access::{AgentAccess, AgentMode};
use super::approvals::{ApplySafety, ApprovalRequest, Approvals, RowPreview};
use super::call::CallLog;
use super::config::{AgentConfig, AgentConfigStore};
use super::history::{ApprovalAnswer, ApprovalRecord, History, HistoryEntry, HistoryEvent, Outcome, now_ms};
use super::server::AgentServer;
use super::sessions::{AgentSession, AgentSessions};
use crate::connections::{ConnectionProfile, ConnectionStore, resolve_secrets};
use crate::db::types::StatementOutcome;
use crate::error::{AppError, AppResult, ErrorDetail};
use crate::ssh::known_hosts::KnownHosts;
use crate::vault::Vault;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const IDLE_CLOSE_MS: u64 = 10 * 60 * 1000;
const MAINTENANCE_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum CloseReason {
    Agent,
    User,
    Idle,
    Access,
    ServerOff,
    AppExit,
}

impl CloseReason {
    fn message(self) -> &'static str {
        match self {
            Self::Agent => "Closed by the agent",
            Self::User => "Closed in Fran DB",
            Self::Idle => "Closed after 10 minutes without use",
            Self::Access => "Closed because the connection changed or stopped being shared",
            Self::ServerOff => "Closed because the agent server was turned off",
            Self::AppExit => "Closed because Fran DB quit",
        }
    }
}

/// A session checked against the connection's current access, ready for one call.
pub struct Live {
    pub session: Arc<AgentSession>,
    pub access: AgentAccess,
    pub mode: AgentMode,
    pub config: AgentConfig,
}

pub fn refused(log: &CallLog, message: impl Into<String>) -> AppError {
    log.outcome(Outcome::Refused);
    AppError::general(message)
}

/// How the host reaches the app's window. Tests swap in their own.
pub trait Notifier: Send + Sync {
    fn emit(&self, event: &str, payload: Value);
    fn bring_to_front(&self);
}

impl Notifier for AppHandle {
    fn emit(&self, event: &str, payload: Value) {
        let _ = Emitter::emit(self, event, payload);
    }

    fn bring_to_front(&self) {
        crate::splash::bring_to_front(self);
    }
}

struct RunningServer {
    stop: CancellationToken,
}

pub struct AgentHost {
    app: Arc<dyn Notifier>,
    pub store: Arc<ConnectionStore>,
    pub vault: Arc<Vault>,
    pub known_hosts: Arc<KnownHosts>,
    config: AgentConfigStore,
    pub sessions: AgentSessions,
    pub approvals: Approvals,
    pub history: History,
    server: Mutex<Option<RunningServer>>,
    status: RwLock<ServerStatus>,
    peers: StdMutex<Vec<Peer<RoleServer>>>,
}

impl AgentHost {
    pub fn new(
        app: Arc<dyn Notifier>,
        data_dir: PathBuf,
        store: Arc<ConnectionStore>,
        vault: Arc<Vault>,
        known_hosts: Arc<KnownHosts>,
    ) -> Self {
        Self {
            app,
            store,
            vault,
            known_hosts,
            config: AgentConfigStore::load(data_dir.join("agent.json")),
            sessions: AgentSessions::default(),
            approvals: Approvals::default(),
            history: History::new(data_dir.join("agent_history")),
            server: Mutex::new(None),
            status: RwLock::new(ServerStatus::default()),
            peers: StdMutex::new(Vec::new()),
        }
    }

    pub fn config(&self) -> AppResult<AgentConfig> {
        self.config.read(Clone::clone)
    }

    pub fn status(&self) -> ServerStatus {
        self.status.read().map(|status| status.clone()).unwrap_or_default()
    }

    fn set_status(&self, status: ServerStatus) {
        if let Ok(mut current) = self.status.write() {
            *current = status;
        }
        self.app.emit("agent-status-changed", json!(self.status()));
    }

    pub(super) fn emit_sessions_changed(&self) {
        self.app.emit("agent-sessions-changed", Value::Null);
    }

    fn emit_approvals_changed(&self) {
        self.app.emit("agent-approvals-changed", Value::Null);
    }

    pub async fn set_config(self: &Arc<Self>, next: AgentConfig) -> AppResult<()> {
        next.validate()?;
        let previous = self.config()?;
        let saved = next.clone();
        self.config.update(move |config| {
            *config = saved;
            Ok(())
        })?;

        let restart = previous.enabled != next.enabled
            || previous.port != next.port
            || (next.history_enabled && !previous.history_enabled);
        if restart {
            self.apply().await;
        }
        if previous.allow_raw_sql != next.allow_raw_sql || previous.allow_changes != next.allow_changes {
            self.notify_tools_changed();
        }
        Ok(())
    }

    /// Starts or stops the server to match the saved config.
    pub async fn apply(self: &Arc<Self>) {
        let mut server = self.server.lock().await;
        if let Some(running) = server.take() {
            running.stop.cancel();
        }

        let config = match self.config() {
            Ok(config) => config,
            Err(error) => {
                self.set_status(ServerStatus { error: Some(error.message), ..Default::default() });
                return;
            }
        };
        if !config.enabled {
            drop(server);
            self.close_all(CloseReason::ServerOff).await;
            self.set_status(ServerStatus::default());
            return;
        }
        if config.history_enabled
            && let Err(error) = self.history.unlock(&self.vault)
        {
            self.set_status(ServerStatus {
                error: Some(format!("Agent history can't be kept, so the server didn't start: {}", error.message)),
                ..Default::default()
            });
            return;
        }

        match self.start(config.port).await {
            Ok(running) => {
                *server = Some(running);
                self.set_status(ServerStatus { running: true, port: Some(config.port), error: None });
            }
            Err(error) => self.set_status(ServerStatus { error: Some(error), ..Default::default() }),
        }
    }

    async fn start(self: &Arc<Self>, port: u16) -> Result<RunningServer, String> {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|error| match error.kind() {
                ErrorKind::AddrInUse => format!("Port {port} is already in use. Pick another one."),
                _ => format!("Couldn't listen on port {port}: {error}"),
            })?;

        let stop = CancellationToken::new();
        let config = StreamableHttpServerConfig::default()
            .enforce_origin_validation()
            .with_cancellation_token(stop.child_token());
        let host = self.clone();
        let service = StreamableHttpService::new(
            move || Ok(AgentServer::new(host.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let router = axum::Router::new().route_service("/mcp", service);

        let shutdown = stop.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown.cancelled().await })
                .await;
        });
        Ok(RunningServer { stop })
    }

    pub fn start_maintenance(self: &Arc<Self>) {
        let host = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut pruned_day = 0;
            loop {
                tokio::time::sleep(MAINTENANCE_EVERY).await;
                for id in host.sessions.idle_since(now_ms().saturating_sub(IDLE_CLOSE_MS)) {
                    host.close_session(&id, CloseReason::Idle).await;
                }
                let today = now_ms() / 86_400_000;
                if today != pruned_day
                    && let Ok(config) = host.config()
                {
                    pruned_day = today;
                    let _ = host.history.prune(config.history_days);
                }
            }
        });
    }

    pub fn add_peer(&self, peer: Peer<RoleServer>) {
        if let Ok(mut peers) = self.peers.lock() {
            peers.push(peer);
        }
    }

    fn notify_tools_changed(&self) {
        let peers = match self.peers.lock() {
            Ok(mut peers) => {
                peers.retain(|peer| !peer.is_transport_closed());
                peers.clone()
            }
            Err(_) => return,
        };
        for peer in peers {
            tokio::spawn(async move {
                let _ = peer.notify_tool_list_changed().await;
            });
        }
    }

    pub fn record(&self, entry: HistoryEntry) {
        if !self.config().is_ok_and(|config| config.history_enabled) {
            return;
        }
        if let Err(error) = self.history.append(&entry) {
            eprintln!("agent history: {}", error.message);
            return;
        }
        self.app.emit("agent-history-added", json!(HistoryEntry { reply: None, ..entry }));
    }

    pub async fn close_session(&self, id: &str, reason: CloseReason) -> bool {
        let Some(session) = self.sessions.remove(id) else {
            return false;
        };
        session.close().await;
        if !matches!(reason, CloseReason::Agent) {
            self.record(HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                at: now_ms(),
                event: HistoryEvent::SessionClosed,
                agent: Some(session.agent.clone()),
                session_id: Some(session.id.clone()),
                connection_id: Some(session.connection_id.clone()),
                connection_name: Some(session.connection_name.clone()),
                database: session.catalog.clone(),
                tool: None,
                arguments: None,
                statements: Vec::new(),
                approval: None,
                outcome: Outcome::Ok,
                message: Some(reason.message().to_string()),
                duration_ms: 0.0,
                reply: None,
            });
        }
        self.emit_sessions_changed();
        true
    }

    pub async fn close_all(&self, reason: CloseReason) {
        for id in self.sessions.ids() {
            self.close_session(&id, reason).await;
        }
    }

    pub async fn close_connection_sessions(&self, connection_id: &str) {
        for id in self.sessions.for_connection(connection_id) {
            self.close_session(&id, CloseReason::Access).await;
        }
    }

    pub fn find_profile(&self, connection: &str) -> AppResult<ConnectionProfile> {
        let profiles = self.store.read(|file| file.connections.clone())?;
        if let Some(profile) = profiles.iter().find(|profile| profile.id == connection) {
            return Ok(profile.clone());
        }
        let named: Vec<&ConnectionProfile> = profiles
            .iter()
            .filter(|profile| profile.name.eq_ignore_ascii_case(connection.trim()))
            .collect();
        match named.as_slice() {
            [profile] => Ok((*profile).clone()),
            [] => Err(AppError::not_found(format!(
                "No connection named {connection}. list_connections shows the shared ones."
            ))),
            _ => Err(AppError::general(format!(
                "More than one connection is named {connection}. Pass its id from list_connections."
            ))),
        }
    }

    pub async fn connect(&self, profile: &ConnectionProfile) -> AppResult<Box<dyn crate::db::session::DatabaseSession>> {
        let name = &profile.name;
        let bag = resolve_secrets(
            &self.vault,
            Some(&profile.id),
            HashMap::new(),
            &profile.driver.required_secrets(),
            &profile.driver.optional_secrets(),
        )
        .map_err(|error| match error.detail {
            ErrorDetail::SecretsRequired { .. } => AppError::general(format!(
                "Fran DB has no saved password for {name}. Save it in the connection's settings so agents can connect."
            )),
            _ => error,
        })?;

        profile.driver.connect(&bag, &self.known_hosts).await.map_err(|error| match error.detail {
            ErrorDetail::UnknownHostKey { .. } => AppError::general(format!(
                "Fran DB doesn't know the SSH host key for {name} yet. Connect to it once in Fran DB and trust the key."
            )),
            ErrorDetail::HostKeyChanged { .. } => AppError::general(format!(
                "The SSH host key for {name} changed. Connect to it in Fran DB to check the new key."
            )),
            _ => error,
        })
    }

    /// Looks up a session for a call and checks that its connection is still shared.
    pub async fn live(&self, id: &str, log: &CallLog) -> AppResult<Live> {
        let session = self.sessions.get(id).ok_or_else(|| {
            AppError::not_found(format!(
                "No open session {id}. It may have been closed in Fran DB or after 10 minutes without use. \
                 Open a new one with open_session."
            ))
        })?;
        log.session(&session);

        let config = self.config()?;
        let access = self
            .store
            .find(&session.connection_id)
            .map(|profile| profile.agent_access)
            .unwrap_or_default();
        let mode = access.effective_mode(&config);
        if mode == AgentMode::Off {
            self.close_session(id, CloseReason::Access).await;
            return Err(refused(log, format!("{} isn't shared with agents anymore", session.connection_name)));
        }

        session.touch();
        Ok(Live { session, access, mode, config })
    }

    pub fn require_raw_sql(&self, live: &Live, log: &CallLog) -> AppResult<()> {
        if live.config.allow_raw_sql {
            Ok(())
        } else {
            Err(refused(log, "Raw SQL is turned off for agents in Fran DB. Use the other tools instead."))
        }
    }

    pub fn require_changes(&self, live: &Live, log: &CallLog) -> AppResult<()> {
        match live.mode {
            AgentMode::Ask => Ok(()),
            _ if !live.config.allow_changes => Err(refused(log, "Changes are turned off for all agents in Fran DB")),
            _ => Err(refused(log, format!("{} is read only for agents", live.session.connection_name))),
        }
    }

    /// The database a tool works in, checked against the connection's shared list.
    pub fn database(&self, live: &Live, given: Option<String>, log: &CallLog) -> AppResult<String> {
        let database = given.or_else(|| live.session.default_database.clone()).ok_or_else(|| {
            AppError::general("Pass database. list_databases shows the ones you can use.")
        })?;
        self.check_database(live, &database, log)?;
        Ok(database)
    }

    pub fn optional_database(&self, live: &Live, given: Option<String>, log: &CallLog) -> AppResult<Option<String>> {
        match given.or_else(|| live.session.default_database.clone()) {
            Some(database) => self.check_database(live, &database, log).map(|()| Some(database)),
            None => Ok(None),
        }
    }

    fn check_database(&self, live: &Live, database: &str, log: &CallLog) -> AppResult<()> {
        log.database(Some(database));
        if live.session.catalog.is_none() && !live.access.allows(database) {
            return Err(refused(
                log,
                format!("{database} isn't shared with agents. Shared: {}", live.access.databases.join(", ")),
            ));
        }
        Ok(())
    }

    /// Runs reads in the session's read-only context, cancelled by the agent, a closed session or the timeout.
    pub async fn run_read(
        &self,
        live: &Live,
        database: Option<&str>,
        sql: &str,
        row_limit: usize,
        log: &CallLog,
    ) -> AppResult<Vec<StatementOutcome>> {
        let session = &live.session;
        let context = session.read_context(database).await?;
        let run = session.session.execute(context, sql, row_limit);
        tokio::pin!(run);

        let (outcome, message) = tokio::select! {
            result = &mut run => return result,
            () = log.cancel.cancelled() => (Outcome::Cancelled, "The query was cancelled"),
            () = session.closed.cancelled() => (Outcome::Cancelled, "The session was closed in Fran DB"),
            () = tokio::time::sleep(CALL_TIMEOUT) => (Outcome::TimedOut, "The query ran longer than 30 seconds and was stopped"),
        };
        let _ = session.session.cancel(context).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), run).await;
        log.outcome(outcome);
        Err(AppError::general(message))
    }

    /// Asks the user in Fran DB, then runs the statements with `execute_batch`.
    pub async fn run_changes(
        &self,
        live: &Live,
        database: Option<&str>,
        tool: &str,
        statements: Vec<String>,
        preview: Option<RowPreview>,
        log: &CallLog,
    ) -> AppResult<Value> {
        let row_changes = preview.is_some();
        if statements.is_empty() {
            return Err(AppError::general("There's nothing to change"));
        }
        log.statements(&statements);

        let server = &live.session.server;
        let transactional =
            row_changes || server.transactional_ddl || !statements.iter().any(|statement| is_schema_change(statement));
        let safety = if transactional {
            ApplySafety::Transaction
        } else {
            ApplySafety::Schema { atomic: server.atomic_ddl }
        };

        let session = &live.session;
        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session.id.clone(),
            agent: session.agent.clone(),
            connection_id: session.connection_id.clone(),
            connection_name: session.connection_name.clone(),
            kind: session.kind,
            database: database.map(str::to_string).or_else(|| session.catalog.clone()),
            tool: tool.to_string(),
            arguments: log.arguments.clone(),
            statements: statements.clone(),
            safety,
            preview,
            requested_at: now_ms(),
        };
        self.ask(live, request, log).await?;

        let outcome = live.session.session.execute_batch(database, &statements, transactional).await?;
        match outcome.error {
            None => Ok(json!({ "executed": outcome.executed })),
            Some(error) => {
                let kept = if outcome.rolled_back || outcome.executed == 0 {
                    "Nothing was kept."
                } else {
                    "The statements before it stayed applied."
                };
                let at = outcome
                    .failed_index
                    .map(|index| format!("Statement {} failed: ", index + 1))
                    .unwrap_or_default();
                Err(AppError::general(format!("{at}{} {kept}", error.message)))
            }
        }
    }

    async fn ask(&self, live: &Live, request: ApprovalRequest, log: &CallLog) -> AppResult<()> {
        let session = &live.session;
        let receiver = self.approvals.open(request.clone());
        self.emit_approvals_changed();
        self.app.bring_to_front();

        let answer = tokio::select! {
            answer = receiver => match answer {
                Ok(true) => ApprovalAnswer::Approved,
                Ok(false) => ApprovalAnswer::Rejected,
                Err(_) => ApprovalAnswer::Cancelled,
            },
            () = tokio::time::sleep(APPROVAL_TIMEOUT) => ApprovalAnswer::TimedOut,
            () = log.cancel.cancelled() => ApprovalAnswer::Cancelled,
            () = session.closed.cancelled() => ApprovalAnswer::Cancelled,
        };
        if self.approvals.withdraw(&request.id) || answer != ApprovalAnswer::Approved {
            self.emit_approvals_changed();
        }
        log.approval(ApprovalRecord {
            answer,
            requested_at: request.requested_at,
            answered_at: now_ms(),
        });

        match answer {
            ApprovalAnswer::Approved => Ok(()),
            ApprovalAnswer::Rejected => {
                log.outcome(Outcome::Refused);
                Err(AppError::general("The user rejected this change in Fran DB, so nothing ran"))
            }
            ApprovalAnswer::TimedOut => {
                log.outcome(Outcome::TimedOut);
                Err(AppError::general("Nobody answered in Fran DB within 5 minutes, so nothing ran"))
            }
            ApprovalAnswer::Cancelled => {
                log.outcome(Outcome::Cancelled);
                Err(AppError::general("The change was cancelled before anyone answered, so nothing ran"))
            }
        }
    }

    pub fn answer(&self, id: &str, approved: bool) -> bool {
        let answered = self.approvals.answer(id, approved);
        self.emit_approvals_changed();
        answered
    }
}

fn is_schema_change(statement: &str) -> bool {
    let first = statement
        .trim_start_matches(|character: char| character.is_whitespace() || character == '(')
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "COMMENT" | "GRANT" | "REVOKE"
    )
}
