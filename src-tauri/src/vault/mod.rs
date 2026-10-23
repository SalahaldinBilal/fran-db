pub mod commands;

use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::PathBuf,
    sync::Mutex,
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AppError, AppResult};
use crate::json_store::write_atomic;

const KEYRING_SERVICE: &str = "com.frandb.app";
const KEYRING_ACCOUNT: &str = "vault-master-key";
const MAGIC: &[u8; 8] = b"FRDBVLT1";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub available: bool,
    pub message: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Secrets {
    version: u32,
    entries: BTreeMap<String, String>,
}

impl Drop for Secrets {
    fn drop(&mut self) {
        for value in self.entries.values_mut() {
            value.zeroize();
        }
    }
}

struct Unlocked {
    key: Zeroizing<Vec<u8>>,
    secrets: Secrets,
}

/// Secrets encrypted with XChaCha20-Poly1305 in one file. The key never touches
/// disk, it lives in the OS keychain (Credential Manager, Keychain, Secret Service).
pub struct Vault {
    path: PathBuf,
    state: Mutex<Option<Unlocked>>,
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(None),
        }
    }

    pub fn status(&self) -> VaultStatus {
        match self.with_unlocked(|_| Ok(())) {
            Ok(()) => VaultStatus {
                available: true,
                message: None,
            },
            Err(error) => VaultStatus {
                available: false,
                message: Some(error.message),
            },
        }
    }

    pub fn get(&self, id: &str) -> AppResult<Option<Zeroizing<String>>> {
        self.with_unlocked(|unlocked| {
            Ok(unlocked
                .secrets
                .entries
                .get(id)
                .map(|value| Zeroizing::new(value.clone())))
        })
    }

    pub fn ids_with_prefix(&self, prefix: &str) -> AppResult<Vec<String>> {
        self.with_unlocked(|unlocked| {
            Ok(unlocked
                .secrets
                .entries
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect())
        })
    }

    pub fn set(&self, id: &str, value: &str) -> AppResult<()> {
        self.modify(|secrets| {
            if let Some(mut old) = secrets.entries.insert(id.to_string(), value.to_string()) {
                old.zeroize();
            }
        })
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        self.modify(|secrets| {
            if let Some(mut old) = secrets.entries.remove(id) {
                old.zeroize();
            }
        })
    }

    pub fn remove_prefix(&self, prefix: &str) -> AppResult<()> {
        self.modify(|secrets| {
            let ids: Vec<String> = secrets
                .entries
                .keys()
                .filter(|id| id.starts_with(prefix))
                .cloned()
                .collect();

            for id in ids {
                if let Some(mut old) = secrets.entries.remove(&id) {
                    old.zeroize();
                }
            }
        })
    }

    pub fn copy_prefix(&self, from_prefix: &str, to_prefix: &str) -> AppResult<()> {
        self.modify(|secrets| {
            let copies: Vec<(String, String)> = secrets
                .entries
                .iter()
                .filter_map(|(id, value)| {
                    id.strip_prefix(from_prefix)
                        .map(|rest| (format!("{to_prefix}{rest}"), value.clone()))
                })
                .collect();

            secrets.entries.extend(copies);
        })
    }

    /// Wipes every stored secret and the keychain key. Only for a vault that can no longer be opened.
    pub fn reset(&self) -> AppResult<()> {
        let mut state = self.lock_state()?;
        *state = None;

        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        let entry = keychain_entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keychain_error(error)),
        }
    }

    fn lock_state(&self) -> AppResult<std::sync::MutexGuard<'_, Option<Unlocked>>> {
        self.state
            .lock()
            .map_err(|_| AppError::vault("Vault lock poisoned"))
    }

    fn with_unlocked<R>(&self, action: impl FnOnce(&mut Unlocked) -> AppResult<R>) -> AppResult<R> {
        let mut state = self.lock_state()?;

        if state.is_none() {
            *state = Some(self.unlock()?);
        }

        match state.as_mut() {
            Some(unlocked) => action(unlocked),
            None => Err(AppError::vault("Vault is locked")),
        }
    }

    fn modify(&self, change: impl FnOnce(&mut Secrets)) -> AppResult<()> {
        let path = self.path.clone();
        self.with_unlocked(|unlocked| {
            let backup = unlocked.secrets.entries.clone();
            change(&mut unlocked.secrets);

            let result = persist(&path, &unlocked.key, &unlocked.secrets);
            if result.is_err() {
                for value in unlocked.secrets.entries.values_mut() {
                    value.zeroize();
                }
                unlocked.secrets.entries = backup;
            } else {
                let mut backup = backup;
                for value in backup.values_mut() {
                    value.zeroize();
                }
            }
            result
        })
    }

    fn unlock(&self) -> AppResult<Unlocked> {
        let file = match fs::read(&self.path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::vault(format!("Vault file could not be read: {error}"))),
        };

        let key = load_or_create_key(file.is_some())?;

        let secrets = match file {
            Some(bytes) => decrypt(&key, &bytes)?,
            None => Secrets {
                version: 1,
                entries: BTreeMap::new(),
            },
        };

        Ok(Unlocked { key, secrets })
    }
}

