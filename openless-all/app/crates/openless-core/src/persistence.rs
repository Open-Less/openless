//! Small framework-independent JSON persistence primitives.

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::errors::{BackendError, BackendErrorCode};

pub(crate) fn read_or_default<T: DeserializeOwned + Default>(
    path: &Path,
) -> Result<T, BackendError> {
    if path.as_os_str().is_empty() || !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).map_err(|_| persistence_error("read JSON store"))?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(&bytes).map_err(|_| persistence_error("decode JSON store"))
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), BackendError> {
    if path.as_os_str().is_empty() {
        return Err(persistence_error("empty JSON store path"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| persistence_error("create JSON store directory"))?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary =
        path.with_file_name(format!("{file_name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, contents)
        .map_err(|_| persistence_error("write JSON store temporary file"))?;
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(persistence_error("replace JSON store file"));
    }
    Ok(())
}

pub(crate) fn persistence_error(operation: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::Persistence, operation)
}
