//! Privacy-gated context and edit observations, bound to the original input
//! identity. Neither a late worker nor a newly focused field can replace it.
use futures_util::future::BoxFuture;
use openless_core::{
    BackendError, BackendErrorCode, EditObservationAdapter, EditObservationSink,
    HostContextAdapter, HostContextCapture,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

pub const CONTEXT_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSnapshot {
    pub version: u32,
    pub target: String,
    #[serde(default)]
    pub application: String,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub cursor: usize,
}

pub trait ContextReader: Send + Sync {
    fn read(
        &self,
        expected: Option<&str>,
        include_text: bool,
    ) -> Result<TargetSnapshot, BackendError>;
}

pub struct NativeContextReader;
impl ContextReader for NativeContextReader {
    fn read(
        &self,
        expected: Option<&str>,
        include_text: bool,
    ) -> Result<TargetSnapshot, BackendError> {
        #[cfg(target_os = "linux")]
        {
            if expected.is_some_and(|s| s.starts_with("atspi:")) {
                return crate::atspi::snapshot(expected, include_text);
            }
            let result = (|| {
                let connection = dbus::blocking::Connection::new_session().map_err(platform)?;
                let proxy = connection.with_proxy(
                    crate::fcitx5::DESTINATION,
                    crate::fcitx5::OBJECT_PATH,
                    std::time::Duration::from_millis(500),
                );
                let (json,): (String,) = proxy
                    .method_call(
                        crate::fcitx5::INTERFACE,
                        "ContextSnapshot",
                        (expected.unwrap_or_default(), include_text),
                    )
                    .map_err(platform)?;
                let snapshot: TargetSnapshot = serde_json::from_str(&json).map_err(platform)?;
                validate_snapshot(&snapshot, expected, include_text)?;
                Ok(snapshot)
            })();
            // Once a native target was bound, never switch bridge or application.
            if expected.is_none()
                && result.as_ref().map_or(true, |s: &TargetSnapshot| {
                    include_text && s.text.is_none() && !s.sensitive
                })
            {
                if let Ok(snapshot) = crate::atspi::snapshot(None, include_text) {
                    return Ok(snapshot);
                }
            }
            result
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (expected, include_text);
            Err(platform("Linux context capture unavailable"))
        }
    }
}

pub fn validate_snapshot(
    snapshot: &TargetSnapshot,
    expected: Option<&str>,
    include_text: bool,
) -> Result<(), BackendError> {
    if snapshot.version != CONTEXT_PROTOCOL_VERSION
        || snapshot.target.is_empty()
        || expected.is_some_and(|e| e != snapshot.target)
    {
        return Err(platform(
            "context target expired or bridge version mismatch",
        ));
    }
    if (snapshot.sensitive || !include_text) && snapshot.text.is_some() {
        return Err(platform("context bridge returned text without permission"));
    }
    if snapshot.text.as_ref().is_some_and(|s| s.len() > 65536) {
        return Err(platform("context exceeds capture limit"));
    }
    Ok(())
}

pub(crate) fn platform(error: impl std::fmt::Display) -> BackendError {
    BackendError::new(BackendErrorCode::Platform, error.to_string())
}

#[derive(Clone)]
pub struct LinuxContextAdapter {
    reader: Arc<dyn ContextReader>,
    target: Arc<Mutex<Option<String>>>,
    generation: Arc<AtomicU64>,
}
impl Default for LinuxContextAdapter {
    fn default() -> Self {
        Self::new(Arc::new(NativeContextReader))
    }
}
impl LinuxContextAdapter {
    pub fn new(reader: Arc<dyn ContextReader>) -> Self {
        Self {
            reader,
            target: Arc::default(),
            generation: Arc::default(),
        }
    }
}
impl HostContextAdapter for LinuxContextAdapter {
    fn capture(
        &self,
        include_cursor: bool,
    ) -> BoxFuture<'static, Result<HostContextCapture, BackendError>> {
        let this = self.clone();
        let generation = {
            let mut target = this.target.lock().unwrap_or_else(|p| p.into_inner());
            let generation = this.generation.fetch_add(1, Ordering::SeqCst) + 1;
            *target = None;
            generation
        };
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let snapshot = match this.reader.read(None, include_cursor) {
                    Ok(snapshot) => snapshot,
                    // Applications may have neither IME surrounding text nor
                    // accessibility support. Dictation itself remains usable.
                    Err(_) => return Ok(HostContextCapture::default()),
                };
                validate_snapshot(&snapshot, None, include_cursor)?;
                let mut target = this.target.lock().unwrap_or_else(|p| p.into_inner());
                if this.generation.load(Ordering::SeqCst) != generation {
                    return Ok(HostContextCapture::default());
                }
                *target = Some(snapshot.target);
                Ok(HostContextCapture {
                    front_app: (!snapshot.application.is_empty()).then_some(snapshot.application),
                    cursor_context: snapshot.text,
                })
            })
            .await
            .map_err(platform)?
        })
    }
}
impl EditObservationAdapter for LinuxContextAdapter {
    fn arm(
        &self,
        typed_text: String,
        sink: Arc<dyn EditObservationSink>,
    ) -> Result<(), BackendError> {
        let (generation, target) = {
            let target = self.target.lock().unwrap_or_else(|p| p.into_inner());
            (
                self.generation.fetch_add(1, Ordering::SeqCst) + 1,
                target.clone(),
            )
        };
        let Some(target) = target else {
            return Ok(());
        };
        let this = self.clone();
        std::thread::Builder::new()
            .name("openless-edit-observer".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                let mut baseline: Option<String> = None;
                while started.elapsed() < std::time::Duration::from_secs(90)
                    && this.generation.load(Ordering::SeqCst) == generation
                {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let Ok(snapshot) = this.reader.read(Some(&target), true) else {
                        break;
                    };
                    if validate_snapshot(&snapshot, Some(&target), true).is_err()
                        || snapshot.sensitive
                    {
                        break;
                    }
                    let Some(text) = snapshot.text else {
                        break;
                    };
                    if this.generation.load(Ordering::SeqCst) != generation {
                        break;
                    }
                    match baseline.as_ref() {
                        None if text.contains(&typed_text) && !typed_text.is_empty() => {
                            baseline = Some(text)
                        }
                        Some(before) if before != &text => {
                            if let Some(edit) =
                                openless_core::host_document::minimal_edit(before, &text)
                            {
                                if sink.publish(edit) {
                                    baseline = Some(text);
                                }
                            }
                        }
                        _ => (),
                    }
                }
            })
            .map_err(platform)?;
        Ok(())
    }
    fn disarm(&self) {
        let _guard = self.target.lock().unwrap_or_else(|p| p.into_inner());
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn late_capture_cannot_replace_a_newer_target() {
        struct Reader {
            count: AtomicU64,
            started: std::sync::mpsc::Sender<()>,
            release: Mutex<std::sync::mpsc::Receiver<()>>,
        }
        impl ContextReader for Reader {
            fn read(&self, _: Option<&str>, _: bool) -> Result<TargetSnapshot, BackendError> {
                let old = self.count.fetch_add(1, Ordering::SeqCst) == 0;
                if old {
                    self.started.send(()).unwrap();
                    self.release.lock().unwrap().recv().unwrap();
                }
                let name = if old { "old" } else { "new" };
                Ok(TargetSnapshot {
                    version: 1,
                    target: name.into(),
                    application: name.into(),
                    ..Default::default()
                })
            }
        }
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let adapter = LinuxContextAdapter::new(Arc::new(Reader {
            count: AtomicU64::new(0),
            started: started_tx,
            release: Mutex::new(release_rx),
        }));
        let first = tokio::spawn(adapter.capture(false));
        tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
            .await
            .unwrap();
        assert_eq!(
            adapter.capture(false).await.unwrap().front_app.as_deref(),
            Some("new")
        );
        release_tx.send(()).unwrap();
        assert!(first.await.unwrap().unwrap().front_app.is_none());
        assert_eq!(adapter.target.lock().unwrap().as_deref(), Some("new"));
    }
    #[test]
    fn mismatched_target_version_and_privacy_are_rejected() {
        let mut s = TargetSnapshot {
            version: 1,
            target: "original".into(),
            ..Default::default()
        };
        assert!(validate_snapshot(&s, Some("new-target"), true).is_err());
        s.version = 2;
        assert!(validate_snapshot(&s, None, true).is_err());
        s.version = 1;
        s.text = Some("private".into());
        assert!(validate_snapshot(&s, None, false).is_err());
        s.sensitive = true;
        assert!(validate_snapshot(&s, None, true).is_err());
    }
}
