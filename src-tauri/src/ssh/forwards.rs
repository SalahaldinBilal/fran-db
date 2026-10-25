use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::State;

use super::{SshTunnel, known_hosts::KnownHosts};
use crate::connections::{ConnectionStore, resolve_secrets};
use crate::error::{AppError, AppResult};
use crate::vault::Vault;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForwardInfo {
    pub connection_id: String,
    pub local_port: u16,
    pub host: String,
    pub port: u16,
}

#[derive(Default)]
pub struct PortForwards {
    active: Mutex<HashMap<String, (PortForwardInfo, SshTunnel)>>,
}

impl PortForwards {
    fn list(&self) -> Vec<PortForwardInfo> {
        match self.active.lock() {
            Ok(active) => active.values().map(|(info, _)| info.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[tauri::command]
pub async fn start_port_forward(
    store: State<'_, Arc<ConnectionStore>>,
    vault: State<'_, Arc<Vault>>,
    known_hosts: State<'_, Arc<KnownHosts>>,
    forwards: State<'_, PortForwards>,
    connection_id: String,
    local_port: u16,
    secrets: HashMap<String, String>,
) -> AppResult<PortForwardInfo> {
    if local_port == 0 {
        return Err(AppError::general("Choose a local port"));
    }
    let profile = store.find(&connection_id)?;
    let (ssh, host, port) = profile.driver.ssh_target().ok_or_else(|| {
        AppError::general("Port forwarding needs a connection that uses an SSH tunnel")
    })?;
    let bag = resolve_secrets(
        &vault,
        Some(&profile.id),
        secrets,
        &ssh.required_secrets(),
        &ssh.optional_secrets(),
    )?;

    let tunnel = SshTunnel::open(
        ssh,
        &bag,
        &known_hosts,
        host,
        port,
        local_port,
        CONNECT_TIMEOUT,
    )
    .await?;
    let info = PortForwardInfo {
        connection_id: profile.id.clone(),
        local_port,
        host: host.to_string(),
        port,
    };
    forwards
        .active
        .lock()
        .map_err(|_| AppError::general("Forward lock poisoned"))?
        .insert(profile.id, (info.clone(), tunnel));
    Ok(info)
}

#[tauri::command]
pub fn stop_port_forward(
    forwards: State<'_, PortForwards>,
    connection_id: String,
) -> AppResult<()> {
    forwards
        .active
        .lock()
        .map_err(|_| AppError::general("Forward lock poisoned"))?
        .remove(&connection_id);
    Ok(())
}

#[tauri::command]
pub fn list_port_forwards(forwards: State<'_, PortForwards>) -> Vec<PortForwardInfo> {
    forwards.list()
}
