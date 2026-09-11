use std::path::{Path, PathBuf};

const ROTATE_LIMIT_BYTES: u64 = 5 * 1024 * 1024;

pub fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("openless.log")
}

pub fn init_file_logger(data_dir: &Path) -> Result<PathBuf, String> {
    use simplelog::{
        ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TermLogger, TerminalMode,
        WriteLogger,
    };

    let path = log_path(data_dir);
    let parent = path.parent().ok_or("log path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    rotate_if_needed(&path).map_err(|error| error.to_string())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    let config = ConfigBuilder::new().set_time_format_rfc3339().build();
    CombinedLogger::init(vec![
        TermLogger::new(
            LevelFilter::Info,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(LevelFilter::Info, config, file),
    ])
    .map_err(|error| error.to_string())?;
    log::info!("Linux egui file logger ready: {}", path.display());
    Ok(path)
}

fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() <= ROTATE_LIMIT_BYTES {
        return Ok(());
    }
    let archive = path.with_file_name("openless.log.1");
    match std::fs::remove_file(&archive) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::rename(path, archive)
}

pub fn export_error_log(source: &Path, destination: &Path) -> Result<(), crate::DesktopError> {
    let bytes = std::fs::read(source).map_err(|source| crate::DesktopError::Io {
        operation: "read error log",
        source,
    })?;
    crate::atomic_save(destination, &bytes).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_the_current_log_with_atomic_save() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-log-export-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let source = log_path(&root);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"diagnostic\n").unwrap();
        let destination = root.join("exported.log");

        export_error_log(&source, &destination).unwrap();

        assert_eq!(std::fs::read(destination).unwrap(), b"diagnostic\n");
        std::fs::remove_dir_all(root).unwrap();
    }
}
