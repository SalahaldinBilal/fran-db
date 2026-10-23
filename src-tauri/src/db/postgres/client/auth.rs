use fallible_iterator::FallibleIterator;
use postgres_protocol::{
    authentication::{
        md5_hash,
        sasl::{self, ChannelBinding, ScramSha256},
    },
    message::{backend::Message, frontend},
};

use super::{BoxSocket, CancelKey, PgConnection, PgError, encoding, report};

pub struct StartupParams<'a> {
    pub user: &'a str,
    pub password: &'a str,
    pub database: &'a str,
    /// Command-line style server options, such as `-c search_path=...`.
    pub options: Option<String>,
}

fn unsupported(method: &str) -> PgError {
    PgError::Refused(format!(
        "The server asks for {method} authentication, which Fran DB doesn't support. Use a password or trust login."
    ))
}

fn out_of_step(step: &str) -> PgError {
    PgError::Transport(format!("The server sent {step} at the wrong time during login"))
}

impl PgConnection {
    pub async fn startup(socket: BoxSocket, params: &StartupParams<'_>) -> Result<Self, PgError> {
        let mut connection = Self::new(socket);

        let mut parameters = vec![
            ("user", params.user),
            ("database", params.database),
            ("client_encoding", "UTF8"),
            ("DateStyle", "ISO"),
            ("application_name", "Fran DB"),
        ];
        if let Some(options) = params.options.as_deref() {
            parameters.push(("options", options));
        }
        frontend::startup_message(parameters, &mut connection.write_buf).map_err(encoding)?;
        connection.send().await?;

        connection.authenticate(params).await?;

        loop {
            match connection.receive().await? {
                Message::BackendKeyData(body) => {
                    connection.key = Some(CancelKey {
                        process_id: body.process_id(),
                        secret_key: body.secret_key(),
                    });
                }
                Message::ReadyForQuery(_) => return Ok(connection),
                Message::ErrorResponse(body) => return Err(PgError::Server(report(body.fields())?)),
                _ => {}
            }
        }
    }

    async fn authenticate(&mut self, params: &StartupParams<'_>) -> Result<(), PgError> {
        let mut scram: Option<ScramSha256> = None;

        loop {
            match self.receive().await? {
                Message::AuthenticationOk => return Ok(()),
                Message::AuthenticationCleartextPassword => {
                    frontend::password_message(params.password.as_bytes(), &mut self.write_buf).map_err(encoding)?;
                    self.send().await?;
                }
                Message::AuthenticationMd5Password(body) => {
                    let hash = md5_hash(params.user.as_bytes(), params.password.as_bytes(), body.salt());
                    frontend::password_message(hash.as_bytes(), &mut self.write_buf).map_err(encoding)?;
                    self.send().await?;
                }
                Message::AuthenticationSasl(body) => {
                    let mut mechanisms = body.mechanisms();
                    let mut offers_scram = false;
                    while let Some(mechanism) = mechanisms.next().map_err(PgError::transport)? {
                        offers_scram |= mechanism == sasl::SCRAM_SHA_256;
                    }
                    if !offers_scram {
                        return Err(unsupported("a SASL method other than SCRAM-SHA-256"));
                    }

                    // Channel binding is left out, so plain SCRAM-SHA-256 is used even over TLS.
                    let client = ScramSha256::new(params.password.as_bytes(), ChannelBinding::unsupported());
                    frontend::sasl_initial_response(sasl::SCRAM_SHA_256, client.message(), &mut self.write_buf)
                        .map_err(encoding)?;
                    self.send().await?;
                    scram = Some(client);
                }
                Message::AuthenticationSaslContinue(body) => {
                    let client = scram.as_mut().ok_or_else(|| out_of_step("a SASL challenge"))?;
                    client
                        .update(body.data())
                        .map_err(|error| PgError::Refused(format!("Password login failed: {error}")))?;
                    frontend::sasl_response(client.message(), &mut self.write_buf).map_err(encoding)?;
                    self.send().await?;
                }
                Message::AuthenticationSaslFinal(body) => {
                    let client = scram.as_mut().ok_or_else(|| out_of_step("a SASL result"))?;
                    client
                        .finish(body.data())
                        .map_err(|error| PgError::Refused(format!("The server's login proof didn't check out: {error}")))?;
                }
                Message::AuthenticationGss | Message::AuthenticationGssContinue(_) => return Err(unsupported("GSSAPI")),
                Message::AuthenticationSspi => return Err(unsupported("SSPI")),
                Message::AuthenticationKerberosV5 => return Err(unsupported("Kerberos")),
                Message::AuthenticationScmCredential => return Err(unsupported("SCM credential")),
                Message::ErrorResponse(body) => return Err(PgError::Server(report(body.fields())?)),
                _ => {}
            }
        }
    }
}
