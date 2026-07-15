use std::io::{Read, Write};
use std::path::Path;

const MAX_PIN_FILE_BYTES: u64 = 64;

pub(super) fn load_or_create_pin_at_path(
    path: &Path,
    generate_pin: impl FnOnce() -> String,
) -> std::io::Result<String> {
    if let Some(pin) = read_valid_persisted_pin(path)? {
        return Ok(pin);
    }

    let pin = generate_pin();
    persist_pin_atomically(path, &pin)?;
    Ok(pin)
}

fn read_valid_persisted_pin(path: &Path) -> std::io::Result<Option<String>> {
    let file = match open_pin_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    read_valid_pin_from_file(file)
}

#[cfg(unix)]
fn open_pin_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
}

#[cfg(target_os = "windows")]
fn open_pin_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn open_pin_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

fn read_valid_pin_from_file(mut file: std::fs::File) -> std::io::Result<Option<String>> {
    validate_and_repair_pin_file(&file)?;
    let mut contents = Vec::with_capacity(MAX_PIN_FILE_BYTES as usize + 1);
    std::io::Read::by_ref(&mut file)
        .take(MAX_PIN_FILE_BYTES + 1)
        .read_to_end(&mut contents)?;
    if contents.len() as u64 > MAX_PIN_FILE_BYTES {
        return Err(invalid_pin_file("pairing PIN file exceeds size limit"));
    }
    let Ok(contents) = std::str::from_utf8(&contents) else {
        return Ok(None);
    };
    let pin = contents.trim();
    if pin.len() != 6 || !pin.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    Ok(Some(pin.to_string()))
}

#[cfg(unix)]
fn validate_and_repair_pin_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() != 1 {
        return Err(invalid_pin_file(
            "pairing PIN path must be a single-link regular file",
        ));
    }
    if metadata.len() > MAX_PIN_FILE_BYTES {
        return Err(invalid_pin_file("pairing PIN file exceeds size limit"));
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn validate_and_repair_pin_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, GetFileType, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_TYPE_DISK,
    };

    let handle = HANDLE(file.as_raw_handle());
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(invalid_pin_file("pairing PIN path must be a disk file"));
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut information) }.map_err(windows_io_error)?;
    let rejected_attributes = FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0;
    if information.dwFileAttributes & rejected_attributes != 0 || information.nNumberOfLinks != 1 {
        return Err(invalid_pin_file(
            "pairing PIN path must be a single-link regular file without reparse points",
        ));
    }
    let file_size = ((information.nFileSizeHigh as u64) << 32) | information.nFileSizeLow as u64;
    if file_size > MAX_PIN_FILE_BYTES {
        return Err(invalid_pin_file("pairing PIN file exceeds size limit"));
    }
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn validate_and_repair_pin_file(file: &std::fs::File) -> std::io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_PIN_FILE_BYTES {
        return Err(invalid_pin_file(
            "pairing PIN path must be a bounded regular file",
        ));
    }
    Ok(())
}

