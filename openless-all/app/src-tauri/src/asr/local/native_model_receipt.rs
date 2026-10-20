//! File-only receipts for SDK-managed models. Catalog/status queries must not
//! initialize the vendor SDK merely to ask whether a download already exists.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Receipt {
    model_dir: PathBuf,
    files: Vec<(PathBuf, u64)>,
}

fn receipt_path(root: &Path, alias: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        !alias.is_empty()
            && alias
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "invalid native model alias"
    );
    Ok(root
        .join(".openless-installed")
        .join(format!("{alias}.json")))
}

pub(super) fn record(root: &Path, alias: &str, model_dir: &Path) -> Result<()> {
    fn collect(dir: &Path, files: &mut Vec<(PathBuf, u64)>, depth: usize) -> Result<()> {
        anyhow::ensure!(
            depth < 32 && files.len() < 10000,
            "native model directory is too large"
        );
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name() == ".openless-installed" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                collect(&entry.path(), files, depth + 1)?;
            } else if kind.is_file() {
                files.push((entry.path(), entry.metadata()?.len()));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    if model_dir.is_file() {
        files.push((model_dir.to_path_buf(), std::fs::metadata(model_dir)?.len()));
    } else {
        collect(model_dir, &mut files, 0)?;
    }
    anyhow::ensure!(
        files.iter().any(|(_, size)| *size > 0),
        "native model has no downloaded files"
    );
    let path = receipt_path(root, alias)?;
    std::fs::create_dir_all(path.parent().context("receipt parent")?)?;
    // An interrupted write is treated as not installed, never as permission to load.
    std::fs::write(
        path,
        serde_json::to_vec(&Receipt {
            model_dir: model_dir.into(),
            files,
        })?,
    )?;
    Ok(())
}

pub(super) fn installed_dir(root: &Path, alias: &str) -> Option<PathBuf> {
    let receipt: Receipt =
        serde_json::from_slice(&std::fs::read(receipt_path(root, alias).ok()?).ok()?).ok()?;
    if receipt.files.is_empty()
        || !receipt.files.iter().all(|(path, size)| {
            std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == *size)
        })
    {
        return None;
    }
    Some(receipt.model_dir)
}

pub(super) fn forget(root: &Path, alias: &str) -> Result<()> {
    let path = receipt_path(root, alias)?;
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result.map_err(Into::into),
    }
}

/// Resolve even a missing model directory through its existing ancestors, so
/// incomplete caches can be removed without granting authority through symlinks.
fn managed_model_path(root: &Path, path: &Path) -> Result<PathBuf> {
    anyhow::ensure!(path.is_absolute(), "native model path must be absolute");
    let root = root.canonicalize()?;
    let mut ancestor = path.to_path_buf();
    let mut missing = Vec::new();
    let mut canonical = loop {
        match std::fs::symlink_metadata(&ancestor) {
            Ok(_) => break ancestor.canonicalize()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .context("invalid native model path")?
                        .to_owned(),
                );
                ancestor = ancestor
                    .parent()
                    .context("native model path has no parent")?
                    .into();
            }
            Err(error) => return Err(error.into()),
        }
    };
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    anyhow::ensure!(
        canonical != root && canonical.starts_with(&root),
        "refusing to delete a model outside its cache directory"
    );
    Ok(canonical)
}

