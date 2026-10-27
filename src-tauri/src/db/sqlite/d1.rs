use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use super::connection::{RawColumn, RawResult, SqlValue, SqliteConnection, SqliteConnector};
use super::session::SqliteSession;
use crate::connections::{SecretBag, SecretSlot};
use crate::db::types::BatchOutcome;
use crate::error::{AppError, AppResult, ErrorDetail};

pub const API_TOKEN_SLOT: &str = "apiToken";
const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const DATABASES_PER_PAGE: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct D1Options {
    pub account_id: String,
    /// A database name or its UUID.
    pub database: String,
    pub timeout_secs: u32,
}

impl Default for D1Options {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            database: String::new(),
            timeout_secs: 30,
        }
    }
}

impl D1Options {
    pub fn required_secrets(&self) -> Vec<SecretSlot> {
        vec![SecretSlot::new(API_TOKEN_SLOT, "API token")]
    }

    pub fn optional_secrets(&self) -> Vec<SecretSlot> {
        Vec::new()
    }
}

pub async fn connect(options: &D1Options, secrets: &SecretBag) -> AppResult<SqliteSession> {
    SqliteSession::open(Arc::new(D1Connector::new(options, secrets).await?)).await
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    errors: Vec<ApiMessage>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiMessage {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize, Clone)]
struct DatabaseEntry {
    uuid: String,
    name: String,
}

#[derive(Deserialize, Default)]
struct RawEntry {
    results: Option<RawRows>,
    #[serde(default)]
    meta: Meta,
}

#[derive(Deserialize, Default)]
struct RawRows {
    #[serde(default)]
    columns: Vec<String>,
    #[serde(default)]
    rows: Vec<Vec<Value>>,
}

#[derive(Deserialize, Default)]
struct Meta {
    #[serde(default)]
    changes: u64,
    #[serde(default)]
    last_row_id: i64,
    duration: Option<f64>,
}

/// D1 wraps SQLite errors as `D1_ERROR: <message>: SQLITE_ERROR`, and only the middle part says anything.
fn clean_message(message: &str) -> String {
    let message = message.trim();
    let message = message.strip_prefix("D1_ERROR: ").unwrap_or(message);
    match message.rsplit_once(": SQLITE_") {
        Some((head, code)) if code.chars().all(|character| character.is_ascii_uppercase() || character == '_') => {
            head.to_string()
        }
        _ => message.to_string(),
    }
}

fn api_error(errors: &[ApiMessage], status: reqwest::StatusCode) -> AppError {
    let Some(first) = errors.first() else {
        return AppError::general(format!("Cloudflare answered with HTTP {status}"));
    };
    let message = errors.iter().map(|error| clean_message(&error.message)).collect::<Vec<_>>().join("\n");
    AppError::new(
        message,
        ErrorDetail::Database {
            code: u16::try_from(first.code).ok(),
            sql_state: None,
        },
    )
}

struct Api {
    client: reqwest::Client,
    token: Zeroizing<String>,
    account_id: String,
}

impl Api {
    fn url(&self, path: &str) -> String {
        format!("{API_BASE}/accounts/{}/d1/database{path}", self.account_id)
    }

