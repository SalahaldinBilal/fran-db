use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use super::HostKeyInfo;
use crate::error::AppResult;
use crate::json_store::JsonStore;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KnownHostsFile {
    pub hosts: Vec<HostKeyInfo>,
}

pub type KnownHosts = JsonStore<KnownHostsFile>;

impl KnownHosts {
    pub fn fingerprint_for(&self, host: &str, port: u16) -> AppResult<Option<String>> {
        self.read(|file| {
            file.hosts
                .iter()
                .find(|known| known.host.eq_ignore_ascii_case(host) && known.port == port)
                .map(|known| known.fingerprint.clone())
        })
    }

    pub fn trust(&self, key: HostKeyInfo) -> AppResult<()> {
        self.update(move |file| {
            file.hosts
                .retain(|known| !(known.host.eq_ignore_ascii_case(&key.host) && known.port == key.port));
            file.hosts.push(key);
            Ok(())
        })
    }
}

#[tauri::command]
pub async fn trust_host_key(
    known_hosts: State<'_, Arc<KnownHosts>>,
    key: HostKeyInfo,
) -> AppResult<()> {
    known_hosts.trust(key)
}
