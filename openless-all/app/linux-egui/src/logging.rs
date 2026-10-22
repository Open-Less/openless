//! Linux egui 的日志后端。
//!
//! 两个进程（宿主 `openless`、UI 窗口 `openless --ui-client`）都写同一个
//! `$XDG_DATA/openless/logs/openless.log`，这样「窗口进程怎么没了」这类问题
//! 两端的记录在同一处。
//!
//! 三条约定，缺一条日志就会退化成没人看的噪声：
//!
//! 1. **级别可调，不用重新编译**。`OPENLESS_LOG`（或 `RUST_LOG`）决定过滤，
//!    例如 `OPENLESS_LOG=debug`、`OPENLESS_LOG=openless_core=debug,info`。
//!    默认 `info` —— 发布版看不出问题的时候，用户可以只加一个环境变量重启复现。
//! 2. **周期心跳不许占满文件**。日志是 append + 轮转，任何每 10 秒一条的
//!    「我还在」都会在一天内吃掉整个配额，把真正的错误挤出去。心跳走 `debug`，
//!    `info` 只留状态变化与低频存活证明（见 `log_pump_heartbeat`）。
//! 3. **`error` 只写失败**，不写进度。轮转后仍有 3 份历史，够回溯一次事故。

use std::path::{Path, PathBuf};

/// 单文件上限。整个日志目录最多 `ROTATE_KEEP + 1` 份，即 8 MiB。
const ROTATE_LIMIT_BYTES: u64 = 2 * 1024 * 1024;
/// 保留的历史份数：`openless.log.1` … `.3`。
const ROTATE_KEEP: usize = 3;
/// 无法解析 `OPENLESS_LOG` 时的默认级别。
const DEFAULT_LEVEL: log::LevelFilter = log::LevelFilter::Info;

pub fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("openless.log")
}

/// 环境变量名：优先 `OPENLESS_LOG`，兼容 Rust 生态惯用的 `RUST_LOG`。
const ENV_KEYS: [&str; 2] = ["OPENLESS_LOG", "RUST_LOG"];