    async fn send<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> AppResult<T> {
        let response = request
            .bearer_auth(self.token.as_str())
            .send()
            .await
            .map_err(|error| AppError::general(format!("Couldn't reach Cloudflare: {error}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| AppError::general(format!("Couldn't read Cloudflare's answer: {error}")))?;
        let envelope: Envelope<T> = serde_json::from_str(&body)
            .map_err(|_| AppError::general(format!("Cloudflare answered with HTTP {status}")))?;
        match envelope.result {
            Some(result) if envelope.success => Ok(result),
            _ => Err(api_error(&envelope.errors, status)),
        }
    }

    async fn databases(&self) -> AppResult<Vec<DatabaseEntry>> {
        let mut databases = Vec::new();
        for page in 1.. {
            let request = self.client.get(self.url(&format!("?page={page}&per_page={DATABASES_PER_PAGE}")));
            let found: Vec<DatabaseEntry> = self.send(request).await?;
            let done = found.len() < DATABASES_PER_PAGE;
            databases.extend(found);
            if done {
                break;
            }
        }
        Ok(databases)
    }
}

/// One D1 database in a Cloudflare account, reached over the HTTP API.
pub struct D1Connector {
    api: Arc<Api>,
    database: DatabaseEntry,
}

impl D1Connector {
    async fn new(options: &D1Options, secrets: &SecretBag) -> AppResult<Self> {
        let token = secrets
            .get(API_TOKEN_SLOT)
            .ok_or_else(|| AppError::secrets_required(options.required_secrets()))?;
        let account_id = options.account_id.trim();
        if account_id.is_empty() {
            return Err(AppError::general("Enter the Cloudflare account ID"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(u64::from(options.timeout_secs.max(1))))
            .build()
            .map_err(|error| AppError::general(error.to_string()))?;
        let api = Arc::new(Api {
            client,
            token: Zeroizing::new(token.to_string()),
            account_id: account_id.to_string(),
        });
        let database = Self::find(&api, options.database.trim()).await?;
        Ok(Self { api, database })
    }

    async fn find(api: &Api, wanted: &str) -> AppResult<DatabaseEntry> {
        if wanted.is_empty() {
            return Err(AppError::general("Enter the D1 database name or ID"));
        }
        api.databases()
            .await?
            .into_iter()
            .find(|database| database.name == wanted || database.uuid.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| AppError::not_found(format!("There's no D1 database called {wanted} in this account")))
    }
}

#[async_trait]
impl SqliteConnector for D1Connector {
    fn product(&self) -> &'static str {
        "Cloudflare D1"
    }

    async fn connect(&self) -> AppResult<Arc<dyn SqliteConnection>> {
        Ok(Arc::new(D1Connection {
            api: self.api.clone(),
            url: self.api.url(&format!("/{}/raw", self.database.uuid)),
        }))
    }

    fn is_internal_table(&self, name: &str) -> bool {
        name.starts_with("sqlite_") || name.starts_with("_cf_")
    }

    fn catalog(&self) -> Option<String> {
        Some(self.database.name.clone())
    }

    async fn list_catalogs(&self) -> AppResult<Vec<String>> {
        let mut names: Vec<String> = self.api.databases().await?.into_iter().map(|database| database.name).collect();
        names.sort();
        Ok(names)
    }

    async fn with_catalog(&self, catalog: &str) -> AppResult<Arc<dyn SqliteConnector>> {
        Ok(Arc::new(Self {
            api: self.api.clone(),
            database: Self::find(&self.api, catalog).await?,
        }))
    }
}

/// Every call is its own HTTP request, so nothing carries over between statements,
/// and BEGIN and COMMIT aren't allowed. A batch request runs as one transaction instead.
struct D1Connection {
    api: Arc<Api>,
    url: String,
}

fn param_text(value: &SqlValue) -> AppResult<String> {
    match value {
        SqlValue::Text(value) => Ok(value.clone()),
        SqlValue::Integer(value) => Ok(value.to_string()),
        SqlValue::Real(value) => Ok(value.to_string()),
        SqlValue::Null | SqlValue::Blob(_) => Err(AppError::general("D1 only takes text parameters")),
    }
}

/// D1 sends blobs as arrays of byte values.
fn sql_value(value: Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(value) => SqlValue::Integer(i64::from(value)),
        Value::Number(number) => match number.as_i64() {
            Some(value) => SqlValue::Integer(value),
            None => SqlValue::Real(number.as_f64().unwrap_or(f64::NAN)),
        },
        Value::String(value) => SqlValue::Text(value),
        Value::Array(items) => {
            let bytes: Option<Vec<u8>> =
                items.iter().map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok())).collect();
            match bytes {
                Some(bytes) => SqlValue::Blob(bytes),
                None => SqlValue::Text(Value::Array(items).to_string()),
            }
        }
        other => SqlValue::Text(other.to_string()),
    }
}

fn is_insert(sql: &str) -> bool {
    let first = sql.split_whitespace().next().unwrap_or_default();
    ["INSERT", "REPLACE"].iter().any(|keyword| first.eq_ignore_ascii_case(keyword))
}

fn raw_result(sql: &str, entry: RawEntry, row_limit: usize) -> RawResult {
    let rows = entry.results.unwrap_or_default();
    let row_limit = if row_limit == 0 { usize::MAX } else { row_limit };
    let truncated = rows.rows.len() > row_limit;
    let changes = entry.meta.changes;
    RawResult {
        columns: rows
            .columns
            .into_iter()
            .map(|name| RawColumn {
                name,
                ..Default::default()
            })
            .collect(),
        rows: rows
            .rows
            .into_iter()
            .take(row_limit)
            .map(|row| row.into_iter().map(sql_value).collect())
            .collect(),
        truncated,
        changes,
        last_insert_id: (changes > 0 && is_insert(sql)).then_some(entry.meta.last_row_id),
        server_ms: entry.meta.duration,
    }
}

#[async_trait]
impl SqliteConnection for D1Connection {
    async fn query(&self, sql: &str, params: &[SqlValue], row_limit: usize) -> AppResult<RawResult> {
        let params = params.iter().map(param_text).collect::<AppResult<Vec<_>>>()?;
        let request = self.api.client.post(&self.url).json(&json!({ "sql": sql, "params": params }));
        let entries: Vec<RawEntry> = self.api.send(request).await?;
        let entry = entries.into_iter().next().unwrap_or_default();
        Ok(raw_result(sql, entry, row_limit))
    }

