use serde::{Deserialize, Serialize};

use super::config::AgentConfig;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentMode {
    #[default]
    Off,
    ReadOnly,
    /// Changes are allowed, each one after the user approves it.
    Ask,
}

/// What agents may do with one connection. Stored on the connection profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentAccess {
    pub mode: AgentMode,
    /// All when empty. Where a session is bound to one database (Postgres, D1) these are those databases.
    pub databases: Vec<String>,
}

impl AgentAccess {
    pub fn allows(&self, database: &str) -> bool {
        self.databases.is_empty() || self.databases.iter().any(|allowed| allowed == database)
    }

    /// The connection's mode, made read only when changes are switched off for every agent.
    pub fn effective_mode(&self, config: &AgentConfig) -> AgentMode {
        match self.mode {
            AgentMode::Ask if !config.allow_changes => AgentMode::ReadOnly,
            mode => mode,
        }
    }
}
