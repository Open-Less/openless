//! 录音期间的系统输出静音。
//!
//! 语义是"临时借用"：开启时记下原状态，`drop` 时无条件恢复。
//! 平台实现差异很大，但对外语义一致——拿不到静音能力时返回错误，由宿主决定是否忽略。

#[cfg(target_os = "windows")]
#[path = "mute/windows.rs"]
mod windows;
#[cfg(target_os = "macos")]
#[path = "mute/macos.rs"]
mod macos;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[path = "mute/unsupported.rs"]
mod unsupported;

#[cfg(target_os = "windows")]
use windows as backend;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
use unsupported as backend;

/// 静音守卫：构造即静音，析构即恢复。
///
/// 构造失败时宿主应继续录音（静音只是优化，不是功能前提）。
pub struct AudioMuteGuard {
    inner: Option<backend::PlatformMuteGuard>,
}

impl AudioMuteGuard {
    /// 静音系统输出，并记住原状态。
    pub fn activate() -> Result<Self, String> {
        Ok(Self {
            inner: Some(backend::activate()?),
        })
    }

    /// 立即恢复原状态（`drop` 时会再调一次，重复调用是安全的）。
    pub fn restore(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.restore();
        }
    }
}

impl Drop for AudioMuteGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn restoring_twice_is_harmless() {
        // 用不可用后端构造一个空守卫，验证重复 restore 不会 panic。
        let mut guard = super::AudioMuteGuard { inner: None };
        guard.restore();
        guard.restore();
    }
}