    /// D1 doesn't say which statement of a batch failed, only that the whole batch rolled back.
    async fn run_batch(&self, statements: &[String], _transactional: bool) -> BatchOutcome {
        let batch: Vec<Value> = statements.iter().map(|sql| json!({ "sql": sql })).collect();
        let request = self.api.client.post(&self.url).json(&json!({ "batch": batch }));
        match self.api.send::<Vec<RawEntry>>(request).await {
            Ok(_) => BatchOutcome {
                executed: statements.len(),
                failed_index: None,
                error: None,
                rolled_back: false,
            },
            Err(error) => BatchOutcome {
                executed: 0,
                failed_index: None,
                error: Some(error),
                rolled_back: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_raw_results() {
        let body = r#"{"success":true,"errors":[],"result":[{"success":true,
            "meta":{"changes":1,"last_row_id":7,"duration":0.25},
            "results":{"columns":["id","name","data","score"],"rows":[[1,"a",[222,173],1.5],[2,null,null,2]]}}]}"#;
        let envelope: Envelope<Vec<RawEntry>> = serde_json::from_str(body).unwrap();
        let entry = envelope.result.unwrap().into_iter().next().unwrap();
        let raw = raw_result("insert into t values (1)", entry, 1);

        assert_eq!(raw.columns.len(), 4);
        assert!(raw.truncated);
        assert_eq!(raw.rows, vec![vec![
            SqlValue::Integer(1),
            SqlValue::Text("a".into()),
            SqlValue::Blob(vec![222, 173]),
            SqlValue::Real(1.5),
        ]]);
        assert_eq!(raw.last_insert_id, Some(7));
        assert_eq!(raw.server_ms, Some(0.25));
    }

    #[test]
    fn only_inserts_report_a_row_id() {
        let entry = RawEntry {
            meta: Meta { changes: 3, last_row_id: 7, duration: None },
            ..Default::default()
        };
        assert_eq!(raw_result("UPDATE t SET a = 1", entry, 10).last_insert_id, None);
    }

    #[test]
    fn cleans_error_messages() {
        assert_eq!(clean_message("D1_ERROR: no such table: users: SQLITE_ERROR"), "no such table: users");
        assert_eq!(clean_message("Authentication error"), "Authentication error");
        let body = r#"{"success":false,"errors":[{"code":7500,"message":"D1_ERROR: near \"SELEC\": syntax error: SQLITE_ERROR"}],"result":null}"#;
        let envelope: Envelope<Vec<RawEntry>> = serde_json::from_str(body).unwrap();
        let error = api_error(&envelope.errors, reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "near \"SELEC\": syntax error");
    }
}
