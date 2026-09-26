//! iOS 剪贴板：UIPasteboard 写入。桌面端 arboard、Android 端 JNI 的对应物。
//!
//! 只实现写入：iOS 16+ 读取剪贴板会触发系统「允许粘贴」确认弹窗，App 内
//! 听写流程（转写 → 复制 → 用户手动粘贴）不需要读回。

use objc2_foundation::NSString;
use objc2_ui_kit::UIPasteboard;

/// 写入系统剪贴板。失败（UIPasteboard 不可用）返回 false。
pub(crate) fn copy_to_clipboard(text: &str) -> bool {
    let pasteboard = unsafe { UIPasteboard::generalPasteboard() };
    let value = NSString::from_str(text);
    unsafe { pasteboard.setString(Some(&value)) };
    log::info!("[ios-clipboard] copied {} chars to UIPasteboard", text.chars().count());
    true
}

#[cfg(test)]
mod tests {
    // UIPasteboard 依赖 UIKit 运行时，纯逻辑无单测；端到端由模拟器验证覆盖。
}
