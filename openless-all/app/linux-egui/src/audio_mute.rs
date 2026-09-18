//! Temporary system-output mute while recording.
//!
//! Linux parity with the legacy desktop [`AudioMuteGuard`] behavior.
//! `mute_during_recording` is applied by
//! the host recorder (never by Core) so the capture itself never hears the
//! user's speakers.  Restore is guaranteed by RAII: the guard restores the
//! previous mute state when it is dropped, which happens on every terminal
//! path of the recorder lifecycle — normal stop, cancel, runtime fault/error
//! and process shutdown — because Core consumes/drops the active recording
//! handle on each of those.
//!
//! Muting is intentionally best-effort like the reference: if neither `wpctl`
//! (PipeWire) nor `pactl` (PulseAudio) can drive the default sink, activation
//! fails but capture still proceeds.  The parsed state helpers are pure so
//! they can be unit-tested without a live audio server.

/// The output-mute backend discovered at activation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MuteBackend {
    Wpctl,
    Pactl,
}

/// Guard that restores the previous default-sink mute state on drop.
#[derive(Debug)]
pub struct AudioMuteGuard {
    inner: Option<platform::PlatformMuteGuard>,
}

impl AudioMuteGuard {
    /// Mute the default output sink unless it is already muted, capturing the
    /// previous state for later restore.  Best-effort: on failure returns an
    /// error and leaves the sink untouched.
    pub fn activate() -> Result<Self, String> {
        Ok(Self {
            inner: Some(platform::activate()?),
        })
    }

    /// A no-op guard used when `mute_during_recording` is disabled.
    pub fn none() -> Self {
        Self { inner: None }
    }
}

impl Drop for AudioMuteGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.restore();
        }
    }
}

/// `true` when a `wpctl get-volume` capture reports the sink muted.
pub fn parse_wpctl_muted(output: &str) -> bool {
    output.contains("[MUTED]")
}

/// `true` when a `pactl get-sink-mute` capture reports the sink muted.
pub fn parse_pactl_muted(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("yes") || output.contains("是")
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{parse_pactl_muted, parse_wpctl_muted, MuteBackend};
    use std::process::Command;

    #[derive(Debug)]
    pub struct PlatformMuteGuard {
        backend: MuteBackend,
        was_muted: bool,
    }

    pub fn activate() -> Result<PlatformMuteGuard, String> {
        if let Ok(was_muted) = wpctl_muted() {
            if !was_muted {
                set_wpctl_muted(true)?;
            }
            return Ok(PlatformMuteGuard {
                backend: MuteBackend::Wpctl,
                was_muted,
            });
        }
        let was_muted = pactl_muted()?;
        if !was_muted {
            set_pactl_muted(true)?;
        }
        Ok(PlatformMuteGuard {
            backend: MuteBackend::Pactl,
            was_muted,
        })
    }

    impl PlatformMuteGuard {
        pub fn restore(self) {
            let result = match self.backend {
                MuteBackend::Wpctl => set_wpctl_muted(self.was_muted),
                MuteBackend::Pactl => set_pactl_muted(self.was_muted),
            };
            if let Err(error) = result {
                log::warn!("[audio-mute] restore output mute failed: {error}");
            }
        }
    }

    fn wpctl_muted() -> Result<bool, String> {
        let output = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .map_err(|error| format!("wpctl get-volume failed: {error}"))?;
        if !output.status.success() {
            return Err(std::str::from_utf8(&output.stderr)
                .unwrap_or_default()
                .trim()
                .to_string());
        }
        Ok(parse_wpctl_muted(
            std::str::from_utf8(&output.stdout).unwrap_or_default(),
        ))
    }

    fn set_wpctl_muted(muted: bool) -> Result<(), String> {
        let value = if muted { "1" } else { "0" };
        let output = Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", value])
            .output()
            .map_err(|error| format!("wpctl set-mute failed: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(std::str::from_utf8(&output.stderr)
                .unwrap_or_default()
                .trim()
                .to_string())
        }
    }

    fn pactl_muted() -> Result<bool, String> {
        let output = Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
            .map_err(|error| format!("pactl get-sink-mute failed: {error}"))?;
        if !output.status.success() {
            return Err(std::str::from_utf8(&output.stderr)
                .unwrap_or_default()
                .trim()
                .to_string());
        }
        Ok(parse_pactl_muted(
            std::str::from_utf8(&output.stdout).unwrap_or_default(),
        ))
    }

    fn set_pactl_muted(muted: bool) -> Result<(), String> {
        let value = if muted { "1" } else { "0" };
        let output = Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", value])
            .output()
            .map_err(|error| format!("pactl set-sink-mute failed: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(std::str::from_utf8(&output.stderr)
                .unwrap_or_default()
                .trim()
                .to_string())
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    #[derive(Debug)]
    pub struct PlatformMuteGuard;

    pub fn activate() -> Result<PlatformMuteGuard, String> {
        Err("output mute is not supported on this platform".to_string())
    }

    impl PlatformMuteGuard {
        pub fn restore(self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpctl_output_parser_detects_muted_flag() {
        assert!(parse_wpctl_muted("Volume: 0.00 [MUTED]"));
        assert!(!parse_wpctl_muted("Volume: 0.82"));
    }

    #[test]
    fn pactl_output_parser_detects_muted_flag_in_any_locale() {
        assert!(parse_pactl_muted("Mute: yes"));
        assert!(parse_pactl_muted("静音：是"));
        assert!(!parse_pactl_muted("Mute: no"));
        assert!(!parse_pactl_muted("静音：否"));
    }

    #[test]
    fn disabled_guard_is_a_noop_that_drops_without_panicking() {
        // `none()` is a pure no-op guard; dropping it must not panic and has no
        // command side effect to assert, so we only verify construction/drop.
        let guard = AudioMuteGuard::none();
        drop(guard);
    }
}
