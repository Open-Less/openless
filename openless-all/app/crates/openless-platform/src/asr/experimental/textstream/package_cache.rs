//! In-process verification memo. A new process or any file identity/metadata
//! change requires full pinned-byte verification again. Never persist trust.
use openless_core::{experimental_asr::ExperimentalAsrModel, BackendError, BackendErrorCode};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    directory: PathBuf,
    // dev, inode, size, modification and change time (nanoseconds included).
    files: Vec<FileStamp>,
}

#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn stamp(directory: &Path, paths: &[String]) -> Result<Stamp, BackendError> {
    let directory = directory.canonicalize().map_err(failure)?;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let meta = std::fs::metadata(directory.join(path)).map_err(failure)?;
        if !meta.is_file() {
            return Err(failure("model package contains a non-file"));
        }
        files.push(FileStamp {
            device: meta.dev(),
            inode: meta.ino(),
            bytes: meta.len(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        });
    }
    Ok(Stamp { directory, files })
}
fn failure(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Provider, error.to_string())
}

#[derive(Default)]
pub(super) struct VerificationCache(Mutex<Option<Stamp>>);
impl VerificationCache {
    /// Only call from the blocking preflight worker. Returns true for a cache hit.
    pub(super) fn verify(
        &self,
        model: ExperimentalAsrModel,
        directory: &Path,
    ) -> Result<bool, BackendError> {
        let paths = model
            .files()
            .into_iter()
            .map(|f| f.path)
            .collect::<Vec<_>>();
        self.verify_with(directory, &paths, || {
            openless_core::experimental_asr::verify_package(model, directory)
        })
    }
    fn verify_with(
        &self,
        directory: &Path,
        paths: &[String],
        verify: impl FnOnce() -> Result<(), BackendError>,
    ) -> Result<bool, BackendError> {
        let before = stamp(directory, paths)?;
        let mut cached = self.0.lock().map_err(failure)?;
        if cached.as_ref() == Some(&before) {
            return Ok(true);
        }
        *cached = None;
        verify()?;
        let after = stamp(directory, paths)?;
        if before != after {
            return Err(failure("model package changed during verification"));
        }
        *cached = Some(after);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_package_is_cached_but_same_size_replacement_and_deletion_are_not() {
        let root =
            std::env::temp_dir().join(format!("openless-package-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let model = root.join("model");
        let paths = vec!["model".to_owned()];
        std::fs::write(&model, b"good").unwrap();
        let cache = VerificationCache::default();
        assert!(!cache.verify_with(&root, &paths, || Ok(())).unwrap());
        assert!(cache
            .verify_with(&root, &paths, || panic!("unchanged package rehashed"))
            .unwrap());
        std::fs::write(root.join("replacement"), b"evil").unwrap();
        std::fs::rename(root.join("replacement"), &model).unwrap();
        assert!(cache
            .verify_with(&root, &paths, || Err(failure("checksum")))
            .is_err());
        assert!(cache.0.lock().unwrap().is_none());
        std::fs::remove_file(&model).unwrap();
        assert!(cache
            .verify_with(&root, &paths, || panic!("missing model was trusted"))
            .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires existing pinned TextStream weights; never downloads"]
    fn existing_textstream_package_is_hashed_once_per_unchanged_process_cache() {
        let root = PathBuf::from(
            std::env::var_os("OPENLESS_TEXTSTREAM_MODEL_DIR").expect("model directory"),
        );
        let cache = VerificationCache::default();
        let start = std::time::Instant::now();
        assert!(!cache
            .verify(ExperimentalAsrModel::TextStream, &root)
            .unwrap());
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        assert!(cache
            .verify(ExperimentalAsrModel::TextStream, &root)
            .unwrap());
        println!(
            "TextStream pinned preflight cold={cold:?} warm={:?}",
            start.elapsed()
        );
    }

    #[test]
    fn concurrent_package_mutation_is_never_memoized() {
        let root =
            std::env::temp_dir().join(format!("openless-package-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("model"), b"good").unwrap();
        let cache = VerificationCache::default();
        let result = cache.verify_with(&root, &["model".to_owned()], || {
            std::fs::write(root.join("model"), b"different").unwrap();
            Ok(())
        });
        assert!(result.is_err());
        assert!(cache.0.lock().unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
