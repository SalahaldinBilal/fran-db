mod auth;
mod tls;

use std::{collections::HashMap, error::Error as StdError, fmt::Display, io, time::Instant};

use bytes::{BufMut, BytesMut};
use fallible_iterator::FallibleIterator;
use postgres_protocol::{
    IsNull,
    message::{
        backend::{DataRowBody, ErrorFields, Message, RowDescriptionBody},
        frontend::{self, BindError},
    },
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{AppError, ErrorDetail};

pub use auth::StartupParams;
pub use tls::{Negotiated, TlsSettings, Verification, negotiate};

pub trait Socket: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Socket for T {}
pub type BoxSocket = Box<dyn Socket>;

const READ_CHUNK: usize = 16 * 1024;

/// An error or notice as the server reported it.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug)]
pub enum PgError {
    /// The server rejected the statement. The connection is still usable.
    Server(Report),
    /// Fran DB refused something, like COPY. The connection is still usable.
    Refused(String),
    /// The connection is gone or out of step and has to be dropped.
    Transport(String),
    /// Anything else, such as an object that doesn't exist. The connection is still usable.
    Other(AppError),
}

impl From<AppError> for PgError {
    fn from(error: AppError) -> Self {
        Self::Other(error)
    }
}

impl PgError {
    pub fn transport(error: impl Display) -> Self {
        Self::Transport(error.to_string())
    }

    pub fn connection_lost(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl From<io::Error> for PgError {
    fn from(error: io::Error) -> Self {
        Self::Transport(error.to_string())
    }
}

impl From<PgError> for AppError {
    fn from(error: PgError) -> Self {
        match error {
            PgError::Server(report) => {
                let mut message = report.message;
                if let Some(detail) = report.detail {
                    message.push_str("\nDETAIL: ");
                    message.push_str(&detail);
                }
                if let Some(hint) = report.hint {
                    message.push_str("\nHINT: ");
                    message.push_str(&hint);
                }
                AppError::new(
                    message,
                    ErrorDetail::Database {
                        code: None,
                        sql_state: (!report.code.is_empty()).then_some(report.code),
                    },
                )
            }
            PgError::Refused(message) | PgError::Transport(message) => AppError::new(
                message,
                ErrorDetail::Database {
                    code: None,
                    sql_state: None,
                },
            ),
            PgError::Other(error) => error,
        }
    }
}

fn encoding(error: impl Display) -> PgError {
    PgError::Refused(format!("The query couldn't be sent: {error}"))
}

#[derive(Debug, Clone, Copy)]
pub struct CancelKey {
    pub process_id: i32,
    pub secret_key: i32,
}

/// One column of a RowDescription. `table_oid` and `column_id` are 0 when the value isn't a plain table column.
#[derive(Debug, Clone)]
pub struct RawColumn {
    pub name: String,
    pub table_oid: u32,
    pub column_id: i16,
    pub type_oid: u32,
    pub type_modifier: i32,
}

#[derive(Debug, Clone, Default)]
pub struct RawResult {
    pub columns: Vec<RawColumn>,
    pub rows: Vec<Vec<Option<String>>>,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct SimpleOutcome {
    pub results: Vec<RawResult>,
    pub tags: Vec<String>,
    pub first_reply: Option<Instant>,
}

pub type TextRow = Vec<Option<String>>;

pub struct PgConnection {
    socket: BoxSocket,
    read_buf: BytesMut,
    write_buf: BytesMut,
    parameters: HashMap<String, String>,
    key: Option<CancelKey>,
    notices: Vec<Report>,
}

fn report(mut fields: ErrorFields<'_>) -> Result<Report, PgError> {
    let mut report = Report::default();
    let mut localized_severity = String::new();

    while let Some(field) = fields.next().map_err(PgError::transport)? {
        let value = String::from_utf8_lossy(field.value_bytes()).into_owned();
        match field.type_() {
            b'S' => localized_severity = value,
            b'V' => report.severity = value,
            b'C' => report.code = value,
            b'M' => report.message = value,
            b'D' => report.detail = Some(value),
            b'H' => report.hint = Some(value),
            _ => {}
        }
    }

    if report.severity.is_empty() {
        report.severity = localized_severity;
    }
    Ok(report)
}

fn columns(body: &RowDescriptionBody) -> Result<Vec<RawColumn>, PgError> {
    body.fields()
        .map(|field| {
            Ok(RawColumn {
                name: field.name().to_string(),
                table_oid: field.table_oid(),
                column_id: field.column_id(),
                type_oid: field.type_oid(),
                type_modifier: field.type_modifier(),
            })
        })
        .collect::<Vec<_>>()
        .map_err(PgError::transport)
}

fn row_values(body: &DataRowBody) -> Result<TextRow, PgError> {
    let buffer = body.buffer();
    body.ranges()
        .map(|range| Ok(range.map(|range| String::from_utf8_lossy(&buffer[range]).into_owned())))
        .collect::<Vec<_>>()
        .map_err(PgError::transport)
}

fn write_text(value: Option<&str>, buf: &mut BytesMut) -> Result<IsNull, Box<dyn StdError + Sync + Send>> {
    match value {
        Some(text) => {
            buf.put_slice(text.as_bytes());
            Ok(IsNull::No)
        }
        None => Ok(IsNull::Yes),
    }
}

impl PgConnection {
    fn new(socket: BoxSocket) -> Self {
        Self {
            socket,
            read_buf: BytesMut::with_capacity(READ_CHUNK),
            write_buf: BytesMut::new(),
            parameters: HashMap::new(),
            key: None,
            notices: Vec::new(),
        }
    }

    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters.get(name).map(String::as_str)
    }

    pub fn cancel_key(&self) -> Option<CancelKey> {
        self.key
    }

    pub fn take_notices(&mut self) -> Vec<Report> {
        std::mem::take(&mut self.notices)
    }

    async fn send(&mut self) -> Result<(), PgError> {
        self.socket.write_all(&self.write_buf).await?;
        self.socket.flush().await?;
        self.write_buf.clear();
        Ok(())
    }

    /// The next message that needs an answer. Status updates and notices are taken in along the way.
    async fn receive(&mut self) -> Result<Message, PgError> {
        loop {
            match Message::parse(&mut self.read_buf).map_err(PgError::transport)? {
                Some(Message::ParameterStatus(body)) => {
                    let name = body.name().map_err(PgError::transport)?.to_string();
                    let value = body.value().map_err(PgError::transport)?.to_string();
                    self.parameters.insert(name, value);
                }
                Some(Message::NoticeResponse(body)) => {
                    let notice = report(body.fields())?;
                    self.notices.push(notice);
                }
                Some(Message::NotificationResponse(body)) => {
                    let channel = body.channel().map_err(PgError::transport)?;
                    let payload = body.message().map_err(PgError::transport)?;
                    self.notices.push(Report {
                        severity: "NOTIFY".to_string(),
                        message: format!("{channel}: {payload}"),
                        ..Report::default()
                    });
                }
                Some(message) => return Ok(message),
                None => {
                    if self.read_buf.capacity() - self.read_buf.len() < READ_CHUNK / 4 {
                        self.read_buf.reserve(READ_CHUNK);
                    }
                    if self.socket.read_buf(&mut self.read_buf).await? == 0 {
                        return Err(PgError::Transport("The server closed the connection".to_string()));
                    }
                }
            }
        }
    }

    /// Runs SQL with the simple query protocol. Rows past `row_limit` are read and dropped, 0 means no limit.
    pub async fn simple_query(&mut self, sql: &str, row_limit: usize) -> Result<SimpleOutcome, PgError> {
        self.write_buf.clear();
        frontend::query(sql, &mut self.write_buf).map_err(encoding)?;
        self.send().await?;

        let limit = if row_limit == 0 { usize::MAX } else { row_limit };
        let mut outcome = SimpleOutcome::default();
        let mut failure: Option<PgError> = None;

        loop {
            let message = self.receive().await?;
            outcome.first_reply.get_or_insert_with(Instant::now);

            match message {
                Message::RowDescription(body) => outcome.results.push(RawResult {
                    columns: columns(&body)?,
                    ..RawResult::default()
                }),
                Message::DataRow(body) => {
                    if let Some(result) = outcome.results.last_mut() {
                        if result.rows.len() >= limit {
                            result.truncated = true;
                        } else {
                            result.rows.push(row_values(&body)?);
                        }
                    }
                }
                Message::CommandComplete(body) => {
                    outcome.tags.push(body.tag().map_err(PgError::transport)?.to_string());
                }
                Message::ErrorResponse(body) => {
                    let error = report(body.fields())?;
                    failure.get_or_insert(PgError::Server(error));
                }
                Message::CopyInResponse(_) => {
                    frontend::copy_fail("Fran DB doesn't send COPY data", &mut self.write_buf).map_err(encoding)?;
                    self.send().await?;
                    failure.get_or_insert(PgError::Refused(
                        "COPY FROM STDIN isn't supported here. Use INSERT, or COPY from a file on the server.".to_string(),
                    ));
                }
                Message::CopyOutResponse(_) => {
                    failure.get_or_insert(PgError::Refused(
                        "COPY TO STDOUT isn't supported here, so its output was discarded. Use a SELECT instead.".to_string(),
                    ));
                }
                Message::ReadyForQuery(_) => break,
                _ => {}
            }
        }

        match failure {
            Some(error) => Err(error),
            None => Ok(outcome),
        }
    }

    /// Runs one statement with the extended protocol, parameters and results both as text.
    pub async fn query_text(&mut self, sql: &str, params: &[Option<&str>]) -> Result<Vec<TextRow>, PgError> {
        self.write_buf.clear();
        frontend::parse("", sql, std::iter::empty(), &mut self.write_buf).map_err(encoding)?;
        frontend::bind("", "", [0i16], params.iter().copied(), write_text, [0i16], &mut self.write_buf).map_err(
            |error| match error {
                BindError::Conversion(error) => encoding(error),
                BindError::Serialization(error) => encoding(error),
            },
        )?;
        frontend::execute("", 0, &mut self.write_buf).map_err(encoding)?;
        frontend::sync(&mut self.write_buf);
        self.send().await?;

        let mut rows = Vec::new();
        let mut failure: Option<PgError> = None;

        loop {
            match self.receive().await? {
                Message::DataRow(body) => rows.push(row_values(&body)?),
                Message::ErrorResponse(body) => {
                    let error = report(body.fields())?;
                    failure.get_or_insert(PgError::Server(error));
                }
                Message::CopyInResponse(_) => {
                    frontend::copy_fail("Fran DB doesn't send COPY data", &mut self.write_buf).map_err(encoding)?;
                    frontend::sync(&mut self.write_buf);
                    self.send().await?;
                }
                Message::ReadyForQuery(_) => break,
                _ => {}
            }
        }

        match failure {
            Some(error) => Err(error),
            None => Ok(rows),
        }
    }

    pub async fn execute(&mut self, sql: &str) -> Result<(), PgError> {
        self.simple_query(sql, 1).await.map(|_| ())
    }

    pub async fn ping(&mut self) -> Result<(), PgError> {
        self.simple_query("", 1).await.map(|_| ())
    }

    pub async fn close(mut self) {
        self.write_buf.clear();
        frontend::terminate(&mut self.write_buf);
        let _ = self.send().await;
        let _ = self.socket.shutdown().await;
    }
}

pub async fn send_cancel(mut socket: BoxSocket, key: CancelKey) -> Result<(), PgError> {
    let mut buf = BytesMut::new();
    frontend::cancel_request(key.process_id, key.secret_key, &mut buf);
    socket.write_all(&buf).await?;
    socket.flush().await?;
    let mut rest = [0u8; 1];
    let _ = socket.read(&mut rest).await;
    Ok(())
}
