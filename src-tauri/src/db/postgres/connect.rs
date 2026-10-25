use std::{path::PathBuf, time::Duration};

use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use zeroize::Zeroizing;

use super::client::{
    BoxSocket, CancelKey, Negotiated, PgConnection, PgError, StartupParams, TlsSettings, Verification, negotiate,
    send_cancel,
};
use super::options::{PASSWORD_SLOT, PostgresConnectMethod, PostgresOptions, PostgresSslMode};
use super::splitter;
use super::sqlgen::quote_identifier;
use crate::connections::SecretBag;
use crate::error::{AppError, AppResult};
use crate::ssh::{SshTunnel, known_hosts::KnownHosts};

const KEEPALIVE_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone)]
enum Endpoint {
    Tcp { host: String, port: u16 },
    #[cfg_attr(not(unix), allow(dead_code))]
    Socket(PathBuf),
}

/// Everything needed to open another connection to the same server, for contexts, batches and cancels.
#[derive(Clone)]
pub struct Connector {
    endpoint: Endpoint,
    tls: Option<TlsSettings>,
    tls_required: bool,
    user: String,
    password: Zeroizing<String>,
    database: String,
    timeout: Duration,
    init_sql: Vec<String>,
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(unix)]
fn socket_endpoint(raw: &str, port: u16) -> AppResult<Endpoint> {
    let path = PathBuf::from(non_empty(raw).ok_or_else(|| AppError::general("Socket path is empty"))?);
    Ok(Endpoint::Socket(if path.is_dir() {
        path.join(format!(".s.PGSQL.{port}"))
    } else {
        path
    }))
}

#[cfg(not(unix))]
fn socket_endpoint(_raw: &str, _port: u16) -> AppResult<Endpoint> {
    Err(AppError::general(
        "Postgres can't be reached through a socket on Windows. Use TCP/IP instead.",
    ))
}

/// Startup `options` value that puts `schema` first on the search path, with `public` after it.
pub fn search_path_option(schema: &str) -> String {
    let mut path = quote_identifier(schema);
    if schema != "public" {
        path.push_str(",public");
    }

    let mut option = String::from("-c search_path=");
    for character in path.chars() {
        if character.is_whitespace() || character == '\\' {
            option.push('\\');
        }
        option.push(character);
    }
    option
}

fn tls_settings(options: &PostgresOptions) -> AppResult<Option<TlsSettings>> {
    let ssl = &options.ssl;
    if ssl.mode == PostgresSslMode::Disable || options.method == PostgresConnectMethod::Socket {
        return Ok(None);
    }

    let ca_file = non_empty(&ssl.ca_file);
    let verification = match ssl.mode {
        PostgresSslMode::VerifyFull => Verification::Full,
        PostgresSslMode::VerifyCa => Verification::Ca,
        PostgresSslMode::Require if ca_file.is_some() => Verification::Ca,
        _ => Verification::None,
    };

    let identity = match (non_empty(&ssl.cert_file), non_empty(&ssl.key_file)) {
        (Some(cert), Some(key)) => Some((cert, key)),
        (None, None) => None,
        _ => {
            return Err(AppError::general(
                "SSL needs both a client certificate and a key file, or neither",
            ));
        }
    };

    TlsSettings::new(
        verification,
        &options.host,
        ca_file.as_deref(),
        identity.as_ref().map(|(cert, key)| (cert.as_str(), key.as_str())),
    )
    .map(Some)
}

impl Connector {
    pub async fn prepare(
        options: &PostgresOptions,
        secrets: &SecretBag,
        known_hosts: &KnownHosts,
    ) -> AppResult<(Self, Option<SshTunnel>)> {
        let timeout = Duration::from_secs(u64::from(options.connect_timeout_secs.max(1)));
        let password = secrets
            .get(PASSWORD_SLOT)
            .ok_or_else(|| AppError::secrets_required(vec![PostgresOptions::password_slot()]))?;
        let user = non_empty(&options.user).ok_or_else(|| AppError::general("User is empty"))?;
        let host = options.host.trim().to_string();
        let tls = tls_settings(options)?;

        let (endpoint, tunnel) = match options.method {
            PostgresConnectMethod::Tcp => (Endpoint::Tcp { host, port: options.port }, None),
            PostgresConnectMethod::Socket => (socket_endpoint(&options.socket_path, options.port)?, None),
            PostgresConnectMethod::SshTunnel => {
                let tunnel =
                    SshTunnel::open(&options.ssh, secrets, known_hosts, &host, options.port, 0, timeout).await?;
                (
                    Endpoint::Tcp {
                        host: "127.0.0.1".to_string(),
                        port: tunnel.local_port(),
                    },
                    Some(tunnel),
                )
            }
        };

        let connector = Self {
            endpoint,
            tls,
            tls_required: options.ssl.mode != PostgresSslMode::Prefer,
            // Postgres itself falls back to the user name when no database is given.
            database: non_empty(&options.database).unwrap_or_else(|| user.clone()),
            user,
            password: Zeroizing::new(password.to_string()),
            timeout,
            init_sql: splitter::split(&options.init_sql)
                .into_iter()
                .map(|statement| statement.text)
                .collect(),
        };

        Ok((connector, tunnel))
    }

