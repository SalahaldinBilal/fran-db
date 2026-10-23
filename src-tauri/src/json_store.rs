use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::RwLock,
};

use serde::{Serialize, de::DeserializeOwned};

use crate::error::{AppError, AppResult};

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    let mut temp_name = path.as_os_str().to_owned();
    temp_name.push(".tmp");
    let temp_path = PathBuf::from(temp_name);

    fs::write(&temp_path, bytes)?;
    fs::rename(&temp_path, path)
}

/// A JSON file held in memory. A file that fails to parse is never overwritten,
/// every access reports the parse error instead.
pub struct JsonStore<T> {
    path: PathBuf,
    data: RwLock<T>,
    load_error: Option<String>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone> JsonStore<T> {
    pub fn load(path: PathBuf) -> Self {
        let (data, load_error) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(data) => (data, None),
                Err(error) => (
                    T::default(),
                    Some(format!("{} could not be read: {error}", path.display())),
                ),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => (T::default(), None),
            Err(error) => (
                T::default(),
                Some(format!("{} could not be opened: {error}", path.display())),
            ),
        };

        Self {
            path,
            data: RwLock::new(data),
            load_error,
        }
    }

    fn check_loaded(&self) -> AppResult<()> {
        match &self.load_error {
            Some(error) => Err(AppError::general(error.clone())),
            None => Ok(()),
        }
    }

    pub fn read<R>(&self, reader: impl FnOnce(&T) -> R) -> AppResult<R> {
        self.check_loaded()?;
        let data = self.data.read().map_err(|_| AppError::general("Store lock poisoned"))?;
        Ok(reader(&data))
    }

    pub fn update<R>(&self, updater: impl FnOnce(&mut T) -> AppResult<R>) -> AppResult<R> {
        self.check_loaded()?;
        let mut data = self.data.write().map_err(|_| AppError::general("Store lock poisoned"))?;
        let mut draft = data.clone();
        let result = updater(&mut draft)?;
        let bytes = serde_json::to_vec_pretty(&draft)?;
        write_atomic(&self.path, &bytes)?;
        *data = draft;
        Ok(result)
    }
}
