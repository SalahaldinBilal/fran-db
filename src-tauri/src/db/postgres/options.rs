use serde::{Deserialize, Serialize};

use crate::connections::SecretSlot;
use crate::ssh::SshOptions;

pub const PASSWORD_SLOT: &str = "password";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PostgresConnectMethod {
    #[default]
    Tcp,
    /// Unix socket. Postgres has no named pipes on Windows.
    Socket,
    SshTunnel,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PostgresSslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostgresSslOptions {
    pub mode: PostgresSslMode,
    pub ca_file: String,
    pub cert_file: String,
    pub key_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostgresOptions {
    pub method: PostgresConnectMethod,
    /// For the SSH method this is the Postgres host as seen from the SSH server.
    pub host: String,
    /// Also names the socket file for the socket method.
    pub port: u16,
    /// A socket directory such as `/var/run/postgresql`, or the socket file itself.
    pub socket_path: String,
    pub user: String,
    pub database: String,
    pub default_schema: String,
    pub ssh: SshOptions,
    pub ssl: PostgresSslOptions,
    pub connect_timeout_secs: u32,
    pub init_sql: String,
}

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            method: PostgresConnectMethod::Tcp,
            host: "127.0.0.1".to_string(),
            port: 5432,
            socket_path: String::new(),
            user: "postgres".to_string(),
            database: "postgres".to_string(),
            default_schema: "public".to_string(),
            ssh: SshOptions::default(),
            ssl: PostgresSslOptions::default(),
            connect_timeout_secs: 10,
            init_sql: String::new(),
        }
    }
}

impl PostgresOptions {
    pub fn password_slot() -> SecretSlot {
        SecretSlot::new(PASSWORD_SLOT, "Password")
    }

    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        let mut slots = vec![Self::password_slot()];
        if self.method == PostgresConnectMethod::SshTunnel {
            slots.extend(self.ssh.required_secrets());
        }
        slots
    }

    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        match self.method {
            PostgresConnectMethod::SshTunnel => self.ssh.optional_secrets(),
            _ => Vec::new(),
        }
    }

    pub fn ssh_target(&self) -> Option<(&SshOptions, &str, u16)> {
        match self.method {
            PostgresConnectMethod::SshTunnel => Some((&self.ssh, self.host.trim(), self.port)),
            _ => None,
        }
    }
}