pub fn init_file_logger(data_dir: &Path) -> Result<PathBuf, String> {
    use simplelog::{ColorChoice, SharedLogger, TermLogger, TerminalMode, WriteLogger};

    let path = log_path(data_dir);
    let parent = path.parent().ok_or("log path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    // 只有宿主轮转：UI 窗口进程若是也有资格改名，宿主手里的文件句柄会指向被
    // 改名的 inode，之后的宿主管日志就写进 `.1` 里了。
    let rotated = if is_ui_client_process() {
        false
    } else {
        rotate_if_needed(&path).map_err(|error| error.to_string())?
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| error.to_string())?;

    let filter = FilterSpec::from_env();
    let config = base_config();
    let sinks: Vec<Box<dyn SharedLogger>> = vec![
        // 终端上也用同一份过滤结果，级别再放宽到 Trace 交给 FilterSpec 决定。
        TermLogger::new(
            log::LevelFilter::Trace,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(log::LevelFilter::Trace, config, file),
    ];

    let logger = FilteredLogger {
        sinks,
        spec: filter.clone(),
    };
    log::set_max_level(filter.max_level());
    log::set_boxed_logger(Box::new(logger)).map_err(|error| error.to_string())?;

    log::info!(
        "Linux egui logger ready: level={} rotated={rotated} path={}",
        filter.describe(),
        path.display()
    );
    Ok(path)
}

fn base_config() -> simplelog::Config {
    use simplelog::{ConfigBuilder, LevelFilter, ThreadLogMode};
    // simplelog 的 builder 方法是 `&mut self` 风格，先持有再链式调用。
    let mut builder = ConfigBuilder::new();
    builder.set_time_format_rfc3339();
    // 模块名一直在，定位「哪一层的日志」不用猜。
    // simplelog 的判据是 `config.target <= record.level()`，级别越小越宽松，
    // 所以「始终打印」要设在 Error（最严格端），设成 Trace 反而不打印。
    builder.set_target_level(LevelFilter::Error);
    // 线程与源码位置只在 debug 及以上出现，避免 info 行变长。
    builder.set_thread_level(LevelFilter::Debug);
    builder.set_thread_mode(ThreadLogMode::Names);
    builder.set_location_level(LevelFilter::Debug);
    // 用户在 +08:00，日志却是 UTC 的 `Z`：让时间戳落到本地时区。
    // 判定不出来（Err）就保持 UTC，不影响其余配置。
    let _ = builder.set_time_offset_to_local();
    builder.build()
}

/// 日志过滤规则：一个默认级别 + 若干 `模块前缀=级别` 覆盖。
#[derive(Clone)]
struct FilterSpec {
    default: log::LevelFilter,
    overrides: Vec<(String, log::LevelFilter)>,
}

impl FilterSpec {
    fn from_env() -> Self {
        for key in ENV_KEYS {
            if let Ok(value) = std::env::var(key) {
                if let Some(spec) = Self::parse(&value) {
                    return spec;
                }
            }
        }
        Self {
            default: DEFAULT_LEVEL,
            overrides: Vec::new(),
        }
    }

    /// 解析 `debug` / `info,openless_core=debug` / `OPENLESS_LOG=*`。
    /// 任何一条无法理解的片段都退回默认级别：宁可多记，不可静默少记。
    fn parse(value: &str) -> Option<Self> {
        let mut default = None;
        let mut overrides = Vec::new();
        for piece in value.split(',') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            let (target, level) = match piece.split_once('=') {
                Some((target, level)) => (Some(target.trim()), level.trim()),
                None => (None, piece),
            };
            let level = parse_level(level)?;
            match target {
                Some(target) if !target.is_empty() && target != "*" => {
                    overrides.push((target.to_string(), level));
                }
                _ => default = Some(level),
            }
        }
        Some(Self {
            default: default.unwrap_or(DEFAULT_LEVEL),
            overrides,
        })
    }

    fn level_for(&self, target: &str) -> log::LevelFilter {
        // 最长匹配优先：`openless_core::asr` 比 `openless_core` 更具体。
        let mut best: Option<(usize, log::LevelFilter)> = None;
        for (prefix, level) in &self.overrides {
            if target == prefix || target.starts_with(&format!("{prefix}::")) {
                let len = prefix.len();
                if best.is_none_or(|(best_len, _)| len >= best_len) {
                    best = Some((len, *level));
                }
            }
        }
        best.map(|(_, level)| level).unwrap_or(self.default)
    }

    fn max_level(&self) -> log::LevelFilter {
        self.overrides
            .iter()
            .map(|(_, level)| *level)
            .chain(std::iter::once(self.default))
            .max()
            .unwrap_or(DEFAULT_LEVEL)
    }

    fn describe(&self) -> String {
        let overrides = self
            .overrides
            .iter()
            .map(|(target, level)| format!("{target}={}", level_label(*level)))
            .collect::<Vec<_>>();
        if overrides.is_empty() {
            level_label(self.default).to_string()
        } else {
            format!("{} ({})", level_label(self.default), overrides.join(","))
        }
    }
}

fn parse_level(value: &str) -> Option<log::LevelFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" => Some(log::LevelFilter::Off),
        "error" => Some(log::LevelFilter::Error),
        "warn" | "warning" => Some(log::LevelFilter::Warn),
        "info" => Some(log::LevelFilter::Info),
        "debug" => Some(log::LevelFilter::Debug),
        "trace" => Some(log::LevelFilter::Trace),
        _ => None,
    }
}

fn level_label(level: log::LevelFilter) -> &'static str {
    match level {
        log::LevelFilter::Off => "off",
        log::LevelFilter::Error => "error",
        log::LevelFilter::Warn => "warn",
        log::LevelFilter::Info => "info",
        log::LevelFilter::Debug => "debug",
        log::LevelFilter::Trace => "trace",
    }
}

