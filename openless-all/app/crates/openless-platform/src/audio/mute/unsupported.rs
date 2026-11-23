//! 其余平台的静音占位。

pub struct PlatformMuteGuard;

pub fn activate() -> Result<PlatformMuteGuard, String> {
    Err("output mute is not supported on this platform".to_string())
}

impl PlatformMuteGuard {
    pub fn restore(self) {}
}
