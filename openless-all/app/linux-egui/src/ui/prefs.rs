//! Linux UI 的外观偏好（仅表现层状态，不进 Core 合同）。
//!
//! 存放于 `$XDG_DATA_HOME/OpenLess/ui-prefs.json`（无 XDG 时
//! `~/.local/share/OpenLess/`），与 Core 的 `preferences.json` 同目录但互不
//! 干涉：Core 的 `UserPreferences` 是共享合同，深浅主题是 egui 宿主的表现
//! 层选择，两类状态分开持久化。

use serde::{Deserialize, Serialize};

/// UI 外观偏好。字段保持向后兼容：缺失字段按默认值回落。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UiPrefs {
    /// 深色主题（2.0 默认浅色）。
    #[serde(default)]
    pub dark: bool,
}

impl UiPrefs {
    fn file_path() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".local/share"))
            })?;
        Some(base.join("OpenLess").join("ui-prefs.json"))
    }

    /// 读取偏好；文件缺失或损坏时回落默认（浅色），不阻塞启动。
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// 写回偏好；失败只记日志，不影响运行。
    pub fn save(&self) {
        let Some(path) = Self::file_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析容错：旧文件缺字段/坏 JSON 都回落默认，不 panic。
    #[test]
    fn parses_legacy_and_corrupt_files_leniently() {
        assert_eq!(
            serde_json::from_str::<UiPrefs>("{}").unwrap(),
            UiPrefs::default()
        );
        assert_eq!(
            serde_json::from_str::<UiPrefs>(r#"{"dark":true}"#).unwrap(),
            UiPrefs { dark: true }
        );
        assert!(serde_json::from_str::<UiPrefs>("not json").is_err());
    }
}
