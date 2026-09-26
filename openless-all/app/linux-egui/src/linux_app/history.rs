use super::*;

/// One read at a time; the last successful result survives failures and refreshes.
#[derive(Default)]
pub(super) struct HistoryCache {
    pub(super) entries: Vec<openless_core::DictationSession>,
    pub(super) error: Option<String>,
    policy: Option<(u32, Option<u32>, Option<u32>)>,
    revision: u64,
    wanted_revision: Option<u64>,
    generation: u64,
    in_flight: Option<(u64, u64)>,
    pending: bool,
}

impl HistoryCache {
    pub(super) fn new(preferences: &UserPreferences) -> Self {
        let mut cache = Self::default();
        cache.policy_changed(preferences);
        cache
    }

    pub(super) fn policy_changed(&mut self, preferences: &UserPreferences) -> bool {
        let policy = (
            preferences.history_retention_days,
            preferences.history_max_entries,
            preferences.audio_recording_max_entries,
        );
        self.policy
            .replace(policy)
            .is_some_and(|previous| previous != policy)
    }

    pub(super) fn observe(&mut self, revision: u64) {
        if self.wanted_revision.is_none_or(|wanted| revision > wanted) {
            self.request(revision);
        }
    }

    pub(super) fn request(&mut self, revision: u64) {
        self.wanted_revision = Some(self.wanted_revision.unwrap_or(0).max(revision));
        self.generation += 1;
        self.pending = true;
    }

    pub(super) fn loading(&self) -> bool {
        self.pending || self.in_flight.is_some()
    }

    fn begin(&mut self) -> Option<(u64, u64)> {
        if !self.pending || self.in_flight.is_some() {
            return None;
        }
        self.pending = false;
        let request = (self.generation, self.wanted_revision.unwrap_or(0));
        self.in_flight = Some(request);
        Some(request)
    }

    pub(super) fn finish(
        &mut self,
        generation: u64,
        revision: u64,
        result: Result<Vec<openless_core::DictationSession>, String>,
    ) -> bool {
        if self.in_flight != Some((generation, revision)) {
            return false;
        }
        self.in_flight = None;
        if generation != self.generation || revision < self.revision {
            return false;
        }
        match result {
            Ok(entries) => {
                self.entries = entries;
                self.revision = revision;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
}

impl OpenLessEguiApp {
    pub(super) fn load_history(&mut self) {
        let Some(backend) = self.backend() else {
            return;
        };
        let Some((generation, revision)) = self.history.begin() else {
            return;
        };
        spawn_history_read(
            &self.tokio,
            self.tx.clone(),
            generation,
            revision,
            move || {
                let mut entries = backend.list_history().map_err(|error| error.to_string())?;
                for entry in &mut entries {
                    entry.has_audio_recording = Some(
                        entry.has_audio_recording.unwrap_or(false)
                            || openless_linux_egui::recording_path(
                                &backend.config().data_dir,
                                &entry.id,
                            )
                            .is_ok_and(|path| path.exists()),
                    );
                }
                Ok(entries)
            },
        );
    }
}

fn spawn_history_read(
    runtime: &tokio::runtime::Runtime,
    tx: mpsc::Sender<UiResult>,
    generation: u64,
    revision: u64,
    read: impl FnOnce() -> Result<Vec<openless_core::DictationSession>, String> + Send + 'static,
) {
    runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(read)
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        let _ = tx.send(UiResult::HistoryLoaded {
            generation,
            revision,
            result,
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_ticks_do_not_read_and_refreshes_coalesce_while_blocked() {
        let mut cache = HistoryCache::default();
        cache.observe(0);
        let first = cache.begin().unwrap();
        for _ in 0..1000 {
            cache.observe(0);
            assert!(cache.begin().is_none());
        }
        cache.observe(1);
        cache.observe(2);
        cache.request(2);
        assert!(!cache.finish(first.0, first.1, Ok(Vec::new())));
        let latest = cache.begin().unwrap();
        assert_eq!(latest.1, 2);
        assert!(!cache.finish(first.0, first.1, Err("stale".into())));
        assert!(cache.finish(latest.0, latest.1, Ok(Vec::new())));
        for _ in 0..1000 {
            cache.observe(2);
            assert!(cache.begin().is_none());
        }
        cache.request(2);
        let refresh = cache.begin().unwrap();
        assert!(!cache.finish(refresh.0, refresh.1, Err("disk unavailable".into())));
        assert!(cache.error.is_some());
        cache.observe(2);
        assert!(
            cache.begin().is_none(),
            "failed reads must wait for an explicit refresh or new revision"
        );
        cache.observe(3);
        assert!(cache.begin().is_some());
    }
    #[test]
    fn a_blocked_disk_read_does_not_block_host_messages_or_spawn_more_reads() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut cache = HistoryCache::default();
        cache.observe(1);
        let (generation, revision) = cache.begin().unwrap();
        let count = reads.clone();
        spawn_history_read(&runtime, tx.clone(), generation, revision, move || {
            count.fetch_add(1, Ordering::SeqCst);
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(Vec::new())
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..1000 {
            cache.observe(1);
            assert!(cache.begin().is_none());
        }
        tx.send(UiResult::Message("host still handles actions".into()))
            .unwrap();
        assert!(matches!(rx.try_recv(), Ok(UiResult::Message(_))));
        release_tx.send(()).unwrap();
        match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
            UiResult::HistoryLoaded {
                generation,
                revision,
                result,
            } => {
                assert!(cache.finish(generation, revision, result));
            }
            _ => panic!("expected the one history read"),
        }
        for _ in 0..1000 {
            cache.observe(1);
            assert!(cache.begin().is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}
