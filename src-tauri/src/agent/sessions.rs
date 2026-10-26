use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::history::now_ms;
use crate::db::DatabaseKind;
use crate::db::session::{ContextAccess, ContextId, DatabaseSession};
use crate::db::types::ServerInfo;
use crate::error::AppResult;

/// One database connection opened for one agent with `open_session`.
pub struct AgentSession {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub kind: DatabaseKind,
    pub server: ServerInfo,
    pub catalog: Option<String>,
    /// Used by tools when the agent leaves `database` out.
    pub default_database: Option<String>,
    pub agent: String,
    pub opened_at: u64,
    last_used: AtomicU64,
    pub session: Box<dyn DatabaseSession>,
    contexts: Mutex<HashMap<Option<String>, ContextId>>,
    /// Cancelled when the session closes, which stops its running calls.
    pub closed: CancellationToken,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionInfo {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub kind: DatabaseKind,
    pub catalog: Option<String>,
    pub default_database: Option<String>,
    pub agent: String,
    pub opened_at: u64,
    pub last_used: u64,
}

impl AgentSession {
    pub fn new(
        connection_id: String,
        connection_name: String,
        kind: DatabaseKind,
        default_database: Option<String>,
        agent: String,
        session: Box<dyn DatabaseSession>,
    ) -> Self {
        let opened_at = now_ms();
        Self {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            connection_id,
            connection_name,
            kind,
            server: session.server_info(),
            catalog: session.catalog(),
            default_database,
            agent,
            opened_at,
            last_used: AtomicU64::new(opened_at),
            session,
            contexts: Mutex::new(HashMap::new()),
            closed: CancellationToken::new(),
        }
    }

    pub fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::Relaxed);
    }

    pub fn last_used(&self) -> u64 {
        self.last_used.load(Ordering::Relaxed)
    }

    pub fn info(&self) -> AgentSessionInfo {
        AgentSessionInfo {
            id: self.id.clone(),
            connection_id: self.connection_id.clone(),
            connection_name: self.connection_name.clone(),
            kind: self.kind,
            catalog: self.catalog.clone(),
            default_database: self.default_database.clone(),
            agent: self.agent.clone(),
            opened_at: self.opened_at,
            last_used: self.last_used(),
        }
    }

    /// The read-only context for `database`, opened on first use.
    pub async fn read_context(&self, database: Option<&str>) -> AppResult<ContextId> {
        let mut contexts = self.contexts.lock().await;
        let key = database.map(str::to_string);
        if let Some(context) = contexts.get(&key) {
            return Ok(*context);
        }
        let context = self.session.open_context(key.clone(), ContextAccess::ReadOnly).await?;
        contexts.insert(key, context);
        Ok(context)
    }

    pub async fn close(&self) {
        self.closed.cancel();
        let contexts: Vec<ContextId> = self.contexts.lock().await.drain().map(|(_, context)| context).collect();
        for context in contexts {
            self.session.close_context(context).await;
        }
        self.session.close().await;
    }
}

#[derive(Default)]
pub struct AgentSessions {
    sessions: RwLock<HashMap<String, Arc<AgentSession>>>,
}

impl AgentSessions {
    pub fn insert(&self, session: AgentSession) -> Arc<AgentSession> {
        let session = Arc::new(session);
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.insert(session.id.clone(), session.clone());
        }
        session
    }

    pub fn get(&self, id: &str) -> Option<Arc<AgentSession>> {
        self.sessions.read().ok()?.get(id).cloned()
    }

    pub fn remove(&self, id: &str) -> Option<Arc<AgentSession>> {
        self.sessions.write().ok()?.remove(id)
    }

    fn ids_where(&self, keep: impl Fn(&AgentSession) -> bool) -> Vec<String> {
        match self.sessions.read() {
            Ok(sessions) => sessions.values().filter(|session| keep(session)).map(|session| session.id.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn ids(&self) -> Vec<String> {
        self.ids_where(|_| true)
    }

    pub fn idle_since(&self, before: u64) -> Vec<String> {
        self.ids_where(|session| session.last_used() < before)
    }

    pub fn for_connection(&self, connection_id: &str) -> Vec<String> {
        self.ids_where(|session| session.connection_id == connection_id)
    }

    pub fn list(&self) -> Vec<AgentSessionInfo> {
        let mut list: Vec<AgentSessionInfo> = match self.sessions.read() {
            Ok(sessions) => sessions.values().map(|session| session.info()).collect(),
            Err(_) => Vec::new(),
        };
        list.sort_by_key(|session| std::cmp::Reverse(session.opened_at));
        list
    }
}
