//! 没有侧键监听后端的平台（Android / iOS / Linux）。
//!
//! 侧键组合依赖系统级键盘钩子；这些平台不提供，因此安装会显式失败，
//! 而不是注册一个永远不会触发的东西。

/// 该平台没有侧键分发后端。
pub fn dispatch_vk(_vk_code: u32, _pressed: bool) -> bool {
    false
}
