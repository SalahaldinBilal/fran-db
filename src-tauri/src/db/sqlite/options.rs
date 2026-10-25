use serde::{Deserialize, Serialize};

use crate::connections::SecretSlot;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqliteOptions {
    pub path: String,
    pub read_only: bool,
    /// Off by default so a mistyped path fails instead of leaving an empty database behind.
    pub create_if_missing: bool,
    pub foreign_keys: bool,
    pub busy_timeout_secs: u32,
    pub init_sql: String,
}

impl Default for SqliteOptions {
    fn default() -> Self {
        Self {
            path: String::new(),
            read_only: false,
            create_if_missing: false,
            foreign_keys: true,
            busy_timeout_secs: 5,
            init_sql: String::new(),
        }
    }
}

impl SqliteOptions {
    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        Vec::new()
    }

    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        Vec::new()
    }
}
