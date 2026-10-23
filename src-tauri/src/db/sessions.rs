use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use serde::Serialize;

use super::{DatabaseKind, session::DatabaseSession, types::ServerInfo};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub connection_id: String,
    pub kind: DatabaseKind,
    pub server: ServerInfo,
    pub catalog: Option<String>,
}

pub struct OpenSession {
    pub info: SessionInfo,
    pub session: Box<dyn DatabaseSession>,
}

#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<OpenSession>>>,
}

impl SessionManager {
    pub fn insert(&self, session: OpenSession) -> AppResult<SessionInfo> {
        let info = session.info.clone();
        self.sessions
            .write()
            .map_err(|_| AppError::general("Session lock poisoned"))?
            .insert(info.id.clone(), Arc::new(session));
        Ok(info)
    }

    pub fn get(&self, id: &str) -> AppResult<Arc<OpenSession>> {
        self.sessions
            .read()
            .map_err(|_| AppError::general("Session lock poisoned"))?
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::not_found("This connection is closed"))
    }

    pub fn remove(&self, id: &str) -> Option<Arc<OpenSession>> {
        self.sessions.write().ok()?.remove(id)
    }

    pub fn drain(&self) -> Vec<Arc<OpenSession>> {
        match self.sessions.write() {
            Ok(mut sessions) => sessions.drain().map(|(_, session)| session).collect(),
            Err(_) => Vec::new(),
        }
    }
}