fn invalid_pin_file(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

pub(super) fn persist_pin_atomically(path: &Path, pin: &str) -> std::io::Result<()> {
    persist_pin_atomically_with(path, pin, replace_pin_file)
}

fn persist_pin_atomically_with(
    path: &Path,
    pin: &str,
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    validate_existing_pin_path(path)?;

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "remote-input-pin.txt".to_string());
    let temp_path = path.with_file_name(format!(
        ".{file_name}.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));

    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut temp = options.open(&temp_path)?;
        temp.write_all(pin.as_bytes())?;
        temp.sync_all()?;
        validate_and_repair_pin_file(&temp)?;
        drop(temp);

        replace(&temp_path, path)?;
        sync_pin_parent(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

fn validate_existing_pin_path(path: &Path) -> std::io::Result<bool> {
    match open_pin_file(path) {
        Ok(file) => {
            validate_and_repair_pin_file(&file)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn replace_pin_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(target_os = "windows")]
fn replace_pin_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    if !validate_existing_pin_path(path)? {
        return move_windows_file(temp_path, path);
    }

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};

    let backup_path = path.with_file_name(format!(
        ".{}.backup-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "remote-input-pin.txt".to_string()),
        uuid::Uuid::new_v4().simple()
    ));
    let replaced = windows_wide_path(path);
    let replacement = windows_wide_path(temp_path);
    let backup = windows_wide_path(&backup_path);
    let result = unsafe {
        ReplaceFileW(
            PCWSTR(replaced.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR(backup.as_ptr()),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
    };
    match result {
        Ok(()) => {
            if let Err(error) = std::fs::remove_file(&backup_path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!(
                        "[remote-input] remove pairing PIN replacement backup failed at {}: {error}",
                        backup_path.display()
                    );
                }
            }
            Ok(())
        }
        Err(replace_error) => {
            rollback_windows_backup(&backup_path, path)?;
            Err(windows_io_error(replace_error))
        }
    }
}

#[cfg(not(any(unix, target_os = "windows")))]
fn replace_pin_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(target_os = "windows")]
fn move_windows_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let source = windows_wide_path(source);
    let destination = windows_wide_path(destination);
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(windows_io_error)
}

#[cfg(target_os = "windows")]
fn rollback_windows_backup(backup_path: &Path, path: &Path) -> std::io::Result<()> {
    if path_entry_exists(backup_path)? && !path_entry_exists(path)? {
        move_windows_file(backup_path, path)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn path_entry_exists(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "windows")]
fn windows_wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
fn windows_io_error(error: windows::core::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, error)
}

#[cfg(unix)]
fn sync_pin_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_pin_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(std::path::PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "openless-pin-{label}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn pin_path(&self) -> std::path::PathBuf {
            self.0.join("remote-input-pin.txt")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn load_or_create_test_pin(path: &Path) -> std::io::Result<String> {
        load_or_create_pin_at_path(path, || "654321".to_string())
    }

    #[test]
    fn atomic_replacement_never_leaves_partial_or_temporary_pin_files() {
        let root = TestDir::new("replace");
        let path = root.pin_path();

        persist_pin_atomically(&path, "123456").expect("persist initial PIN");
        persist_pin_atomically(&path, "654321").expect("replace PIN");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "654321");
        assert_eq!(directory_entry_names(&root), ["remote-input-pin.txt"]);
    }

    #[test]
    fn replacement_failure_preserves_old_pin_and_cleans_temporary_file() {
        let root = TestDir::new("replace-failure");
        let path = root.pin_path();
        persist_pin_atomically(&path, "123456").expect("persist initial PIN");

        let error = persist_pin_atomically_with(&path, "654321", |_temp_path, _path| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected replacement failure",
            ))
        })
        .expect_err("injected replacement must fail");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "123456");
        assert_eq!(directory_entry_names(&root), ["remote-input-pin.txt"]);
    }

    fn directory_entry_names(root: &TestDir) -> Vec<String> {
        let mut names = std::fs::read_dir(&root.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn invalid_or_partial_pin_is_replaced_with_a_complete_six_digit_pin() {
        for invalid in ["", "12", "12345x", "1234567"] {
            let root = TestDir::new("invalid");
            let path = root.pin_path();
            std::fs::write(&path, invalid).unwrap();

            let pin = load_or_create_test_pin(&path).unwrap();

            assert_eq!(pin.len(), 6);
            assert!(pin.bytes().all(|byte| byte.is_ascii_digit()));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), pin);
        }
    }

    #[cfg(unix)]
    #[test]
    fn temporary_pin_file_is_owner_only_before_replacement() {
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new("new-mode");
        let path = root.pin_path();
        persist_pin_atomically_with(&path, "123456", |temp_path, destination| {
            assert_eq!(
                std::fs::metadata(temp_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            std::fs::rename(temp_path, destination)
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn loading_valid_legacy_pin_repairs_permissive_mode_on_open_fd() {
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new("repair-mode");
        let path = root.pin_path();
        std::fs::write(&path, "123456").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(
            read_valid_persisted_pin(&path).unwrap().as_deref(),
            Some("123456")
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_swap_after_open_never_repairs_replacement_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root = TestDir::new("path-swap");
        let path = root.pin_path();
        std::fs::write(&path, "123456").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let opened = open_pin_file(&path).unwrap();

        let original = root.0.join("opened-original.txt");
        std::fs::rename(&path, &original).unwrap();
        let target = root.0.join("replacement-target.txt");
        std::fs::write(&target, "654321").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&target, &path).unwrap();

        assert_eq!(
            read_valid_pin_from_file(opened).unwrap().as_deref(),
            Some("123456")
        );
        assert_eq!(
            std::fs::metadata(&original).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "654321");
    }

    #[cfg(unix)]
    #[test]
    fn fifo_pin_path_is_rejected_without_blocking() {
        use std::os::unix::ffi::OsStrExt;
        use std::time::Duration;

        let root = TestDir::new("fifo");
        let path = root.pin_path();
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_valid_persisted_pin(&path).is_err());
        });

        assert!(rx
            .recv_timeout(Duration::from_millis(250))
            .expect("FIFO validation must not block"));
    }

    #[cfg(unix)]
    #[test]
    fn device_pin_path_is_rejected_before_reading() {
        assert!(read_valid_persisted_pin(Path::new("/dev/null")).is_err());
    }

    #[test]
    fn oversized_pin_file_is_rejected_before_unbounded_reading() {
        let root = TestDir::new("oversized");
        let path = root.pin_path();
        let oversized = format!("123456{}", " ".repeat(4096));
        std::fs::write(&path, &oversized).unwrap();

        assert!(read_valid_persisted_pin(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), oversized);
    }

    #[test]
    fn directory_pin_path_is_rejected_without_replacement() {
        let root = TestDir::new("directory");
        let path = root.pin_path();
        std::fs::create_dir(&path).unwrap();

        assert!(load_or_create_test_pin(&path).is_err());
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_fifo_is_rejected_without_opening_the_target() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;
        use std::time::Duration;

        let root = TestDir::new("symlink-fifo");
        let fifo = root.0.join("target-fifo");
        let c_fifo = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_fifo.as_ptr(), 0o600) }, 0);
        let path = root.pin_path();
        symlink(&fifo, &path).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_valid_persisted_pin(&path).is_err());
        });

        assert!(rx
            .recv_timeout(Duration::from_millis(250))
            .expect("symlink validation must not open a blocking target"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_pin_path_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new("symlink");
        let path = root.pin_path();
        let target = root.0.join("outside-target.txt");
        std::fs::write(&target, "123456").unwrap();
        symlink(&target, &path).unwrap();

        assert!(load_or_create_test_pin(&path).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "123456");
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn symlink_pin_path_is_rejected_without_touching_its_target() {
        use std::os::windows::fs::symlink_file;

        let root = TestDir::new("symlink");
        let path = root.pin_path();
        let target = root.0.join("outside-target.txt");
        std::fs::write(&target, "123456").unwrap();
        symlink_file(&target, &path).unwrap();

        assert!(load_or_create_test_pin(&path).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "123456");
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn hard_link_pin_path_is_rejected_without_touching_the_other_link() {
        let root = TestDir::new("hard-link");
        let path = root.pin_path();
        let other_link = root.0.join("other-link.txt");
        std::fs::write(&other_link, "123456").unwrap();
        std::fs::hard_link(&other_link, &path).unwrap();

        assert!(load_or_create_test_pin(&path).is_err());
        assert_eq!(std::fs::read_to_string(&other_link).unwrap(), "123456");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "123456");
    }
}