/// 把 `FilterSpec` 套在所有 sink 前面。simplelog 自身只支持单一级别，
/// 按模块区分级别得在它外面做一层。
struct FilteredLogger {
    sinks: Vec<Box<dyn simplelog::SharedLogger>>,
    spec: FilterSpec,
}

impl log::Log for FilteredLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.spec.level_for(metadata.target())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        for sink in &self.sinks {
            sink.log(record);
        }
    }

    fn flush(&self) {
        for sink in &self.sinks {
            sink.flush();
        }
    }
}

/// UI 窗口进程由宿主拉起，参数里带 `--ui-client`。
fn is_ui_client_process() -> bool {
    std::env::args().any(|arg| arg == "--ui-client")
}

/// 按 `.log` → `.1` → … → `.ROTATE_KEEP` 依次后移，最老的一份被丢弃。
/// 返回是否真的轮转过。
fn rotate_if_needed(path: &Path) -> std::io::Result<bool> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(false);
    };
    if metadata.len() <= ROTATE_LIMIT_BYTES {
        return Ok(false);
    }
    let archive = |index: usize| path.with_file_name(format!("openless.log.{index}"));
    // 先把最老的删掉，再逐级改名，最后把当前文件挪成 `.1`。
    match std::fs::remove_file(archive(ROTATE_KEEP)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for index in (1..ROTATE_KEEP).rev() {
        match std::fs::rename(archive(index), archive(index + 1)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    std::fs::rename(path, archive(1))?;
    Ok(true)
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

    #[test]
    fn parses_plain_and_per_module_levels() {
        let spec = FilterSpec::parse("debug").unwrap();
        assert_eq!(spec.level_for("openless_core"), log::LevelFilter::Debug);

        let spec = FilterSpec::parse("info,openless_core=debug").unwrap();
        assert_eq!(spec.level_for("openless_core::asr"), log::LevelFilter::Debug);
        assert_eq!(spec.level_for("openless_linux_egui"), log::LevelFilter::Info);

        // 更具体的模块前缀优先。
        let spec = FilterSpec::parse("openless_core=debug,openless_core::asr=warn").unwrap();
        assert_eq!(
            spec.level_for("openless_core::asr::volcengine"),
            log::LevelFilter::Warn
        );
        assert_eq!(spec.level_for("openless_core::api"), log::LevelFilter::Debug);
    }

    #[test]
    fn rejects_garbage_instead_of_silencing_everything() {
        assert!(FilterSpec::parse("loud").is_none());
    }

    #[test]
    fn rotates_into_numbered_archives_and_drops_the_oldest() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-log-rotate-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("openless.log");
        std::fs::write(&path, vec![b'x'; (ROTATE_LIMIT_BYTES + 1) as usize]).unwrap();
        std::fs::write(root.join("openless.log.1"), b"one").unwrap();
        std::fs::write(root.join("openless.log.2"), b"two").unwrap();
        std::fs::write(root.join("openless.log.3"), b"three").unwrap();

        assert!(rotate_if_needed(&path).unwrap());

        assert!(!path.exists(), "当前文件应被移走");
        assert_eq!(std::fs::read(root.join("openless.log.1")).unwrap().len(), (ROTATE_LIMIT_BYTES + 1) as usize);
        assert_eq!(std::fs::read(root.join("openless.log.2")).unwrap(), b"one");
        assert_eq!(std::fs::read(root.join("openless.log.3")).unwrap(), b"two");
        assert!(!root.join("openless.log.4").exists(), "超出保留份数的文件应被删除");

        // 小于上限时不轮转。
        std::fs::write(&path, b"small").unwrap();
        assert!(!rotate_if_needed(&path).unwrap());
        assert!(path.exists());

        std::fs::remove_dir_all(root).unwrap();
    }
}
