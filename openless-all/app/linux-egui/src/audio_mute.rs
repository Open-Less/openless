//! Restore the output that was muted at activation, even if the default changes.
use std::sync::Arc;

trait OutputControl: Send + Sync {
    fn current(&self) -> Result<(String, bool), String>;
    fn set_muted(&self, sink: &str, muted: bool) -> Result<(), String>;
}

pub struct AudioMuteGuard {
    inner: Option<(Arc<dyn OutputControl>, String, bool)>,
}
impl std::fmt::Debug for AudioMuteGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioMuteGuard")
            .field("active", &self.inner.is_some())
            .finish()
    }
}
impl AudioMuteGuard {
    pub fn activate() -> Result<Self, String> {
        Self::with_control(Arc::new(NativeOutput))
    }
    fn with_control(control: Arc<dyn OutputControl>) -> Result<Self, String> {
        let (sink, was_muted) = control.current()?;
        let guard = Self {
            inner: Some((control.clone(), sink.clone(), was_muted)),
        };
        if !was_muted {
            control.set_muted(&sink, true)?;
        }
        Ok(guard)
    }
    pub fn none() -> Self {
        Self { inner: None }
    }
}
impl Drop for AudioMuteGuard {
    fn drop(&mut self) {
        if let Some((control, sink, was_muted)) = self.inner.take() {
            if let Err(error) = control.set_muted(&sink, was_muted) {
                log::warn!("restore recording output mute: {error}");
            }
        }
    }
}
pub fn parse_wpctl_muted(output: &str) -> bool {
    output.contains("[MUTED]")
}
pub fn parse_pactl_muted(output: &str) -> bool {
    output.to_ascii_lowercase().contains("yes") || output.contains('是')
}
struct NativeOutput;
#[cfg(target_os = "linux")]
fn command(program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("timeout")
        .args(["--signal=KILL", "2s", program])
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
#[cfg(target_os = "linux")]
impl OutputControl for NativeOutput {
    fn current(&self) -> Result<(String, bool), String> {
        // PipeWire's PulseAudio compatibility service uses the same stable sink
        // names, which also work on the GNOME 42 PulseAudio baseline.
        if let Ok(sink) = command("pactl", &["get-default-sink"]) {
            let sink = sink.trim();
            if !sink.is_empty() {
                let state = command("pactl", &["get-sink-mute", sink])?;
                return Ok((format!("pulse:{sink}"), parse_pactl_muted(&state)));
            }
        }
        let object = command("wpctl", &["inspect", "@DEFAULT_AUDIO_SINK@"])?;
        let id = object
            .trim()
            .strip_prefix("id ")
            .and_then(|s| s.split(',').next())
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("cannot identify the original output sink")?;
        let state = command("wpctl", &["get-volume", id])?;
        Ok((format!("pipewire:{id}"), parse_wpctl_muted(&state)))
    }
    fn set_muted(&self, sink: &str, muted: bool) -> Result<(), String> {
        let value = if muted { "1" } else { "0" };
        if let Some(name) = sink.strip_prefix("pulse:") {
            command("pactl", &["set-sink-mute", name, value])?;
        } else if let Some(id) = sink.strip_prefix("pipewire:") {
            command("wpctl", &["set-mute", id, value])?;
        } else {
            return Err("invalid output identity".into());
        }
        Ok(())
    }
}
#[cfg(not(target_os = "linux"))]
impl OutputControl for NativeOutput {
    fn current(&self) -> Result<(String, bool), String> {
        Err("Linux audio unavailable".into())
    }
    fn set_muted(&self, _: &str, _: bool) -> Result<(), String> {
        Err("Linux audio unavailable".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Output {
        default: Mutex<String>,
        calls: Mutex<Vec<(String, bool)>>,
        muted: bool,
    }
    impl OutputControl for Output {
        fn current(&self) -> Result<(String, bool), String> {
            Ok((self.default.lock().unwrap().clone(), self.muted))
        }
        fn set_muted(&self, sink: &str, muted: bool) -> Result<(), String> {
            self.calls.lock().unwrap().push((sink.into(), muted));
            Ok(())
        }
    }
    #[test]
    fn restore_original_sink_on_stop_cancel_and_unwind() {
        for failure in [false, true] {
            let output = Arc::new(Output {
                default: Mutex::new("speakers".into()),
                calls: Mutex::default(),
                muted: false,
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _recording = AudioMuteGuard::with_control(output.clone()).unwrap();
                *output.default.lock().unwrap() = "headphones".into();
                if failure {
                    panic!("recording failed");
                }
            }));
            assert_eq!(result.is_err(), failure);
            assert_eq!(
                *output.calls.lock().unwrap(),
                vec![("speakers".into(), true), ("speakers".into(), false)]
            );
        }
    }
    #[test]
    fn already_muted_output_stays_muted() {
        let output = Arc::new(Output {
            default: Mutex::new("speakers".into()),
            calls: Mutex::default(),
            muted: true,
        });
        drop(AudioMuteGuard::with_control(output.clone()).unwrap());
        assert_eq!(
            *output.calls.lock().unwrap(),
            vec![("speakers".into(), true)]
        );
    }
    #[test]
    fn partially_applied_mute_is_restored_when_activation_reports_failure() {
        struct FailingOutput(Mutex<Vec<bool>>);
        impl OutputControl for FailingOutput {
            fn current(&self) -> Result<(String, bool), String> {
                Ok(("speakers".into(), false))
            }
            fn set_muted(&self, _: &str, muted: bool) -> Result<(), String> {
                self.0.lock().unwrap().push(muted);
                if muted {
                    Err("server disconnected after applying mute".into())
                } else {
                    Ok(())
                }
            }
        }
        let output = Arc::new(FailingOutput(Mutex::default()));
        assert!(AudioMuteGuard::with_control(output.clone()).is_err());
        assert_eq!(*output.0.lock().unwrap(), vec![true, false]);
    }
    #[test]
    fn native_state_parsers() {
        assert!(parse_wpctl_muted("Volume: 0.00 [MUTED]"));
        assert!(!parse_wpctl_muted("Volume: 0.82"));
        assert!(parse_pactl_muted("Mute: yes"));
        assert!(!parse_pactl_muted("Mute: no"));
    }
}
