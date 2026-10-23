pub mod known_hosts;

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key};
use serde::{Deserialize, Serialize};

use crate::connections::{SecretBag, SecretSlot};
use crate::error::{AppError, AppResult, ErrorDetail};
use crate::forward::LocalForward;
use known_hosts::KnownHosts;

pub const SSH_PASSWORD_SLOT: &str = "sshPassword";
pub const SSH_PASSPHRASE_SLOT: &str = "sshKeyPassphrase";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthMethod {
    #[default]
    Password,
    PublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SshOptions {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshAuthMethod,
    pub key_file: String,
}

impl Default for SshOptions {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            user: String::new(),
            auth: SshAuthMethod::Password,
            key_file: String::new(),
        }
    }
}

impl SshOptions {
    pub fn password_slot() -> SecretSlot {
        SecretSlot::new(SSH_PASSWORD_SLOT, "SSH password")
    }

    pub fn passphrase_slot() -> SecretSlot {
        SecretSlot::new(SSH_PASSPHRASE_SLOT, "SSH key passphrase")
    }

    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        match self.auth {
            SshAuthMethod::Password => vec![Self::password_slot()],
            SshAuthMethod::PublicKey => Vec::new(),
        }
    }

    /// The passphrase is only asked for once the key turns out to be encrypted.
    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        match self.auth {
            SshAuthMethod::Password => Vec::new(),
            SshAuthMethod::PublicKey => vec![Self::passphrase_slot()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
}

struct HostKeyCheck {
    host: String,
    port: u16,
    known_fingerprint: Option<String>,
    rejection: Arc<Mutex<Option<AppError>>>,
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key = server_public_key.public_key();
        let key = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            algorithm: public_key.algorithm().to_string(),
            fingerprint: public_key.fingerprint(HashAlg::Sha256).to_string(),
        };

        let rejection = match &self.known_fingerprint {
            Some(known) if *known == key.fingerprint => return Ok(true),
            Some(known) => AppError::new(
                format!(
                    "The SSH host key for {}:{} changed. This can mean someone is intercepting the connection.",
                    key.host, key.port
                ),
                ErrorDetail::HostKeyChanged {
                    known_fingerprint: known.clone(),
                    key,
                },
            ),
            None => AppError::new(
                format!("The SSH host {}:{} is not trusted yet", key.host, key.port),
                ErrorDetail::UnknownHostKey { key },
            ),
        };

        if let Ok(mut slot) = self.rejection.lock() {
            *slot = Some(rejection);
        }
        Ok(false)
    }
}

/// A local port forwarded through SSH to `target_host:target_port`. Dropping it stops the forward.
pub struct SshTunnel {
    forward: LocalForward,
    handle: Arc<client::Handle<HostKeyCheck>>,
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let handle = self.handle.clone();
        tauri::async_runtime::spawn(async move {
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await;
        });
    }
}

impl SshTunnel {
    pub fn local_port(&self) -> u16 {
        self.forward.port
    }

    pub async fn open(
        options: &SshOptions,
        secrets: &SecretBag,
        known_hosts: &KnownHosts,
        target_host: &str,
        target_port: u16,
        timeout: Duration,
    ) -> AppResult<Self> {
        if options.host.trim().is_empty() {
            return Err(AppError::ssh("SSH host is empty"));
        }

        let private_key = match options.auth {
            SshAuthMethod::PublicKey => Some(load_key(options, secrets)?),
            SshAuthMethod::Password => None,
        };

        let rejection = Arc::new(Mutex::new(None));
        let handler = HostKeyCheck {
            host: options.host.clone(),
            port: options.port,
            known_fingerprint: known_hosts.fingerprint_for(&options.host, options.port)?,
            rejection: rejection.clone(),
        };

        let config = Arc::new(client::Config {
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(30)),
            nodelay: true,
            ..Default::default()
        });

        let connect = client::connect(config, (options.host.as_str(), options.port), handler);
        let mut handle = match tokio::time::timeout(timeout, connect).await {
            Err(_) => return Err(AppError::ssh("Timed out connecting to the SSH server")),
            Ok(Err(error)) => {
                let rejected = rejection.lock().ok().and_then(|mut slot| slot.take());
                return Err(rejected.unwrap_or_else(|| AppError::ssh(format!("SSH connection failed: {error}"))));
            }
            Ok(Ok(handle)) => handle,
        };

        let auth = match private_key {
            Some(key) => {
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|error| AppError::ssh(error.to_string()))?
                    .flatten();
                handle
                    .authenticate_publickey(&options.user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                    .await
            }
            None => {
                let password = secrets
                    .get(SSH_PASSWORD_SLOT)
                    .ok_or_else(|| AppError::secrets_required(vec![SshOptions::password_slot()]))?;
                handle.authenticate_password(&options.user, password).await
            }
        }
        .map_err(|error| AppError::ssh(format!("SSH authentication failed: {error}")))?;

        if !auth.success() {
            return Err(AppError::ssh("SSH authentication was rejected"));
        }

        let handle = Arc::new(handle);
        let forward_handle = handle.clone();
        let target_host = target_host.to_string();

        let forward = LocalForward::start(move |peer| {
            let handle = forward_handle.clone();
            let target_host = target_host.clone();
            async move {
                let channel = handle
                    .channel_open_direct_tcpip(
                        target_host,
                        u32::from(target_port),
                        peer.ip().to_string(),
                        u32::from(peer.port()),
                    )
                    .await
                    .map_err(std::io::Error::other)?;
                Ok(channel.into_stream())
            }
        })
        .await?;

        Ok(Self { forward, handle })
    }
}

fn load_key(options: &SshOptions, secrets: &SecretBag) -> AppResult<russh::keys::PrivateKey> {
    let path = Path::new(&options.key_file);
    if options.key_file.trim().is_empty() || !path.is_file() {
        return Err(AppError::ssh(format!("SSH key file not found: {}", options.key_file)));
    }

    match secrets.get(SSH_PASSPHRASE_SLOT) {
        Some(passphrase) => load_secret_key(path, Some(passphrase))
            .map_err(|error| AppError::ssh(format!("SSH key could not be opened: {error}"))),
        None => load_secret_key(path, None)
            .map_err(|_| AppError::secrets_required(vec![SshOptions::passphrase_slot()])),
    }
}
