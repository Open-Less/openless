//! 没有原生全局热键后端的桌面构建：安装直接失败，调用方据此给用户明确提示，
//! 而不是默默不工作。真实实现见 macOS（CGEventTap）与 Windows（WH_KEYBOARD_LL）。

use super::{
    HotkeyAdapter, HotkeyBinding, HotkeyCombinedEdge, HotkeyEvent, HotkeyInstallError,
};
use std::sync::mpsc::Sender;

pub fn start_adapter(
    _binding: HotkeyBinding,
    _tx: Sender<HotkeyEvent>,
    _cancel_tx: Sender<()>,
    _combo_tx: Sender<HotkeyCombinedEdge>,
) -> Result<Box<dyn HotkeyAdapter>, HotkeyInstallError> {
    Err(HotkeyInstallError {
        code: "hotkey_unsupported".to_string(),
        message: "当前平台不提供原生全局热键后端".to_string(),
    })
}
