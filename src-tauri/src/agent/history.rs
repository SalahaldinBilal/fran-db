use std::{
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};
use crate::vault::Vault;

const MAGIC: &[u8; 8] = b"FRDBHIS1";
const KEY_ID: &str = "agent:historyKey";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HistoryEvent {
    Call,
    SessionClosed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Ok,
    Error,
    /// Turned away by the connection's access or a global switch.
    Refused,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalAnswer {
    Approved,
    Rejected,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    pub answer: ApprovalAnswer,
    pub requested_at: u64,
    pub answered_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub at: u64,
    pub event: HistoryEvent,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub connection_id: Option<String>,
    pub connection_name: Option<String>,
    pub database: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<Value>,
    pub statements: Vec<String>,
    pub approval: Option<ApprovalRecord>,
    pub outcome: Outcome,
    pub message: Option<String>,
    pub duration_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<Value>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// `YYYY-MM-DD` of a day counted from the Unix epoch, in UTC.
fn day_name(day: u64) -> String {
    let z = day as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn day_number(name: &str) -> Option<u64> {
    let mut parts = name.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u64::try_from(era * 146_097 + doe - 719_468).ok()
}

/// Agent calls in one encrypted file per UTC day, appended record by record.
pub struct History {
    dir: PathBuf,
    key: Mutex<Option<Zeroizing<Vec<u8>>>>,
}

impl History {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            key: Mutex::new(None),
        }
    }

    /// Loads the history key from the vault, making one the first time.
    pub fn unlock(&self, vault: &Vault) -> AppResult<()> {
        let mut key = self.key.lock().map_err(|_| AppError::general("History lock poisoned"))?;
        if key.is_some() {
            return Ok(());
        }

        let stored = match vault.get(KEY_ID)? {
            Some(stored) => Zeroizing::new(
                hex::decode(stored.as_str()).map_err(|_| AppError::vault("The agent history key is damaged"))?,
            ),
            None => {
                let mut fresh = Zeroizing::new(vec![0u8; KEY_LEN]);
                getrandom::fill(&mut fresh)
                    .map_err(|error| AppError::vault(format!("No secure random source: {error}")))?;
                vault.set(KEY_ID, &Zeroizing::new(hex::encode(fresh.as_slice())))?;
                fresh
            }
        };
        if stored.len() != KEY_LEN {
            return Err(AppError::vault("The agent history key has the wrong length"));
        }
        *key = Some(stored);
        Ok(())
    }

    /// Drops the key and every file, for a vault reset that makes the history unreadable.
    pub fn forget(&self) -> AppResult<()> {
        if let Ok(mut key) = self.key.lock() {
            *key = None;
        }
        self.clear()
    }

    fn cipher(&self) -> AppResult<XChaCha20Poly1305> {
        let key = self.key.lock().map_err(|_| AppError::general("History lock poisoned"))?;
        let key = key.as_ref().ok_or_else(|| AppError::vault("Agent history is locked"))?;
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| AppError::vault("Invalid history key"))
    }

    fn path(&self, day: &str) -> PathBuf {
        self.dir.join(format!("{day}.log"))
    }

    fn aad(day: &str) -> Vec<u8> {
        [MAGIC.as_slice(), day.as_bytes()].concat()
    }

    pub fn append(&self, entry: &HistoryEntry) -> AppResult<()> {
        let cipher = self.cipher()?;
        let day = day_name(entry.at / DAY_MS);
        let plaintext = Zeroizing::new(serde_json::to_vec(entry)?);

        let mut nonce = XNonce::default();
        getrandom::fill(&mut nonce).map_err(|error| AppError::vault(format!("No secure random source: {error}")))?;
        let aad = Self::aad(&day);
        let ciphertext = cipher
            .encrypt(&nonce, Payload { msg: &plaintext, aad: &aad })
            .map_err(|_| AppError::vault("Encrypting agent history failed"))?;

        let length = u32::try_from(ciphertext.len()).map_err(|_| AppError::general("History entry too large"))?;
        let mut record = Vec::with_capacity(4 + NONCE_LEN + ciphertext.len());
        record.extend_from_slice(&length.to_le_bytes());
        record.extend_from_slice(&nonce);
        record.extend_from_slice(&ciphertext);

        fs::create_dir_all(&self.dir)?;
        let mut file = fs::OpenOptions::new().create(true).append(true).open(self.path(&day))?;
        file.write_all(&record)?;
        Ok(())
    }

    fn read_day(&self, cipher: &XChaCha20Poly1305, day: &str) -> AppResult<Vec<HistoryEntry>> {
        let bytes = match fs::read(self.path(day)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let aad = Self::aad(day);

        let mut entries = Vec::new();
        let mut offset = 0;
        // A record cut short by a crash is the last one in the file, so reading stops there.
        while offset + 4 + NONCE_LEN <= bytes.len() {
            let length = u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
            let start = offset + 4 + NONCE_LEN;
            if start + length > bytes.len() {
                break;
            }
            let mut nonce = XNonce::default();
            nonce.copy_from_slice(&bytes[offset + 4..start]);
            let plaintext = Zeroizing::new(
                cipher
                    .decrypt(&nonce, Payload { msg: &bytes[start..start + length], aad: &aad })
                    .map_err(|_| AppError::vault(format!("Agent history for {day} can't be decrypted")))?,
            );
            entries.push(serde_json::from_slice(&plaintext)?);
            offset = start + length;
        }
        Ok(entries)
    }

    /// Entries between two times, newest first, without the replies.
    pub fn list(&self, from: u64, to: u64) -> AppResult<Vec<HistoryEntry>> {
        let cipher = self.cipher()?;
        let mut entries = Vec::new();
        for day in from / DAY_MS..=to / DAY_MS {
            entries.extend(
                self.read_day(&cipher, &day_name(day))?
                    .into_iter()
                    .filter(|entry| entry.at >= from && entry.at <= to)
                    .map(|entry| HistoryEntry { reply: None, ..entry }),
            );
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.at));
        Ok(entries)
    }

    pub fn get(&self, id: &str, at: u64) -> AppResult<Option<HistoryEntry>> {
        let cipher = self.cipher()?;
        Ok(self
            .read_day(&cipher, &day_name(at / DAY_MS))?
            .into_iter()
            .find(|entry| entry.id == id))
    }

    pub fn prune(&self, keep_days: u32) -> AppResult<()> {
        let today = now_ms() / DAY_MS;
        let oldest = today.saturating_sub(u64::from(keep_days).saturating_sub(1));
        let listing = match fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for item in listing.flatten() {
            let path = item.path();
            let day = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|_| path.extension().is_some_and(|extension| extension == "log"))
                .and_then(day_number);
            if day.is_some_and(|day| day < oldest) {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    pub fn size(&self) -> AppResult<u64> {
        let listing = match fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        Ok(listing
            .flatten()
            .filter_map(|item| item.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum())
    }

    pub fn clear(&self) -> AppResult<()> {
        match fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_days() {
        assert_eq!(day_name(0), "1970-01-01");
        assert_eq!(day_name(20_752), "2026-10-26");
        assert_eq!(day_name(11_016), "2000-02-29");
        for day in [0, 59, 60, 11_016, 20_752, 30_000] {
            assert_eq!(day_number(&day_name(day)), Some(day));
        }
    }

    fn entry(id: &str, at: u64) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            at,
            event: HistoryEvent::Call,
            agent: Some("claude-code".to_string()),
            session_id: None,
            connection_id: None,
            connection_name: None,
            database: None,
            tool: Some("run_query".to_string()),
            arguments: None,
            statements: vec!["SELECT 1".to_string()],
            approval: None,
            outcome: Outcome::Ok,
            message: None,
            duration_ms: 1.0,
            reply: Some(serde_json::json!({ "rows": [[1]] })),
        }
    }

    #[test]
    fn appends_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("fran-db-history-{}", uuid::Uuid::new_v4()));
        let history = History::new(dir.clone());
        *history.key.lock().unwrap() = Some(Zeroizing::new(vec![7u8; KEY_LEN]));

        let day = 20_752 * DAY_MS;
        history.append(&entry("a", day + 10)).unwrap();
        history.append(&entry("b", day + 20)).unwrap();
        history.append(&entry("c", day + DAY_MS + 5)).unwrap();

        let listed = history.list(day, day + DAY_MS * 2).unwrap();
        assert_eq!(listed.iter().map(|entry| entry.id.as_str()).collect::<Vec<_>>(), ["c", "b", "a"]);
        assert!(listed.iter().all(|entry| entry.reply.is_none()));
        assert!(history.get("b", day + 20).unwrap().unwrap().reply.is_some());

        let path = history.path(&day_name(20_752));
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(&[9, 0, 0]);
        fs::write(&path, &bytes).unwrap();
        assert_eq!(history.list(day, day + DAY_MS - 1).unwrap().len(), 2);

        *history.key.lock().unwrap() = Some(Zeroizing::new(vec![8u8; KEY_LEN]));
        assert!(history.list(day, day).is_err());

        history.clear().unwrap();
        assert!(!dir.exists());
    }
}