/// Cold deletion is file-only; never create an SDK manager to remove a model.
pub(super) fn remove(root: &Path, alias: &str) -> Result<()> {
    let bytes = match std::fs::read(receipt_path(root, alias)?) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    // Loading requires complete files; deletion only requires a safely owned
    // path. Keep the receipt on parse/path/delete failure so cleanup is retryable.
    let receipt: Receipt =
        serde_json::from_slice(&bytes).context("invalid native model receipt")?;
    let path = managed_model_path(root, &receipt.model_dir)?;
    let removed = match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    forget(root, alias)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_checks_files_without_initializing_the_sdk() {
        let root =
            std::env::temp_dir().join(format!("openless-native-receipt-{}", uuid::Uuid::new_v4()));
        let model = root.join("cached/model");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("weights.onnx"), b"weights").unwrap();
        assert!(installed_dir(&root, "whisper-small").is_none());
        record(&root, "whisper-small", &model).unwrap();
        assert_eq!(installed_dir(&root, "whisper-small"), Some(model.clone()));
        remove(&root, "whisper-small").unwrap();
        assert!(!model.exists());
        assert!(installed_dir(&root, "whisper-small").is_none());
        assert!(record(&root, "../unsafe", &model).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("openless-native-receipt-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_receipt(root: &Path, model_dir: &Path) -> PathBuf {
        let path = receipt_path(root, "whisper-small").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&Receipt {
                model_dir: model_dir.into(),
                files: vec![(model_dir.join("missing.onnx"), 42)],
            })
            .unwrap(),
        )
        .unwrap();
        path
    }

    #[test]
    fn cold_delete_removes_missing_or_truncated_models_not_just_the_receipt() {
        for missing in [true, false] {
            let temp = Temp::new();
            let model = temp.0.join("cached/model");
            std::fs::create_dir_all(&model).unwrap();
            std::fs::write(model.join("weights.onnx"), b"large weights").unwrap();
            let decoder = model.join("decoder.onnx");
            std::fs::write(&decoder, b"decoder weights").unwrap();
            record(&temp.0, "whisper-small", &model).unwrap();
            if missing {
                std::fs::remove_file(decoder).unwrap();
            } else {
                std::fs::write(decoder, b"short").unwrap();
            }
            assert!(installed_dir(&temp.0, "whisper-small").is_none());
            remove(&temp.0, "whisper-small").unwrap();
            assert!(
                !model.exists(),
                "incomplete weights must not remain on disk"
            );
            assert!(!receipt_path(&temp.0, "whisper-small").unwrap().exists());
        }
    }

    #[test]
    fn cold_delete_forgets_a_directory_already_missing_with_its_parent() {
        let temp = Temp::new();
        let receipt = write_receipt(&temp.0, &temp.0.join("gone/model"));
        remove(&temp.0, "whisper-small").unwrap();
        assert!(!receipt.exists());
    }

    #[test]
    fn cold_delete_rejects_cache_root_and_outside_paths_and_keeps_the_receipt() {
        let temp = Temp::new();
        let root = temp.0.join("cache");
        let outside = temp.0.join("other-app");
        std::fs::create_dir_all(&outside).unwrap();
        let preserved = outside.join("weights.onnx");
        std::fs::write(&preserved, b"keep").unwrap();
        for path in [root.clone(), outside.clone(), outside.join("gone/model")] {
            let receipt = write_receipt(&root, &path);
            assert!(remove(&root, "whisper-small").is_err());
            assert!(receipt.is_file(), "failed cleanup must remain retryable");
            assert!(preserved.is_file());
        }
    }

    #[cfg(unix)]
    #[test]
    fn cold_delete_rejects_a_symlink_outside_the_cache_even_if_files_are_missing() {
        let temp = Temp::new();
        let root = temp.0.join("cache");
        let outside = temp.0.join("other-app");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let preserved = outside.join("weights.onnx");
        std::fs::write(&preserved, b"keep").unwrap();
        let linked = root.join("linked-model");
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        let receipt = write_receipt(&root, &linked);
        assert!(remove(&root, "whisper-small").is_err());
        assert!(receipt.is_file());
        assert!(preserved.is_file());
    }

    #[test]
    fn cold_delete_reports_a_malformed_receipt_without_forgetting_it() {
        let temp = Temp::new();
        let receipt = write_receipt(&temp.0, &temp.0.join("model"));
        std::fs::write(&receipt, b"{interrupted").unwrap();
        assert!(remove(&temp.0, "whisper-small").is_err());
        assert!(receipt.is_file());
    }

}
