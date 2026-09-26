//! 键盘扩展共享配置（App Group 容器）。
//!
//! 主 App 把键盘转写配置写到 App Group 的 `kb-config.json`，键盘扩展启动时
//! 读取。v1 的 apiKey 明文存于 App Group JSON——真机分发需换 Keychain
//! access group（与桌面凭据库同机制），模拟器开发阶段可接受（无签名构建
//! 本就无法上真机）。TODO(ios-port): Keychain access group。

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSString, NSURL};

use super::KeyboardConfig;

const APP_GROUP_ID: &str = "group.com.openless.app";
const CONFIG_FILE: &str = "kb-config.json";

/// App Group 容器目录。容器不可用（如真机缺 App Group entitlement）返回 None。
pub fn app_group_dir() -> Option<std::path::PathBuf> {
    unsafe {
        let cls = AnyClass::get("NSFileManager")?;
        let manager: *mut AnyObject = msg_send![cls, defaultManager];
        if manager.is_null() {
            return None;
        }
        let group_id = NSString::from_str(APP_GROUP_ID);
        let url: *mut NSURL = msg_send![manager,
            containerURLForSecurityApplicationGroupIdentifier: &*group_id
        ];
        if url.is_null() {
            return None;
        }
        let retained: Retained<NSURL> = Retained::retain(url)?;
        let path: *mut NSString = unsafe { msg_send![&retained, path] };
        if path.is_null() {
            return None;
        }
        let path_string: Retained<NSString> = Retained::retain(path)?;
        Some(std::path::PathBuf::from(path_string.to_string()))
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    app_group_dir().map(|dir| dir.join(CONFIG_FILE))
}

/// 读取键盘配置。容器不可用或文件缺失/损坏时返回默认值（is_configured=false）。
pub fn load_keyboard_config() -> KeyboardConfig {
    let Some(path) = config_path() else {
        return KeyboardConfig::default();
    };
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => KeyboardConfig::default(),
    }
}

/// 写入键盘配置。容器不可用时报错（前端提示检查签名/系统设置）。
pub fn store_keyboard_config(config: &KeyboardConfig) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err(format!(
            "App Group 容器不可用（{APP_GROUP_ID}）：真机需要 App Group entitlement 与有效签名"
        ));
    };
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|error| error.to_string())
}

/// App Group 容器是否可用（设置页诊断用）。
pub fn app_group_available() -> bool {
    app_group_dir().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_unconfigured() {
        let config = KeyboardConfig::default();
        assert!(!config.is_configured());
    }

    #[test]
    fn config_roundtrips_through_json() {
        let config = KeyboardConfig {
            endpoint: "https://api.example.com/v1".into(),
            api_key: "sk-test".into(),
            model: "whisper-1".into(),
            prompt: "技术词汇".into(),
        };
        assert!(config.is_configured());
        let bytes = serde_json::to_vec(&config).unwrap();
        let parsed: KeyboardConfig = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.endpoint, config.endpoint);
        assert_eq!(parsed.api_key, config.api_key);
    }
}