    pub fn database(&self) -> &str {
        &self.database
    }

    /// The same server, login and transport, for another database.
    pub fn with_database(&self, database: &str) -> Self {
        Self {
            database: database.to_string(),
            ..self.clone()
        }
    }

    async fn open_socket(&self) -> Result<BoxSocket, PgError> {
        match &self.endpoint {
            Endpoint::Tcp { host, port } => {
                let unreachable = |error: std::io::Error| PgError::Transport(format!("Couldn't reach {host}:{port}: {error}"));
                let stream = TcpStream::connect((host.as_str(), *port)).await.map_err(unreachable)?;
                stream.set_nodelay(true)?;
                SockRef::from(&stream).set_tcp_keepalive(&TcpKeepalive::new().with_time(KEEPALIVE_AFTER))?;
                Ok(Box::new(stream))
            }
            #[cfg(unix)]
            Endpoint::Socket(path) => {
                let stream = UnixStream::connect(path).await.map_err(|error| {
                    PgError::Transport(format!("Couldn't open the socket {}: {error}", path.display()))
                })?;
                Ok(Box::new(stream))
            }
            #[cfg(not(unix))]
            Endpoint::Socket(_) => Err(PgError::Transport(
                "Postgres can't be reached through a socket on Windows".to_string(),
            )),
        }
    }

    async fn secure(&self, socket: BoxSocket) -> Result<BoxSocket, PgError> {
        let Some(tls) = &self.tls else {
            return Ok(socket);
        };

        match negotiate(socket, tls).await {
            Ok(Negotiated::Tls(socket)) => Ok(socket),
            Ok(Negotiated::Declined(socket)) if !self.tls_required => Ok(socket),
            Ok(Negotiated::Declined(_)) => Err(PgError::Refused(
                "The server doesn't accept SSL connections. Set SSL to Off or If available.".to_string(),
            )),
            Err(_) if !self.tls_required => self.open_socket().await,
            Err(error) => Err(error),
        }
    }

    async fn establish(&self, schema: Option<&str>) -> Result<PgConnection, PgError> {
        let socket = self.secure(self.open_socket().await?).await?;
        let params = StartupParams {
            user: &self.user,
            password: &self.password,
            database: &self.database,
            options: schema.map(search_path_option),
        };

        let mut connection = PgConnection::startup(socket, &params).await?;
        for statement in &self.init_sql {
            connection.execute(statement).await?;
        }
        Ok(connection)
    }

    /// `schema` goes first on the new connection's search path.
    pub async fn connect(&self, schema: Option<&str>) -> Result<PgConnection, PgError> {
        match tokio::time::timeout(self.timeout, self.establish(schema)).await {
            Ok(result) => result,
            Err(_) => Err(PgError::Transport("Timed out connecting to the server".to_string())),
        }
    }

    pub async fn cancel(&self, key: CancelKey) -> Result<(), PgError> {
        let socket = self.open_socket().await?;
        match tokio::time::timeout(self.timeout, send_cancel(socket, key)).await {
            Ok(result) => result,
            Err(_) => Err(PgError::Transport("Timed out sending the cancel request".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::search_path_option;

    #[test]
    fn escapes_search_path_options() {
        assert_eq!(search_path_option("public"), r#"-c search_path="public""#);
        assert_eq!(search_path_option("app"), r#"-c search_path="app",public"#);
        assert_eq!(search_path_option(r"my sc\hema"), r#"-c search_path="my\ sc\\hema",public"#);
    }
}
