use std::fmt::Display;

use serde::Serialize;

use crate::connections::SecretSlot;
use crate::ssh::HostKeyInfo;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub message: String,
    pub detail: ErrorDetail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ErrorDetail {
    General,
    NotFound,
    Vault,
    Ssh,
    #[serde(rename_all = "camelCase")]
    Database {
        code: Option<u16>,
        sql_state: Option<String>,
    },
    SecretsRequired {
        slots: Vec<SecretSlot>,
    },
    UnknownHostKey {
        key: HostKeyInfo,
    },
    #[serde(rename_all = "camelCase")]
    HostKeyChanged {
        key: HostKeyInfo,
        known_fingerprint: String,
    },
}

impl AppError {
    pub fn new(message: impl Into<String>, detail: ErrorDetail) -> Self {
        Self {
            message: message.into(),
            detail,
        }
    }

    pub fn general(message: impl Into<String>) -> Self {
        Self::new(message, ErrorDetail::General)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(message, ErrorDetail::NotFound)
    }

    pub fn vault(message: impl Into<String>) -> Self {
        Self::new(message, ErrorDetail::Vault)
    }

    pub fn ssh(message: impl Into<String>) -> Self {
        Self::new(message, ErrorDetail::Ssh)
    }

    pub fn secrets_required(slots: Vec<SecretSlot>) -> Self {
        let names = slots
            .iter()
            .map(|slot| slot.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Self::new(
            format!("Missing credentials: {names}"),
            ErrorDetail::SecretsRequired { slots },
        )
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::general(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::general(error.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
