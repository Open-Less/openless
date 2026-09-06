use std::io::Write;
use std::path::Path;

use openless_core::{BackendError, BackendErrorCode};

pub(crate) fn write_archive(target: &Path, bytes: &[u8]) -> Result<(), BackendError> {
    if !target.is_absolute() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Linux Marketplace archive target must be an absolute filesystem path",
        ));
    }
    let parent = target.parent().ok_or_else(|| {
        BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Linux Marketplace archive target has no parent directory",
        )
    })?;
    if !parent.is_dir() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Linux Marketplace archive parent directory does not exist",
        ));
    }
    if target.file_name().is_none() {
        return Err(BackendError::new(
            BackendErrorCode::InvalidArgument,
            "Linux Marketplace archive target has no file name",
        ));
    }

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| archive_error("create", error))?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(target);
        return Err(archive_error("write", error));
    }
    Ok(())
}

fn archive_error(operation: &str, error: std::io::Error) -> BackendError {
    BackendError::new(
        BackendErrorCode::Persistence,
        format!("failed to {operation} Linux Marketplace archive: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_archive_sink_preserves_bytes_and_refuses_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-marketplace-archive-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("downloaded.zip");

        write_archive(&target, b"validated core archive").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"validated core archive");

        let error = write_archive(&target, b"replacement")
            .expect_err("the host sink must not overwrite an existing user file");
        assert_eq!(error.code, BackendErrorCode::Persistence);
        assert_eq!(std::fs::read(&target).unwrap(), b"validated core archive");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn filesystem_archive_sink_rejects_relative_or_missing_parent_paths() {
        let relative = write_archive(Path::new("downloaded.zip"), b"bytes")
            .expect_err("relative paths must not be interpreted against process cwd");
        assert_eq!(relative.code, BackendErrorCode::InvalidArgument);

        let root = std::env::temp_dir().join(format!(
            "openless-linux-marketplace-missing-parent-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let missing_parent = write_archive(&root.join("missing").join("downloaded.zip"), b"bytes")
            .expect_err("the UI must select an existing destination directory");
        assert_eq!(missing_parent.code, BackendErrorCode::InvalidArgument);
    }
}
