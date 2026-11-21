//! A single capsule timeline: ribbon -> gathered point -> fluid dots.
//! Shape progress uses monotonic time, not two independently eased clocks.

pub const SHINE_BURST_SECONDS: f64 = 2.0;
pub const SHINE_FAST_CYCLE_SECONDS: f64 = 0.9;
pub const SHINE_SLOW_CYCLE_SECONDS: f64 = 2.4;
const CONTRACT_SECONDS: f64 = 0.7;
const SPREAD_SECONDS: f64 = 0.9;
const MERGE_SECONDS: f64 = 0.45;

fn recording(phase: &str) -> bool {
    matches!(phase, "starting" | "recording")
}

fn terminal(phase: &str) -> bool {
    matches!(
        phase,
        "completed" | "done" | "inserted" | "cancelled" | "failed" | "error"
    )
}

fn ease(age: f64, duration: f64) -> f32 {
    let t = (age / duration).clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, Debug)]
pub struct SiriShape {
    pub resolved: f32,
    pub gather: f32,
}

#[derive(Clone, Debug, Default)]
pub struct CapsuleMotion {
    phase: String,
    phase_at: f64,
    left_recording_at: Option<f64>,
    llm_at: Option<f64>,
    ending: Option<(f64, f32)>,
    recording_resolution: f32,
}

impl CapsuleMotion {
    pub fn update(&mut self, phase: &str, now: f64) -> bool {
        let new_session = recording(phase) && !recording(&self.phase);
        if self.phase == phase {
            return false;
        }
        if new_session {
            self.left_recording_at = None;
            self.llm_at = None;
            self.ending = None;
            self.recording_resolution = 0.0;
        } else if recording(&self.phase) && !recording(phase) {
            self.left_recording_at = Some(now);
        }
        if matches!(phase, "polishing" | "inserting") && self.llm_at.is_none() {
            self.llm_at = Some(now);
        }
        if terminal(phase) && self.ending.is_none() {
            self.ending = Some((now, self.gather(now)));
        }
        self.phase = phase.to_owned();
        self.phase_at = now;
        new_session
    }

    pub fn phase_age(&self, now: f64) -> f64 {
        (now - self.phase_at).max(0.0)
    }

    pub fn shine_cycle_seconds(&self, now: f64) -> f64 {
        if self.phase_age(now) < SHINE_BURST_SECONDS {
            SHINE_FAST_CYCLE_SECONDS
        } else {
            SHINE_SLOW_CYCLE_SECONDS
        }
    }

    fn gather(&self, now: f64) -> f32 {
        if let Some((at, from)) = self.ending {
            return from + (1.0 - from) * ease(now - at, MERGE_SECONDS);
        }
        let Some(llm_at) = self.llm_at else {
            return 1.0;
        };
        let start = self
            .left_recording_at
            .map_or(llm_at, |at| llm_at.max(at + CONTRACT_SECONDS));
        1.0 - ease(now - start, SPREAD_SECONDS)
    }

    pub fn siri_shape(&mut self, now: f64, recording_resolution: f32) -> SiriShape {
        let resolved = if recording(&self.phase) {
            self.recording_resolution = recording_resolution.clamp(0.0, 1.0);
            self.recording_resolution
        } else {
            self.left_recording_at.map_or(0.0, |at| {
                self.recording_resolution * (1.0 - ease(now - at, CONTRACT_SECONDS))
            })
        };
        SiriShape {
            resolved,
            gather: self.gather(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_waits_as_one_point_and_llm_opens_six_dots() {
        let mut motion = CapsuleMotion::default();
        assert!(motion.update("recording", 0.0));
        motion.siri_shape(0.0, 0.8);
        motion.update("transcribing", 1.0);
        assert_eq!(motion.siri_shape(1.0, 0.0).resolved, 0.8);
        assert!((motion.siri_shape(1.35, 0.0).resolved - 0.4).abs() < 0.001);
        let point = motion.siri_shape(10.0, 0.0);
        assert_eq!((point.resolved, point.gather), (0.0, 1.0));
        motion.update("polishing", 10.0);
        assert_eq!(motion.siri_shape(10.0, 0.0).gather, 1.0);
        assert!(motion.siri_shape(10.1, 0.0).gather < 1.0);
        assert_eq!(motion.siri_shape(11.0, 0.0).gather, 0.0);
    }

    #[test]
    fn an_early_llm_cannot_spread_until_contraction_finishes() {
        let mut motion = CapsuleMotion::default();
        motion.update("recording", 0.0);
        motion.siri_shape(0.0, 1.0);
        motion.update("polishing", 1.0);
        assert_eq!(motion.siri_shape(1.4, 0.0).gather, 1.0);
        assert_eq!(motion.siri_shape(1.7, 0.0).resolved, 0.0);
        assert!(motion.siri_shape(1.8, 0.0).gather < 1.0);
    }

    #[test]
    fn ending_merges_continuously_and_a_new_session_resets() {
        let mut motion = CapsuleMotion::default();
        motion.update("polishing", 0.0);
        let before = motion.siri_shape(0.4, 0.0).gather;
        motion.update("completed", 0.4);
        assert_eq!(motion.siri_shape(0.4, 0.0).gather, before);
        assert_eq!(motion.siri_shape(1.0, 0.0).gather, 1.0);
        assert!(motion.update("starting", 2.0));
        assert!(!motion.update("recording", 2.1));
        assert_eq!(motion.siri_shape(2.1, 0.2).resolved, 0.2);
        assert_eq!(motion.siri_shape(2.1, 0.2).gather, 1.0);
    }

    #[test]
    fn the_caption_sweep_retains_its_fast_then_slow_cadence() {
        let mut motion = CapsuleMotion::default();
        motion.update("transcribing", 1.0);
        assert_eq!(motion.shine_cycle_seconds(1.1), SHINE_FAST_CYCLE_SECONDS);
        assert_eq!(motion.shine_cycle_seconds(4.0), SHINE_SLOW_CYCLE_SECONDS);
    }
}
