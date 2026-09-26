//! iOS 键盘扩展启用状态检测。
//!
//! 原理：`UITextInputMode.activeInputModes` 列出当前启用的输入模式；自定义
//! 键盘的 identifier 即其 bundle id（`com.openless.app.OpenLessKeyboard`）。
//! identifier 未在 Swift 头文件公开声明，属业界通用的 KVC 读取方式。
//!
//! 调用约束：必须在主线程执行（UIKit 约束）。命令层负责先 `run_on_main_thread`。
//!
//! 实现说明：objc2-ui-kit 0.2 没有为 UITextInputMode 类生成绑定，这里用
//! objc2 的运行时类查找 + msg_send 直接调。依赖图里同时存在 objc2 0.5（本包）
//! 与 0.6（tao/wry）两套运行时，msg_send 的返回值一律用裸指针声明、再经
//! `Retained::retain` 落回本包类型，避免宏展开时的 Encode trait 解析歧义。

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{ns_string, NSString};

use serde::{Deserialize, Serialize};

pub(crate) const KEYBOARD_BUNDLE_ID_PREFIX: &str = "com.openless.app.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardExtensionStatus {
    /// 键盘扩展是否已在系统设置中启用（设置 → 通用 → 键盘 → 键盘）。
    pub enabled: bool,
    /// 找到的 OpenLess 输入模式 identifier（诊断用）。
    pub identifier: Option<String>,
}

/// 查询失败时的错误载体（当前实现不产生错误，为命令层契约预留）。
#[derive(Debug)]
pub struct IsKeyboardExtensionEnabledError {
    pub message: String,
}

/// 检测 OpenLess 键盘扩展是否启用。必须在主线程调用。
pub(crate) fn detect_keyboard_extension_status() -> KeyboardExtensionStatus {
    let Some(cls) = AnyClass::get("UITextInputMode") else {
        log::warn!("[ios-keyboard] UITextInputMode class not registered");
        return KeyboardExtensionStatus {
            enabled: false,
            identifier: None,
        };
    };
    let modes: *mut AnyObject = unsafe { msg_send![cls, activeInputModes] };
    if modes.is_null() {
        log::warn!("[ios-keyboard] activeInputModes returned nil");
        return KeyboardExtensionStatus {
            enabled: false,
            identifier: None,
        };
    }

    let count: usize = unsafe { msg_send![modes, count] };
    for index in 0..count {
        let mode: *mut AnyObject = unsafe { msg_send![modes, objectAtIndex: index] };
        if mode.is_null() {
            continue;
        }
        // KVC 读取 identifier（自定义键盘的 bundle id）。
        let identifier: *mut NSString = unsafe {
            msg_send![mode, valueForKey: ns_string!("identifier")]
        };
        if identifier.is_null() {
            continue;
        }
        let Some(identifier) = (unsafe { Retained::retain(identifier) }) else {
            continue;
        };
        let identifier = identifier.to_string();
        if identifier.starts_with(KEYBOARD_BUNDLE_ID_PREFIX) {
            return KeyboardExtensionStatus {
                enabled: true,
                identifier: Some(identifier),
            };
        }
    }
    KeyboardExtensionStatus {
        enabled: false,
        identifier: None,
    }
}
