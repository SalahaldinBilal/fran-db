pub mod commands;
pub mod mysql;
pub mod postgres;
pub mod session;
pub mod sessions;
pub mod sqlite;
pub mod types;

use serde::{Deserialize, Serialize};

use crate::connections::{SecretBag, SecretSlot};
use crate::error::AppResult;
use crate::ssh::{SshOptions, known_hosts::KnownHosts};
use session::DatabaseSession;
use types::StatementRange;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DatabaseKind {
    Mysql,
    Postgres,
    Sqlite,
    D1,
}

/// Per-kind connection settings. Adding a database means adding a variant here
/// and an arm in each `match` below, nothing outside this file switches on kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "options", rename_all = "camelCase")]
pub enum DriverOptions {
    Mysql(mysql::MysqlOptions),
    Postgres(postgres::PostgresOptions),
    Sqlite(sqlite::SqliteOptions),
    D1(sqlite::d1::D1Options),
}

impl DriverOptions {
    pub fn kind(&self) -> DatabaseKind {
        match self {
            Self::Mysql(_) => DatabaseKind::Mysql,
            Self::Postgres(_) => DatabaseKind::Postgres,
            Self::Sqlite(_) => DatabaseKind::Sqlite,
            Self::D1(_) => DatabaseKind::D1,
        }
    }

    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        match self {
            Self::Mysql(options) => options.required_secrets(),
            Self::Postgres(options) => options.required_secrets(),
            Self::Sqlite(options) => options.required_secrets(),
            Self::D1(options) => options.required_secrets(),
        }
    }

    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        match self {
            Self::Mysql(options) => options.optional_secrets(),
            Self::Postgres(options) => options.optional_secrets(),
            Self::Sqlite(options) => options.optional_secrets(),
            Self::D1(options) => options.optional_secrets(),
        }
    }

    pub fn ssh_target(&self) -> Option<(&SshOptions, &str, u16)> {
        match self {
            Self::Mysql(options) => options.ssh_target(),
            Self::Postgres(options) => options.ssh_target(),
            Self::Sqlite(_) | Self::D1(_) => None,
        }
    }

    pub async fn connect(
        &self,
        secrets: &SecretBag,
        known_hosts: &KnownHosts,
    ) -> AppResult<Box<dyn DatabaseSession>> {
        match self {
            Self::Mysql(options) => Ok(Box::new(
                mysql::MysqlSession::connect(options, secrets, known_hosts).await?,
            )),
            Self::Postgres(options) => Ok(Box::new(
                postgres::PostgresSession::connect(options, secrets, known_hosts).await?,
            )),
            Self::Sqlite(options) => Ok(Box::new(sqlite::SqliteSession::connect(options).await?)),
            Self::D1(options) => Ok(Box::new(sqlite::d1::connect(options, secrets).await?)),
        }
    }
}

pub fn split_statements(kind: DatabaseKind, sql: &str) -> Vec<StatementRange> {
    match kind {
        DatabaseKind::Mysql => mysql::splitter::split(sql),
        DatabaseKind::Postgres => postgres::splitter::split(sql),
        DatabaseKind::Sqlite | DatabaseKind::D1 => sqlite::splitter::split(sql),
    }
}
