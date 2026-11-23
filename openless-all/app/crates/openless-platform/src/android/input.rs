//! Android 按键注入后端。
//!
//! Android 没有可用的全局合成按键通道（除非走无障碍或 root），
//! 这里返回明确错误，由上层降级到无障碍插入。

use crate::common::error::PlatformError;

pub fn send_copy_shortcut() -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}
