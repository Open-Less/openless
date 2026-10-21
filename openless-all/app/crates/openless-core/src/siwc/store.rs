//! Local-only, versioned secret storage. Hosts supply atomic head replacement.
use super::{error, SiwcError};
use crate::SecretValue;
use std::sync::Arc;

/// Never expose these operations through the general credential/settings IPC.
/// compare_head must compare and replace atomically across host processes.
pub trait SiwcVault: Send + Sync {
    fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError>;
    fn write(&self, key: &str, value: &str) -> Result<(), SiwcError>;
    fn remove(&self, key: &str) -> Result<(), SiwcError>;
    fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError>;
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Head {
    generation: String,
    chunks: usize,
}

pub(super) struct Snapshot {
    pub head: Option<String>,
    pub data: super::Data,
}

pub(super) struct Store(pub Arc<dyn SiwcVault>);
impl Store {
    pub fn load(&self) -> Result<Snapshot, SiwcError> {
        for _ in 0..4 {
            let head = self.0.read("head")?.map(SecretValue::into_exposed);
            let Some(raw) = head.as_deref() else {
                return Ok(Snapshot {
                    head,
                    data: super::Data::default(),
                });
            };
            let manifest: Head = serde_json::from_str(raw).map_err(|_| error("storage_corrupt"))?;
            if uuid::Uuid::parse_str(&manifest.generation).is_err()
                || manifest.chunks == 0
                || manifest.chunks > 4096
            {
                return Err(error("storage_corrupt"));
            }
            let mut payload = zeroize::Zeroizing::new(String::new());
            let mut missing = false;
            for index in 0..manifest.chunks {
                match self.0.read(&format!("{}.{}", manifest.generation, index))? {
                    Some(part) => payload.push_str(part.expose_secret()),
                    None => {
                        missing = true;
                        break;
                    }
                }
            }
            if self
                .0
                .read("head")?
                .as_ref()
                .map(SecretValue::expose_secret)
                != Some(raw)
            {
                continue;
            }
            if missing {
                return Err(error("storage_corrupt"));
            }
            let data = serde_json::from_str(&payload).map_err(|_| error("storage_corrupt"))?;
            return Ok(Snapshot { head, data });
        }
        Err(error("busy"))
    }

    pub fn commit(&self, previous: &Snapshot, data: &super::Data) -> Result<bool, SiwcError> {
        let payload = zeroize::Zeroizing::new(
            serde_json::to_string(data).map_err(|_| error("storage_failed"))?,
        );
        if payload.len() > 3_600_000 {
            return Err(error("storage_full"));
        }
        let generation = uuid::Uuid::new_v4().to_string();
        // Asset Store caps a secret at 1024 bytes; Windows also counts UTF-16 units.
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < payload.len() {
            let mut end = (start + 900).min(payload.len());
            while !payload.is_char_boundary(end) {
                end -= 1;
            }
            chunks.push(&payload[start..end]);
            start = end;
        }
        let head = serde_json::to_string(&Head {
            generation: generation.clone(),
            chunks: chunks.len(),
        })
        .map_err(|_| error("storage_failed"))?;
        let prepare = (|| {
            for (index, part) in chunks.iter().enumerate() {
                let key = format!("{generation}.{index}");
                self.0.write(&key, part)?;
                if self.0.read(&key)?.as_ref().map(SecretValue::expose_secret) != Some(*part) {
                    return Err(error("storage_failed"));
                }
            }
            self.0.compare_head(previous.head.as_deref(), &head)
        })();
        // A native error can occur after publication. Read back before deleting anything.
        let committed = match prepare {
            Ok(value) => value,
            Err(failure) => match self.0.read("head") {
                Ok(Some(current)) if current.expose_secret() == head => true,
                Ok(_) => {
                    for index in 0..chunks.len() {
                        let _ = self.0.remove(&format!("{generation}.{index}"));
                    }
                    return Err(failure);
                }
                Err(_) => return Err(failure),
            },
        };
        if committed {
            if let Some(old) = previous
                .head
                .as_deref()
                .and_then(|s| serde_json::from_str::<Head>(s).ok())
            {
                for index in 0..old.chunks {
                    let _ = self.0.remove(&format!("{}.{}", old.generation, index));
                }
            }
        } else {
            for index in 0..chunks.len() {
                let _ = self.0.remove(&format!("{generation}.{index}"));
            }
        }
        Ok(committed)
    }
}

/// Used by file-backed native hosts. The lock contains no credentials.
pub fn locked_head<T>(
    path: &std::path::Path,
    action: impl FnOnce() -> Result<T, SiwcError>,
) -> Result<T, SiwcError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| error("storage_failed"))?;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| error("storage_failed"))?;
    file.lock().map_err(|_| error("storage_failed"))?;
    action()
}
