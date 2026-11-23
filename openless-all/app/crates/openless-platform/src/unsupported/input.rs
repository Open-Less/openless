//! 其余平台的按键注入占位。

use crate::input::inject::PlatformError;

pub(crate) fn send_copy_shortcut() -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}
