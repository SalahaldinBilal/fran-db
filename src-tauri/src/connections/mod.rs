pub mod commands;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::db::DriverOptions;
use crate::error::{AppError, AppResult};
use crate::json_store::JsonStore;
use crate::vault::Vault;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub color_fill: bool,
    #[serde(flatten)]
    pub driver: DriverOptions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectionsFile {
    pub connections: Vec<ConnectionProfile>,
}

pub type ConnectionStore = JsonStore<ConnectionsFile>;

impl ConnectionStore {
    pub fn find(&self, id: &str) -> AppResult<ConnectionProfile> {
        self.read(|file| file.connections.iter().find(|profile| profile.id == id).cloned())?
            .ok_or_else(|| AppError::not_found("Connection not found"))
    }
}

/// A credential a driver needs, like a password. `id` is stable per driver, `label` is shown in prompts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecretSlot {
    pub id: String,
    pub label: String,
}

impl SecretSlot {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
        }
    }
}

pub fn secret_prefix(connection_id: &str) -> String {
    format!("connection:{connection_id}:")
}

pub fn secret_id(connection_id: &str, slot: &str) -> String {
    format!("{}{slot}", secret_prefix(connection_id))
}

pub struct SecretBag {
    values: HashMap<String, Zeroizing<String>>,
}

impl SecretBag {
    pub fn new(values: HashMap<String, String>) -> Self {
        Self {
            values: values.into_iter().map(|(slot, value)| (slot, Zeroizing::new(value))).collect(),
        }
    }

    pub fn get(&self, slot: &str) -> Option<&str> {
        self.values.get(slot).map(|value| value.as_str())
    }
}

/// Typed-in values win over stored ones. A vault that can't be opened counts as
/// nothing stored, so the user can still connect by typing the password.
pub fn resolve_secrets(
    vault: &Vault,
    connection_id: Option<&str>,
    provided: HashMap<String, String>,
    required: &[SecretSlot],
    optional: &[SecretSlot],
) -> AppResult<SecretBag> {
    let mut provided: HashMap<String, Zeroizing<String>> = provided
        .into_iter()
        .map(|(slot, value)| (slot, Zeroizing::new(value)))
        .collect();

    let mut values = HashMap::new();
    let mut missing = Vec::new();

    for (slot, is_required) in required
        .iter()
        .map(|slot| (slot, true))
        .chain(optional.iter().map(|slot| (slot, false)))
    {
        let value = provided.remove(&slot.id).or_else(|| {
            connection_id.and_then(|id| vault.get(&secret_id(id, &slot.id)).ok().flatten())
        });

        match value {
            Some(value) => {
                values.insert(slot.id.clone(), value);
            }
            None if is_required => missing.push(slot.clone()),
            None => {}
        }
    }

    if !missing.is_empty() {
        return Err(AppError::secrets_required(missing));
    }

    Ok(SecretBag { values })
}
