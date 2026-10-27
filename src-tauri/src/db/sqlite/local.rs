use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use rusqlite::{Connection, InterruptHandle, OpenFlags, types::ValueRef};

use super::connection::{RawColumn, RawResult, SqlValue, SqliteConnection, SqliteConnector};
use super::options::SqliteOptions;
use crate::error::{AppError, AppResult, ErrorDetail};

fn sqlite_error(error: rusqlite::Error) -> AppError {
    match error {
        rusqlite::Error::SqliteFailure(failure, message) => AppError::new(
            message.unwrap_or_else(|| failure.to_string()),
            ErrorDetail::Database {
                code: u16::try_from(failure.extended_code).ok(),
                sql_state: None,
            },
        ),
        other => AppError::general(other.to_string()),
    }
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| AppError::general(format!("SQLite worker failed: {error}")))?
}

pub struct LocalConnector {
    path: PathBuf,
    options: SqliteOptions,
}

impl LocalConnector {
    pub fn new(options: &SqliteOptions) -> AppResult<Self> {
        let path = options.path.trim();
        if path.is_empty() {
            return Err(AppError::general("Choose a database file"));
        }
        let path = PathBuf::from(path);
        if !path.exists() && (options.read_only || !options.create_if_missing) {
            return Err(AppError::not_found(format!("{} doesn't exist", path.display())));
        }
        Ok(Self {
            path,
            options: options.clone(),
        })
    }

    fn open(path: &PathBuf, options: &SqliteOptions) -> AppResult<Connection> {
        let mut flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
        if options.read_only {
            flags |= OpenFlags::SQLITE_OPEN_READ_ONLY;
        } else {
            flags |= OpenFlags::SQLITE_OPEN_READ_WRITE;
            if options.create_if_missing {
                flags |= OpenFlags::SQLITE_OPEN_CREATE;
            }
        }

        let conn = Connection::open_with_flags(path, flags).map_err(sqlite_error)?;
        conn.busy_timeout(Duration::from_secs(u64::from(options.busy_timeout_secs)))
            .map_err(sqlite_error)?;
        if options.foreign_keys {
            conn.execute_batch("PRAGMA foreign_keys = ON").map_err(sqlite_error)?;
        }
        if !options.init_sql.trim().is_empty() {
            conn.execute_batch(&options.init_sql).map_err(sqlite_error)?;
        }
        Ok(conn)
    }
}

#[async_trait]
impl SqliteConnector for LocalConnector {
    fn product(&self) -> &'static str {
        "SQLite"
    }

    async fn connect(&self) -> AppResult<Arc<dyn SqliteConnection>> {
        self.connect_with(self.options.clone()).await
    }

    async fn connect_read_only(&self) -> AppResult<Arc<dyn SqliteConnection>> {
        self.connect_with(SqliteOptions {
            read_only: true,
            create_if_missing: false,
            ..self.options.clone()
        })
        .await
    }
}

impl LocalConnector {
    async fn connect_with(&self, options: SqliteOptions) -> AppResult<Arc<dyn SqliteConnection>> {
        let path = self.path.clone();
        let conn = blocking(move || Self::open(&path, &options)).await?;
        Ok(Arc::new(LocalConnection {
            interrupt: conn.get_interrupt_handle(),
            conn: Arc::new(Mutex::new(conn)),
        }))
    }
}

struct LocalConnection {
    conn: Arc<Mutex<Connection>>,
    interrupt: InterruptHandle,
}

fn owned(value: ValueRef<'_>) -> SqlValue {
    match value {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(value) => SqlValue::Integer(value),
        ValueRef::Real(value) => SqlValue::Real(value),
        ValueRef::Text(bytes) => SqlValue::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => SqlValue::Blob(bytes.to_vec()),
    }
}

fn bind(statement: &mut rusqlite::Statement<'_>, params: &[SqlValue]) -> rusqlite::Result<()> {
    use rusqlite::types::Value;
    for (index, param) in params.iter().enumerate() {
        let value = match param {
            SqlValue::Null => Value::Null,
            SqlValue::Integer(value) => Value::Integer(*value),
            SqlValue::Real(value) => Value::Real(*value),
            SqlValue::Text(value) => Value::Text(value.clone()),
            SqlValue::Blob(value) => Value::Blob(value.clone()),
        };
        statement.raw_bind_parameter(index + 1, value)?;
    }
    Ok(())
}

fn run(conn: &Connection, sql: &str, params: &[SqlValue], row_limit: usize) -> rusqlite::Result<RawResult> {
    let mut statement = conn.prepare(sql)?;
    bind(&mut statement, params)?;

    let mut columns: Vec<RawColumn> = statement
        .columns()
        .into_iter()
        .map(|column| RawColumn {
            name: column.name().to_string(),
            decl_type: column.decl_type().map(str::to_string),
            ..Default::default()
        })
        .collect();
    for (column, source) in columns.iter_mut().zip(statement.columns_with_metadata()) {
        column.schema = source.database_name().map(str::to_string);
        column.table = source.table_name().map(str::to_string);
        column.origin = source.origin_name().map(str::to_string);
    }

    let total_before = conn.total_changes();
    let rowid_before = conn.last_insert_rowid();
    let row_limit = if row_limit == 0 { usize::MAX } else { row_limit };
    let mut rows = Vec::new();
    let mut truncated = false;

    if columns.is_empty() {
        statement.raw_execute()?;
    } else {
        let width = columns.len();
        let mut cursor = statement.raw_query();
        while let Some(row) = cursor.next()? {
            if rows.len() >= row_limit {
                truncated = true;
                break;
            }
            rows.push((0..width).map(|index| row.get_ref(index).map(owned)).collect::<rusqlite::Result<_>>()?);
        }
    }

    let changes = if conn.total_changes() == total_before { 0 } else { conn.changes() };
    let rowid = conn.last_insert_rowid();

    Ok(RawResult {
        columns,
        rows,
        truncated,
        changes,
        last_insert_id: (changes > 0 && rowid != rowid_before).then_some(rowid),
        server_ms: None,
    })
}

#[async_trait]
impl SqliteConnection for LocalConnection {
    async fn query(&self, sql: &str, params: &[SqlValue], row_limit: usize) -> AppResult<RawResult> {
        let conn = self.conn.clone();
        let sql = sql.to_string();
        let params = params.to_vec();
        blocking(move || {
            let conn = conn.lock().map_err(|_| AppError::general("SQLite connection lock poisoned"))?;
            run(&conn, &sql, &params, row_limit).map_err(sqlite_error)
        })
        .await
    }

    fn interrupt(&self) {
        self.interrupt.interrupt();
    }
}
