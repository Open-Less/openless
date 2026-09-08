use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilenceDecision {
    Stop,
    Cancel,
}

pub const SPEECH_LEVEL_THRESHOLD: f32 = 0.02;
pub const MIN_SPEECH_BLOCKS: u32 = 3;
pub const NO_SPEECH_CANCEL: Duration = Duration::from_secs(10);

pub struct SilenceAutoStop {
    silence_after_speech: Duration,
    speech_detected: bool,
    consecutive_speech_blocks: u32,
    last_speech_at: Option<Instant>,
    started_at: Instant,
    decided: bool,
}

impl SilenceAutoStop {
    pub fn new(silence_after_speech: Duration, started_at: Instant) -> Self {
        Self {
            silence_after_speech,
            speech_detected: false,
            consecutive_speech_blocks: 0,
            last_speech_at: None,
            started_at,
            decided: false,
        }
    }

    pub fn on_level(&mut self, level: f32, now: Instant) -> Option<SilenceDecision> {
        if self.decided {
            return None;
        }
        if level >= SPEECH_LEVEL_THRESHOLD {
            self.consecutive_speech_blocks += 1;
            if self.consecutive_speech_blocks >= MIN_SPEECH_BLOCKS {
                self.speech_detected = true;
                self.last_speech_at = Some(now);
            }
        } else {
            self.consecutive_speech_blocks = 0;
        }
        let decision = if self.speech_detected {
            self.last_speech_at
                .filter(|last| now.duration_since(*last) >= self.silence_after_speech)
                .map(|_| SilenceDecision::Stop)
        } else if now.duration_since(self.started_at) >= NO_SPEECH_CANCEL {
            Some(SilenceDecision::Cancel)
        } else {
            None
        };
        self.decided = decision.is_some();
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(
        detector: &mut SilenceAutoStop,
        base: Instant,
        frames: &[(f32, Duration)],
    ) -> Option<SilenceDecision> {
        frames
            .iter()
            .find_map(|(level, offset)| detector.on_level(*level, base + *offset))
    }

    #[test]
    fn speech_then_silence_stops_once() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(3), base);
        assert_eq!(
            feed(
                &mut detector,
                base,
                &[
                    (0.1, Duration::from_millis(10)),
                    (0.1, Duration::from_millis(20)),
                    (0.1, Duration::from_millis(30)),
                    (0.0, Duration::from_secs(4)),
                ],
            ),
            Some(SilenceDecision::Stop)
        );
        assert_eq!(detector.on_level(0.0, base + Duration::from_secs(30)), None);
    }

    #[test]
    fn short_silence_does_not_stop() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(3), base);
        assert_eq!(
            feed(
                &mut detector,
                base,
                &[
                    (0.1, Duration::from_millis(10)),
                    (0.1, Duration::from_millis(20)),
                    (0.1, Duration::from_millis(30)),
                    (0.0, Duration::from_secs(2)),
                ],
            ),
            None
        );
    }

    #[test]
    fn no_speech_cancels_after_ten_seconds() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(3), base);
        assert_eq!(
            detector.on_level(0.0, base + Duration::from_secs(10)),
            Some(SilenceDecision::Cancel)
        );
    }

    #[test]
    fn short_noise_burst_is_not_speech() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(3), base);
        assert_eq!(
            feed(
                &mut detector,
                base,
                &[
                    (0.5, Duration::from_millis(10)),
                    (0.0, Duration::from_millis(20)),
                    (0.0, Duration::from_secs(10)),
                ],
            ),
            Some(SilenceDecision::Cancel)
        );
    }

    #[test]
    fn late_speech_switches_to_the_silence_threshold() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(2), base);
        assert_eq!(
            feed(
                &mut detector,
                base,
                &[
                    (0.0, Duration::from_secs(9)),
                    (0.1, Duration::from_millis(9100)),
                    (0.1, Duration::from_millis(9110)),
                    (0.1, Duration::from_millis(9120)),
                    (0.0, Duration::from_millis(11200)),
                ],
            ),
            Some(SilenceDecision::Stop)
        );
    }

    #[test]
    fn two_speech_blocks_are_filtered_as_noise() {
        let base = Instant::now();
        let mut detector = SilenceAutoStop::new(Duration::from_secs(1), base);
        assert_eq!(
            feed(
                &mut detector,
                base,
                &[
                    (0.1, Duration::from_millis(10)),
                    (0.1, Duration::from_millis(20)),
                    (0.0, Duration::from_secs(10)),
                ],
            ),
            Some(SilenceDecision::Cancel)
        );
    }
}
