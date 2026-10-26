use std::sync::Mutex;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::history::{ApprovalRecord, Outcome};
use super::sessions::AgentSession;

#[derive(Debug, Default)]
pub struct CallState {
    pub session_id: Option<String>,
    pub connection_id: Option<String>,
    pub connection_name: Option<String>,
    pub database: Option<String>,
    pub statements: Vec<String>,
    pub approval: Option<ApprovalRecord>,
    pub outcome: Option<Outcome>,
}

/// One tool call: who made it, how to cancel it, and what it touched, written to history when it returns.
#[derive(Debug)]
pub struct CallLog {
    pub agent: String,
    pub arguments: Option<Value>,
    pub cancel: CancellationToken,
    state: Mutex<CallState>,
}

impl CallLog {
    pub fn new(agent: String, arguments: Option<Value>, cancel: CancellationToken) -> Self {
        Self {
            agent,
            arguments,
            cancel,
            state: Mutex::default(),
        }
    }

    fn update(&self, change: impl FnOnce(&mut CallState)) {
        if let Ok(mut state) = self.state.lock() {
            change(&mut state);
        }
    }

    pub fn session(&self, session: &AgentSession) {
        self.update(|state| {
            state.session_id = Some(session.id.clone());
            state.connection_id = Some(session.connection_id.clone());
            state.connection_name = Some(session.connection_name.clone());
        });
    }

    pub fn database(&self, database: Option<&str>) {
        self.update(|state| state.database = database.map(str::to_string));
    }

    pub fn statements(&self, statements: &[String]) {
        self.update(|state| state.statements.extend_from_slice(statements));
    }

    pub fn approval(&self, record: ApprovalRecord) {
        self.update(|state| state.approval = Some(record));
    }

    pub fn outcome(&self, outcome: Outcome) {
        self.update(|state| state.outcome = Some(outcome));
    }

    pub fn take(&self) -> CallState {
        self.state.lock().map(|mut state| std::mem::take(&mut *state)).unwrap_or_default()
    }
}
