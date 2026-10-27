use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::json_store::JsonStore;

pub const DEFAULT_PORT: u16 = 3726;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentConfig {
    pub enabled: bool,
    pub port: u16,
    pub allow_raw_sql: bool,
    pub allow_changes: bool,
    pub history_enabled: bool,
    pub history_days: u32,
    pub query_timeout_secs: u32,
}

pub const MAX_QUERY_TIMEOUT_SECS: u32 = 3600;

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            allow_raw_sql: true,
            allow_changes: true,
            history_enabled: true,
            history_days: 30,
            query_timeout_secs: 60,
        }
    }
}

impl AgentConfig {
    pub fn validate(&self) -> AppResult<()> {
        if self.port == 0 {
            return Err(AppError::general("Choose a port"));
        }
        if !(1..=MAX_QUERY_TIMEOUT_SECS).contains(&self.query_timeout_secs) {
            return Err(AppError::general(format!(
                "The query timeout has to be between 1 and {MAX_QUERY_TIMEOUT_SECS} seconds"
            )));
        }
        if self.history_days == 0 {
            return Err(AppError::general("Keep history for at least one day"));
        }
        Ok(())
    }
}

pub type AgentConfigStore = JsonStore<AgentConfig>;
