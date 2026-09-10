//! File staging and compensation for a cloud restore while all participating stores are locked.

use crate::{BackendError, BackendErrorCode};
use std::{fs, io::Write, path::PathBuf};

struct StagedFile {
    target: PathBuf,
    replacement: PathBuf,
    backup: Option<PathBuf>,
    replacement_created: bool,
    backup_created: bool,
}

pub(crate) struct RestoreTransaction {
    files: Vec<StagedFile>,
    preserve_backups: bool,
}

impl RestoreTransaction {
    pub(crate) fn prepare(writes: Vec<(PathBuf, Vec<u8>)>) -> Result<Self, BackendError> {
        let mut transaction = Self {
            files: Vec::new(),
            preserve_backups: false,
        };
        for (target, bytes) in writes {
            let parent = target
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .ok_or_else(|| failure("cloud restore store path is unavailable"))?;
            fs::create_dir_all(parent).map_err(|_| failure("prepare cloud restore directory"))?;
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let replacement = parent.join(format!(".cloud-sync-{suffix}.next"));
            let previous = match fs::read(&target) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(failure("read local file before cloud restore")),
            };
            let backup = previous
                .as_ref()
                .map(|_| parent.join(format!(".cloud-sync-{suffix}.backup")));
            transaction.files.push(StagedFile {
                target,
                replacement: replacement.clone(),
                backup: backup.clone(),
                replacement_created: false,
                backup_created: false,
            });
            let staged = transaction
                .files
                .last_mut()
                .expect("just added a staged file");
            if let (Some(backup), Some(previous)) = (backup, previous) {
                write_private_file(&backup, &previous)?;
                staged.backup_created = true;
            }
            write_private_file(&replacement, &bytes)?;
            staged.replacement_created = true;
        }
        Ok(transaction)
    }

    pub(crate) fn commit(mut self) -> Result<(), BackendError> {
        for index in 0..self.files.len() {
            if fs::rename(&self.files[index].replacement, &self.files[index].target).is_err() {
                let mut restored = true;
                for file in self.files[..index].iter_mut().rev() {
                    let result = match &file.backup {
                        Some(backup) => fs::rename(backup, &file.target),
                        None => fs::remove_file(&file.target),
                    };
                    if result.is_err() {
                        restored = false;
                    }
                }
                if !restored {
                    // Keep recovery copies if compensation fails. Never report a partial restore as success.
                    self.preserve_backups = true;
                    return Err(BackendError {
                        code: BackendErrorCode::OutcomeUnknown,
                        message: "云端恢复失败，部分本地文件未能回滚；请保留数据目录中的 .cloud-sync-*.backup 备份并检查本地数据。".into(),
                        retryable: false,
                        details: Some(serde_json::json!({"recoveryRequired": true})),
                    });
                }
                return Err(failure("云端恢复失败，本地数据已回滚。"));
            }
        }
        Ok(())
    }
}

impl Drop for RestoreTransaction {
    fn drop(&mut self) {
        for file in &self.files {
            if file.replacement_created {
                let _ = fs::remove_file(&file.replacement);
            }
            if file.backup_created && !self.preserve_backups {
                if let Some(backup) = &file.backup {
                    let _ = fs::remove_file(backup);
                }
            }
        }
    }
}

fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), BackendError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| failure("stage cloud restore file"))?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(failure("flush cloud restore file"));
    }
    Ok(())
}

fn failure(message: &str) -> BackendError {
    BackendError::new(BackendErrorCode::Persistence, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("openless-sync-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn failure_after_first_replacement_restores_previous_files() {
        let root = directory();
        let first = root.join("first.json");
        let second = root.join("second.json");
        fs::write(&first, b"old first").unwrap();
        fs::write(&second, b"old second").unwrap();
        let transaction = RestoreTransaction::prepare(vec![
            (first.clone(), b"new first".to_vec()),
            (second.clone(), b"new second".to_vec()),
        ])
        .unwrap();
        fs::remove_file(&transaction.files[1].replacement).unwrap();
        assert_eq!(
            transaction.commit().unwrap_err().code,
            BackendErrorCode::Persistence
        );
        assert_eq!(fs::read(first).unwrap(), b"old first");
        assert_eq!(fs::read(second).unwrap(), b"old second");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_compensation_preserves_recovery_files_and_reports_unknown_outcome() {
        let root = directory();
        let first = root.join("first.json");
        let second = root.join("second.json");
        fs::write(&first, b"old first").unwrap();
        fs::write(&second, b"old second").unwrap();
        let transaction = RestoreTransaction::prepare(vec![
            (first, b"new first".to_vec()),
            (second, b"new second".to_vec()),
        ])
        .unwrap();
        fs::remove_file(transaction.files[0].backup.as_ref().unwrap()).unwrap();
        fs::remove_file(&transaction.files[1].replacement).unwrap();
        let surviving_backup = transaction.files[1].backup.clone().unwrap();
        let error = transaction.commit().unwrap_err();
        assert_eq!(error.code, BackendErrorCode::OutcomeUnknown);
        assert_eq!(error.details.unwrap()["recoveryRequired"], true);
        assert_eq!(fs::read(surviving_backup).unwrap(), b"old second");
        fs::remove_dir_all(root).unwrap();
    }
}
