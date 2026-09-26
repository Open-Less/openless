//! Small framework-independent JSON persistence primitives.

use std::fs;
use std::io::{Read, Write};
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
    use crate::cloud_sync_e2ee_store::gate::{begin_unobserved_atomic, ChangeOrigin};
    let permit = begin_unobserved_atomic(path)?;
    let outcome = atomic_replace(path, contents);
    match outcome {
        Ok(changed) => {
            if changed {
                crate::cloud_sync_e2ee_store::gate::note_successful_write(path);
            }
            if let Some(permit) = permit {
                let result = if changed {
                    permit.commit(ChangeOrigin::User).map(|_| ())
                } else {
                    permit.abort_unmodified()
                };
                result.map_err(|_| persistence_error("record saved data generation"))?;
            }
            Ok(())
        }
        Err(error) => {
            if error.code != BackendErrorCode::OutcomeUnknown {
                if let Some(permit) = permit {
                    permit
                        .abort_unmodified()
                        .map_err(|_| persistence_error("clear unused mutation intent"))?;
                }
            }
            Err(error)
        }
    }
}

/// Restore hooks require proof of the same exclusive gate; they emit one final batch change.
pub(crate) fn atomic_write_for_sync(
    path: &Path,
    contents: &[u8],
    permit: &crate::cloud_sync_e2ee_store::gate::ExclusivePermit,
) -> Result<(), BackendError> {
    crate::cloud_sync_e2ee_store::gate::require_exclusive(path, permit)?;
    atomic_replace(path, contents).map(|_| ())
}

fn atomic_replace(path: &Path, contents: &[u8]) -> Result<bool, BackendError> {
    if path.as_os_str().is_empty() {
        return Err(persistence_error("empty JSON store path"));
    }
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        ensure_durable_file(path)?;
        return Ok(false);
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
    let write = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| persistence_error("create JSON store temporary file"))?;
        file.write_all(contents)
            .map_err(|_| persistence_error("write JSON store temporary file"))?;
        file.sync_all()
            .map_err(|_| persistence_error("flush JSON store temporary file"))
    })();
    if let Err(error) = write {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    #[cfg(test)]
    if take_test_fault(path, false) {
        let _ = fs::remove_file(&temporary);
        return Err(persistence_error("fixture pre-rename failure"));
    }
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(persistence_error("replace JSON store file"));
    }
    #[cfg(test)]
    if take_test_fault(path, true) {
        return Err(BackendError::new(
            BackendErrorCode::OutcomeUnknown,
            "fixture directory flush failure",
        ));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::OutcomeUnknown,
                    "JSON replacement outcome needs verification",
                )
            })?;
    }
    Ok(true)
}

/// A read-back of an uncertain rename is complete only after its data and directory flush.
pub(crate) fn ensure_durable_file(path: &Path) -> Result<(), BackendError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.write(true);
    options
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| {
            BackendError::new(
                BackendErrorCode::OutcomeUnknown,
                "stored data durability needs verification",
            )
        })?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| {
                BackendError::new(
                    BackendErrorCode::OutcomeUnknown,
                    "stored directory durability needs verification",
                )
            })?;
    }
    Ok(())
}

pub(crate) fn persistence_error(operation: &'static str) -> BackendError {
    BackendError::new(BackendErrorCode::Persistence, operation)
}

/// Sync must either preserve every stored field or report an unsupported schema.
pub(crate) fn read_lossless_rows<T: DeserializeOwned + serde::Serialize>(
    path: &Path,
    aliases: &[&str],
) -> Result<Vec<T>, BackendError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(persistence_error("read sync rows")),
    };
    let limit = crate::cloud_sync_e2ee_documents::MAX_JSON_BYTES;
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| persistence_error("read sync rows"))?;
    if bytes.len() > limit {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "payload_too_large",
        ));
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<serde_json::Value> =
        serde_json::from_slice(&bytes).map_err(|_| persistence_error("decode sync rows"))?;
    raw.into_iter()
        .map(|value| {
            let row: T = serde_json::from_value(value.clone())
                .map_err(|_| persistence_error("decode sync row"))?;
            let canonical =
                serde_json::to_value(&row).map_err(|_| persistence_error("encode sync row"))?;
            ensure_lossless_value(&value, &canonical, aliases)?;
            Ok(row)
        })
        .collect()
}

/// Every explicit source value must remain representable. Canonical defaults may be added;
/// aliases are allowed only at the root and must be explicitly registered by the caller.
pub(crate) fn ensure_lossless_value(
    raw: &serde_json::Value,
    canonical: &serde_json::Value,
    aliases: &[&str],
) -> Result<(), BackendError> {
    use serde_json::Value;
    match (raw, canonical) {
        (Value::Object(raw), Value::Object(canonical)) => {
            for (key, value) in raw {
                if aliases.contains(&key.as_str()) {
                    continue;
                }
                let next = canonical.get(key).ok_or_else(|| {
                    BackendError::new(BackendErrorCode::Unsupported, "unsupported stored fields")
                })?;
                ensure_lossless_value(value, next, &[])?;
            }
        }
        (Value::Array(raw), Value::Array(canonical)) if raw.len() == canonical.len() => {
            for (raw, canonical) in raw.iter().zip(canonical) {
                ensure_lossless_value(raw, canonical, &[])?;
            }
        }
        _ if raw == canonical => {}
        _ => {
            return Err(BackendError::new(
                BackendErrorCode::Unsupported,
                "stored value cannot be preserved",
            ))
        }
    }
    Ok(())
}

pub(crate) fn read_lossless_object<T: DeserializeOwned + serde::Serialize + Default>(
    path: &Path,
) -> Result<T, BackendError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(_) => return Err(persistence_error("read JSON store")),
    };
    if bytes.is_empty() {
        return Ok(T::default());
    }
    let raw: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| persistence_error("decode JSON store"))?;
    let value: T =
        serde_json::from_value(raw.clone()).map_err(|_| persistence_error("decode JSON store"))?;
    let canonical =
        serde_json::to_value(&value).map_err(|_| persistence_error("encode JSON store"))?;
    ensure_lossless_value(&raw, &canonical, &[])?;
    Ok(value)
}

#[cfg(test)]
static TEST_FAULTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, bool>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));
#[cfg(test)]
pub(crate) fn fail_next_atomic_write(path: &Path, after_rename: bool) {
    TEST_FAULTS
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), after_rename);
}
#[cfg(test)]
fn take_test_fault(path: &Path, after_rename: bool) -> bool {
    let mut faults = TEST_FAULTS.lock().unwrap();
    if faults.get(path) == Some(&after_rename) {
        faults.remove(path);
        true
    } else {
        false
    }
}
