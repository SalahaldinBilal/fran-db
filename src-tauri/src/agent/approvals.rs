use std::{collections::HashMap, sync::Mutex};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::db::DatabaseKind;

/// The same shape as the review dialog's `ApplySafety`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ApplySafety {
    Transaction,
    Schema { atomic: bool },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub session_id: String,
    pub agent: String,
    pub connection_id: String,
    pub connection_name: String,
    pub kind: DatabaseKind,
    pub database: Option<String>,
    pub tool: String,
    pub arguments: Option<Value>,
    pub statements: Vec<String>,
    pub safety: ApplySafety,
    pub preview: Option<RowPreview>,
    pub requested_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RowAction {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub column: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCell {
    pub column: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRow {
    pub action: RowAction,
    pub key: Vec<KeyValue>,
    pub cells: Vec<PreviewCell>,
    /// Whether a row has this key right now. None when it wasn't looked up or the lookup failed.
    pub found: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowPreview {
    pub table: String,
    pub rows: Vec<PreviewRow>,
    pub more: usize,
}

/// Changes waiting for the user. Dropping a request's sender tells its waiting call it was withdrawn.
#[derive(Default)]
pub struct Approvals {
    pending: Mutex<HashMap<String, (ApprovalRequest, oneshot::Sender<bool>)>>,
}

impl Approvals {
    pub fn open(&self, request: ApprovalRequest) -> oneshot::Receiver<bool> {
        let (sender, receiver) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(request.id.clone(), (request, sender));
        }
        receiver
    }

    pub fn answer(&self, id: &str, approved: bool) -> bool {
        let entry = self.pending.lock().ok().and_then(|mut pending| pending.remove(id));
        match entry {
            Some((_, sender)) => sender.send(approved).is_ok(),
            None => false,
        }
    }

    pub fn withdraw(&self, id: &str) -> bool {
        self.pending.lock().ok().and_then(|mut pending| pending.remove(id)).is_some()
    }

    pub fn list(&self) -> Vec<ApprovalRequest> {
        let mut list: Vec<ApprovalRequest> = match self.pending.lock() {
            Ok(pending) => pending.values().map(|(request, _)| request.clone()).collect(),
            Err(_) => Vec::new(),
        };
        list.sort_by_key(|request| request.requested_at);
        list
    }
}