fn keychain_entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(keychain_error)
}

fn keychain_error(error: keyring::Error) -> AppError {
    AppError::vault(format!("OS keychain unavailable: {error}"))
}

fn load_or_create_key(vault_exists: bool) -> AppResult<Zeroizing<Vec<u8>>> {
    let entry = keychain_entry()?;

    match entry.get_secret() {
        Ok(bytes) => {
            let key = Zeroizing::new(bytes);
            if key.len() != KEY_LEN {
                return Err(AppError::vault("The vault key in the OS keychain has the wrong length"));
            }
            Ok(key)
        }
        Err(keyring::Error::NoEntry) if !vault_exists => {
            let mut key = Zeroizing::new(vec![0u8; KEY_LEN]);
            getrandom::fill(&mut key)
                .map_err(|error| AppError::vault(format!("No secure random source: {error}")))?;
            entry.set_secret(&key).map_err(keychain_error)?;
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => Err(AppError::vault(
            "The vault key is missing from the OS keychain, so saved passwords can't be read. Reset the vault to start over.",
        )),
        Err(error) => Err(keychain_error(error)),
    }
}

fn cipher(key: &[u8]) -> AppResult<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| AppError::vault("Invalid vault key"))
}

fn decrypt(key: &[u8], bytes: &[u8]) -> AppResult<Secrets> {
    let header_len = MAGIC.len() + NONCE_LEN;
    if bytes.len() < header_len || &bytes[..MAGIC.len()] != MAGIC {
        return Err(AppError::vault("The vault file is not in a known format"));
    }

    let mut nonce = XNonce::default();
    nonce.copy_from_slice(&bytes[MAGIC.len()..header_len]);

    let plaintext = Zeroizing::new(
        cipher(key)?
            .decrypt(
                &nonce,
                Payload {
                    msg: &bytes[header_len..],
                    aad: MAGIC,
                },
            )
            .map_err(|_| {
                AppError::vault("The vault file can't be decrypted with the key in the OS keychain")
            })?,
    );

    serde_json::from_slice(&plaintext)
        .map_err(|error| AppError::vault(format!("The vault contents are damaged: {error}")))
}

fn persist(path: &PathBuf, key: &[u8], secrets: &Secrets) -> AppResult<()> {
    let plaintext = Zeroizing::new(serde_json::to_vec(secrets)?);

    let mut nonce = XNonce::default();
    getrandom::fill(&mut nonce)
        .map_err(|error| AppError::vault(format!("No secure random source: {error}")))?;

    let ciphertext = cipher(key)?
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: MAGIC,
            },
        )
        .map_err(|_| AppError::vault("Encrypting the vault failed"))?;

    let mut file = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&nonce);
    file.extend_from_slice(&ciphertext);

    write_atomic(path, &file)
        .map_err(|error| AppError::vault(format!("Vault file could not be written: {error}")))
}
