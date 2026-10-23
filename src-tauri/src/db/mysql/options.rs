use serde::{Deserialize, Serialize};

use crate::connections::SecretSlot;
use crate::ssh::SshOptions;

pub const PASSWORD_SLOT: &str = "password";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MysqlConnectMethod {
    #[default]
    Tcp,
    /// Unix socket, or a named pipe on Windows.
    Socket,
    SshTunnel,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SslMode {
    Disabled,
    #[default]
    Preferred,
    Required,
    VerifyCa,
    VerifyIdentity,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MysqlSslOptions {
    pub mode: SslMode,
    pub ca_file: String,
    pub cert_file: String,
    pub key_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MysqlOptions {
    pub method: MysqlConnectMethod,
    /// For the SSH method this is the MySQL host as seen from the SSH server.
    pub host: String,
    pub port: u16,
    pub socket_path: String,
    pub user: String,
    pub default_schema: String,
    pub ssh: SshOptions,
    pub ssl: MysqlSslOptions,
    pub compression: bool,
    pub cleartext_auth: bool,
    pub connect_timeout_secs: u32,
    pub init_sql: String,
}

impl Default for MysqlOptions {
    fn default() -> Self {
        Self {
            method: MysqlConnectMethod::Tcp,
            host: "127.0.0.1".to_string(),
            port: 3306,
            socket_path: String::new(),
            user: "root".to_string(),
            default_schema: String::new(),
            ssh: SshOptions::default(),
            ssl: MysqlSslOptions::default(),
            compression: false,
            cleartext_auth: false,
            connect_timeout_secs: 10,
            init_sql: String::new(),
        }
    }
}

impl MysqlOptions {
    pub fn password_slot() -> SecretSlot {
        SecretSlot::new(PASSWORD_SLOT, "Password")
    }

    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        let mut slots = vec![Self::password_slot()];
        if self.method == MysqlConnectMethod::SshTunnel {
            slots.extend(self.ssh.required_secrets());
        }
        slots
    }

    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        match self.method {
            MysqlConnectMethod::SshTunnel => self.ssh.optional_secrets(),
            _ => Vec::new(),
        }
    }
}
