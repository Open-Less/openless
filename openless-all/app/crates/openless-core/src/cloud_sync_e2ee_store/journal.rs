use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::cloud_sync_e2ee_documents::{
    DocumentError, DocumentResult, JournalStore, SealedJournal, SyncScope, MAX_JOURNAL_BYTES,
};

use super::{state::scope_id, SyncWriteGate};

const MAGIC: &[u8; 8] = b"OLSJRNL1";

pub(super) struct FileJournalStore {
    directory: PathBuf,
    gate: Arc<SyncWriteGate>,
}
impl FileJournalStore {
    pub fn new(directory: PathBuf, gate: Arc<SyncWriteGate>) -> Self {
        Self { directory, gate }
    }
    fn path(&self, scope: &SyncScope) -> DocumentResult<PathBuf> {
        Ok(self
            .directory
            .join(format!("restore-{}.bin", scope_id(scope)?)))
    }
}

fn sync_directory(path: &std::path::Path) -> DocumentResult<()> {
    #[cfg(unix)]
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| DocumentError::JournalUnavailable)?;
    Ok(())
}

impl JournalStore for FileJournalStore {
    fn load(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<Option<SealedJournal>>> {
        Box::pin(async move {
            let path = self.path(&scope)?;
            tokio::task::spawn_blocking(move || {
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(DocumentError::RecoveryRequired)
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(_) => return Err(DocumentError::JournalUnavailable),
                    _ => {}
                }
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .map_err(|_| DocumentError::JournalUnavailable)?
                    .take((MAX_JOURNAL_BYTES + 129 + 44) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| DocumentError::JournalUnavailable)?;
                if bytes.len() < 44
                    || bytes.len() > MAX_JOURNAL_BYTES + 128 + 44
                    || !bytes.starts_with(MAGIC)
                {
                    return Err(DocumentError::RecoveryRequired);
                }
                let operation_id = std::str::from_utf8(&bytes[8..44])
                    .map_err(|_| DocumentError::RecoveryRequired)?
                    .to_string();
                let id = uuid::Uuid::parse_str(&operation_id)
                    .map_err(|_| DocumentError::RecoveryRequired)?;
                if id.get_version() != Some(uuid::Version::Random) || id.to_string() != operation_id
                {
                    return Err(DocumentError::RecoveryRequired);
                }
                Ok(Some(SealedJournal {
                    operation_id,
                    ciphertext: bytes[44..].to_vec(),
                }))
            })
            .await
            .map_err(|_| DocumentError::JournalUnavailable)?
        })
    }

    fn save(&self, scope: SyncScope, journal: SealedJournal) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            let path = self.path(&scope)?;
            let guard = self.gate.clone_exclusive_for_metadata()?;
            if journal.operation_id.len() != 36
                || journal.ciphertext.is_empty()
                || journal.ciphertext.len() > MAX_JOURNAL_BYTES + 128
            {
                return Err(DocumentError::JournalUnavailable);
            }
            tokio::task::spawn_blocking(move || {
                // This clone keeps ordinary writes blocked even if the waiting future is dropped.
                let _guard = guard;
                let mut bytes = Vec::with_capacity(44 + journal.ciphertext.len());
                bytes.extend_from_slice(MAGIC);
                bytes.extend_from_slice(journal.operation_id.as_bytes());
                bytes.extend_from_slice(&journal.ciphertext);
                crate::persistence::atomic_write(&path, &bytes)
                    .map_err(|_| DocumentError::JournalUnavailable)
            })
            .await
            .map_err(|_| DocumentError::JournalUnavailable)?
        })
    }

    fn clear(&self, scope: SyncScope) -> BoxFuture<'_, DocumentResult<()>> {
        Box::pin(async move {
            let path = self.path(&scope)?;
            let scope_id = scope_id(&scope)?;
            let guard = self.gate.clone_exclusive_for_metadata()?;
            tokio::task::spawn_blocking(move || {
                let operation = std::fs::File::open(&path).ok().and_then(|file| {
                    let mut bytes = Vec::new();
                    file.take(44).read_to_end(&mut bytes).ok()?;
                    bytes
                        .get(8..44)
                        .and_then(|bytes| std::str::from_utf8(bytes).ok())
                        .map(str::to_owned)
                });
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(_) => return Err(DocumentError::JournalUnavailable),
                }
                sync_directory(path.parent().ok_or(DocumentError::JournalUnavailable)?)?;
                if let Some(operation) = operation {
                    guard.forget_completed_restore(&operation, &scope_id)?;
                }
                Ok(())
            })
            .await
            .map_err(|_| DocumentError::JournalUnavailable)?
        })
    }
}
