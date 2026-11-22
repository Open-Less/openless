//! Local, opt-in diagnostics. Only run-local recording numbers leave this module.

use crate::{BackendEvent, BackendEventKind, SessionId};
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const RECENT_RECORDINGS: usize = 4;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionTraceStage {
    Accepted,
    Rejected,
    RenderCallback,
}

#[derive(Clone, Copy)]
pub enum CaptionTraceSurface {
    Capsule,
    Rail,
}

#[derive(Default)]
pub(crate) struct MainDictationTrace(OnceLock<Mutex<Run>>);

struct Run {
    token: u64,
    started: Instant,
    recordings: VecDeque<Recording>,
}

struct Recording {
    id: SessionId,
    number: u64,
    samples: [Option<Instant>; 8],
}

impl MainDictationTrace {
    pub(crate) fn enable(&self) {
        self.0.get_or_init(|| {
            Mutex::new(Run {
                token: uuid::Uuid::new_v4().as_u128() as u64,
                started: Instant::now(),
                recordings: VecDeque::with_capacity(RECENT_RECORDINGS),
            })
        });
    }

    pub(crate) fn enabled(&self) -> bool {
        self.0.get().is_some()
    }

    pub(crate) fn begin(&self, id: SessionId) {
        let Some(mut run) = self.0.get().and_then(|run| run.lock().ok()) else {
            return;
        };
        let previous = run
            .recordings
            .back()
            .map_or(0, |recording| recording.number);
        let number = previous + 1;
        if run.recordings.len() == RECENT_RECORDINGS {
            run.recordings.pop_front();
        }
        run.recordings.push_back(Recording {
            id,
            number,
            samples: [None; 8],
        });
        run.log(number, format_args!("stage=begin previous={previous}"));
    }

    pub(crate) fn app_route(&self, event: &BackendEvent, emitted: bool) {
        let Some(id) = event.session_id else { return };
        if let BackendEventKind::TranscriptDelta(delta) = &event.kind {
            self.sample(
                id,
                usize::from(delta.is_final),
                format_args!(
                    "stage=app_route sequence={} final={} emitted={emitted}",
                    event.sequence, delta.is_final
                ),
            );
        }
    }

    pub(crate) fn webview(
        &self,
        id: SessionId,
        sequence: u64,
        surface: CaptionTraceSurface,
        stage: CaptionTraceStage,
    ) {
        if sequence == 0 {
            return;
        }
        let (base, surface) = match surface {
            CaptionTraceSurface::Capsule => (2, "capsule"),
            CaptionTraceSurface::Rail => (5, "rail"),
        };
        let (offset, stage) = match stage {
            CaptionTraceStage::Accepted => (0, "webview_accepted"),
            CaptionTraceStage::Rejected => (1, "webview_rejected"),
            CaptionTraceStage::RenderCallback => (2, "render_callback"),
        };
        self.sample(
            id,
            base + offset,
            format_args!("stage={stage} surface={surface} sequence={sequence}"),
        );
    }

    pub(crate) fn previous(&self, id: SessionId) -> Option<Option<SessionId>> {
        let run = self.0.get()?.lock().ok()?;
        let index = run
            .recordings
            .iter()
            .position(|recording| recording.id == id)?;
        Some(
            index
                .checked_sub(1)
                .map(|previous| run.recordings[previous].id),
        )
    }

    pub(crate) fn record(&self, id: SessionId, message: std::fmt::Arguments<'_>) {
        let Some(run) = self.0.get().and_then(|run| run.lock().ok()) else {
            return;
        };
        if let Some(recording) = run.recordings.iter().find(|recording| recording.id == id) {
            run.log(recording.number, message);
        }
    }

    fn sample(&self, id: SessionId, slot: usize, message: std::fmt::Arguments<'_>) {
        let Some(mut run) = self.0.get().and_then(|run| run.lock().ok()) else {
            return;
        };
        let Some(recording) = run
            .recordings
            .iter_mut()
            .find(|recording| recording.id == id)
        else {
            return;
        };
        let last = &mut recording.samples[slot];
        let now = Instant::now();
        if last.is_some_and(|last| now.duration_since(last) < SAMPLE_INTERVAL) {
            return;
        }
        *last = Some(now);
        let number = recording.number;
        run.log(number, message);
    }
}

impl Run {
    fn log(&self, number: u64, message: std::fmt::Arguments<'_>) {
        log::info!(
            "[main-dictation-trace] run={:016x} recording={number} elapsed_ms={} {message}",
            self.token,
            self.started.elapsed().as_millis()
        );
    }
}
