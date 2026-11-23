//! ASR 引擎需要的宿主路径。
//!
//! 模型缓存、运行时目录、日志目录都由宿主决定（AppData / Application Support /
//! 应用私有目录各不相同）。平台层不猜路径，只用宿主注册进来的解析器。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 一组惰性路径解析器。返回 `Err` 表示该目录当前不可用（例如宿主还没初始化）。
pub struct AsrPaths {
    pub foundry_local_root: fn() -> anyhow::Result<PathBuf>,
    pub foundry_native_runtime_root: fn() -> anyhow::Result<PathBuf>,
    pub foundry_model_cache_root: fn() -> anyhow::Result<PathBuf>,
    pub foundry_app_data_root: fn() -> anyhow::Result<PathBuf>,
    pub foundry_logs_root: fn() -> anyhow::Result<PathBuf>,
}

static PATHS: OnceLock<AsrPaths> = OnceLock::new();

/// 注册宿主路径解析器。宿主启动时调用一次；重复调用只有第一次生效。
pub fn install_paths(paths: AsrPaths) {
    let _ = PATHS.set(paths);
}

fn paths() -> anyhow::Result<&'static AsrPaths> {
    PATHS
        .get()
        .ok_or_else(|| anyhow::anyhow!("ASR host paths are not installed"))
}

pub fn foundry_local_root() -> anyhow::Result<PathBuf> {
    (paths()?.foundry_local_root)()
}

pub fn foundry_native_runtime_root() -> anyhow::Result<PathBuf> {
    (paths()?.foundry_native_runtime_root)()
}

pub fn foundry_model_cache_root() -> anyhow::Result<PathBuf> {
    (paths()?.foundry_model_cache_root)()
}

pub fn foundry_app_data_root() -> anyhow::Result<PathBuf> {
    (paths()?.foundry_app_data_root)()
}

pub fn foundry_logs_root() -> anyhow::Result<PathBuf> {
    (paths()?.foundry_logs_root)()
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_installation_reports_an_explicit_error() {
        // 本测试独立于全局状态：未安装时必须是可读错误而不是 panic。
        let error = super::foundry_local_root().unwrap_err().to_string();
        assert!(error.contains("not installed") || error.contains("ASR host paths"));
    }
}
