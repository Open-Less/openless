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
/// 设置页写入的级别文件名，位于 `<data_dir>/logs/level`。
/// 环境变量仍然优先：临时排查不该被上一次的界面设置挡住。
const LEVEL_FILE_NAME: &str = "level";

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

    let filter = FilterSpec::resolve(data_dir);
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
    /// 优先级：`OPENLESS_LOG`/`RUST_LOG` → 设置里存的级别 → `info`。
    fn resolve(data_dir: &Path) -> Self {
        for key in ENV_KEYS {
            if let Ok(value) = std::env::var(key) {
                if let Some(spec) = Self::parse(&value) {
                    return spec;
                }
            }
        }
        if let Some(level) = read_log_level(data_dir) {
            return Self {
                default: level,
                overrides: Vec::new(),
            };
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

/// 设置页保存的日志级别；文件不存在或内容无法识别时返回 `None`。
pub fn read_log_level(data_dir: &Path) -> Option<log::LevelFilter> {
    let path = data_dir.join("logs").join(LEVEL_FILE_NAME);
    let raw = std::fs::read_to_string(path).ok()?;
    parse_level(raw.trim())
}

/// 把级别写进 `<data_dir>/logs/level`，下次启动生效。
pub fn write_log_level(data_dir: &Path, level: &str) -> std::io::Result<log::LevelFilter> {
    let Some(parsed) = parse_level(level) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown log level: {level}"),
        ));
    };
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir)?;
    crate::atomic_save(&dir.join(LEVEL_FILE_NAME), level_name(parsed).as_bytes())
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(parsed)
}

/// 级别的稳定名称：文件名、设置项与日志首行都用它，避免各自拼写。
pub fn level_name(level: log::LevelFilter) -> &'static str {
    level_label(level)
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

/// 导出「当前日志 + 全部轮转历史」到单个文件。
///
/// 只导出 `openless.log` 时，用户报的问题几乎总是发生在轮转之后 —— 现场刚好被
/// 切进 `.1` 而导出里没有。这里按 `.3` → `.2` → `.1` → 当前的顺序拼接（时间
/// 正序），每段加分隔头，并在最前面写明版本、系统、生效级别与导出时间，
/// 省掉「你用的哪个版本」这一轮往返。
pub fn export_error_logs(data_dir: &Path, destination: &Path) -> Result<(), crate::DesktopError> {
    let current = log_path(data_dir);
    let mut out = String::new();
    out.push_str("# OpenLess Linux diagnostics\n");
    out.push_str(&format!("version: {}\n", env!("OPENLESS_APP_VERSION")));
    out.push_str(&format!(
        "os: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    out.push_str(&format!(
        "level: {}\n",
        level_name(
            std::env::var("OPENLESS_LOG")
                .ok()
                .and_then(|value| FilterSpec::parse(&value))
                .map_or_else(
                    || read_log_level(data_dir).unwrap_or(DEFAULT_LEVEL),
                    |spec| spec.default
                )
        )
    ));
    out.push_str(&format!(
        "exported_at: {}\n\n",
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    ));

    let mut exported = 0usize;
    for index in (1..=ROTATE_KEEP).rev() {
        let path = current.with_file_name(format!("openless.log.{index}"));
        if append_log_section(&mut out, &path, &format!("openless.log.{index}"))? {
            exported += 1;
        }
    }
    if append_log_section(&mut out, &current, "openless.log")? {
        exported += 1;
    }
    if exported == 0 {
        return Err(crate::DesktopError::Io {
            operation: "read error log",
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no log file yet"),
        });
    }
    crate::atomic_save(destination, out.as_bytes()).map(|_| ())
}

fn append_log_section(
    out: &mut String,
    path: &Path,
    label: &str,
) -> Result<bool, crate::DesktopError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            out.push_str(&format!("===== {label} =====\n"));
            out.push_str(&String::from_utf8_lossy(&bytes));
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(crate::DesktopError::Io {
            operation: "read error log",
            source,
        }),
    }
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
    fn exports_every_rotated_file_with_a_header() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-log-export-all-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("openless.log"), b"newest\n").unwrap();
        std::fs::write(logs.join("openless.log.1"), b"older\n").unwrap();
        std::fs::write(logs.join("openless.log.2"), b"oldest\n").unwrap();
        let destination = root.join("diagnostics.log");

        export_error_logs(&root, &destination).unwrap();
        let text = std::fs::read_to_string(&destination).unwrap();

        assert!(text.starts_with("# OpenLess Linux diagnostics\n"));
        assert!(text.contains("version: "));
        // 时间正序：`.2` 在 `.1` 之前，当前文件在最后。
        let oldest = text.find("oldest").unwrap();
        let older = text.find("older").unwrap();
        let newest = text.find("newest").unwrap();
        assert!(oldest < older && older < newest, "sections must be chronological");
        assert!(text.contains("===== openless.log.2 ====="));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_the_level_for_the_next_start() {
        let root = std::env::temp_dir().join(format!(
            "openless-linux-log-level-{}",
            uuid::Uuid::new_v4().simple()
        ));
        assert_eq!(read_log_level(&root), None);
        assert_eq!(
            write_log_level(&root, "debug").unwrap(),
            log::LevelFilter::Debug
        );
        assert_eq!(read_log_level(&root), Some(log::LevelFilter::Debug));
        assert!(write_log_level(&root, "loud").is_err());
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
