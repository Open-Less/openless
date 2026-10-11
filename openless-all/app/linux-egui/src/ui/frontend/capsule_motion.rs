//! Capsule phase timing: keep Tauri's caption sweep while sequencing the Siri
//! ribbon, single-point contraction and six-point LLM stage without overlap.

/// 经典药丸 thinking 文案的扫光：进入转写/润色的头 2 秒走快速 burst（Tauri 用 0.9s/周期
/// 提示「流式刚开始」），之后回落到 2.4s 的稳态。切回其它状态也复位成快速，下次重新 burst。
pub const SHINE_BURST_SECONDS: f64 = 2.0;
pub const SHINE_FAST_CYCLE_SECONDS: f64 = 0.9;
pub const SHINE_SLOW_CYCLE_SECONDS: f64 = 2.4;

/// Contract the recording ribbon completely before handing off to six dots.
pub const WAVE_COLLAPSE_SECONDS: f64 = 0.55;

/// 录音相位：只有它画满幅的声波。
fn phase_is_recording(phase: &str) -> bool {
    matches!(phase, "starting" | "recording")
}

#[derive(Debug, Clone, Default)]
pub struct CapsuleMotion {
    phase: String,
    /// 当前相位是什么时候开始的（胶囊窗口自己的单调时钟，秒）。
    phase_at: f64,
    /// 离开录音相位的时刻；`None` = 当前还在录音 / 本次会话没经过录音。
    recording_left_at: Option<f64>,
    primed: bool,
}

impl CapsuleMotion {
    /// 推进一帧，返回 `true` 表示这是一个**新会话刚进入录音/预备相位**（调用方据此复位
    /// Siri 时钟，让入场动画从头走一遍 —— Tauri 靠组件重新挂载达到同样效果）。
    pub fn update(&mut self, phase: &str, now: f64) -> bool {
        let new_session = phase_is_recording(phase) && !phase_is_recording(&self.phase);
        if self.primed && self.phase == phase {
            return false;
        }
        if phase_is_recording(phase) {
            self.recording_left_at = None;
        } else if phase_is_recording(&self.phase) {
            // Start contraction when recording ends, not when LLM processing starts.
            self.recording_left_at = Some(now);
        }
        self.phase = phase.to_string();
        self.phase_at = now;
        self.primed = true;
        new_session
    }

    /// 当前相位已经持续了多久（秒）。
    pub fn phase_age(&self, now: f64) -> f64 {
        (now - self.phase_at).max(0.0)
    }

    /// 经典药丸 thinking 扫光的本帧周期（秒）。
    pub fn shine_cycle_seconds(&self, now: f64) -> f64 {
        if self.phase_age(now) < SHINE_BURST_SECONDS {
            SHINE_FAST_CYCLE_SECONDS
        } else {
            SHINE_SLOW_CYCLE_SECONDS
        }
    }

    /// Spatial contraction, not an opacity fade. The collapsed wave remains a
    /// single bright point throughout transcription, however long it takes.
    pub fn wave_resolution(&self, now: f64) -> f32 {
        let Some(left_at) = self.recording_left_at else {
            return 0.0;
        };
        let t = ((now - left_at) / WAVE_COLLAPSE_SECONDS).clamp(0.0, 1.0) as f32;
        1.0 - t * t * (3.0 - 2.0 * t)
    }

    /// Enter the six-point effect only in the LLM/output stage and only after
    /// contraction has finished. Never queue both effects in the same frame.
    pub fn orb_ready(&self, now: f64) -> bool {
        matches!(
            self.phase.as_str(),
            "polishing"
                | "inserting"
                | "completed"
                | "done"
                | "inserted"
                | "cancelled"
                | "failed"
                | "error"
        ) && self.wave_resolution(now) == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_holds_one_point_until_the_llm_stage() {
        let mut motion = CapsuleMotion::default();
        motion.update("recording", 0.0);
        motion.update("transcribing", 1.0);
        assert_eq!(motion.wave_resolution(1.0), 1.0);
        let halfway = motion.wave_resolution(1.0 + WAVE_COLLAPSE_SECONDS / 2.0);
        assert!((halfway - 0.5).abs() < 0.001);
        assert_eq!(motion.wave_resolution(2.0), 0.0);
        assert!(!motion.orb_ready(20.0));
        motion.update("polishing", 20.0);
        assert!(motion.orb_ready(20.0));
    }

    #[test]
    fn a_fast_llm_stage_cannot_overlap_the_collapsing_wave() {
        let mut motion = CapsuleMotion::default();
        motion.update("recording", 0.0);
        motion.update("polishing", 1.0);
        assert!(!motion.orb_ready(1.0));
        assert!(!motion.orb_ready(1.2));
        assert!(motion.orb_ready(2.0));
    }

    #[test]
    fn a_popup_opened_mid_polish_starts_with_the_gathered_orb() {
        let mut motion = CapsuleMotion::default();
        motion.update("polishing", 5.0);
        assert_eq!(motion.wave_resolution(5.0), 0.0);
        assert!(motion.orb_ready(5.0));
    }

    #[test]
    fn a_new_recording_phase_restarts_the_session() {
        let mut motion = CapsuleMotion::default();
        assert!(
            motion.update("recording", 0.0),
            "first recording is a session"
        );
        assert!(!motion.update("recording", 0.1));
        assert!(!motion.update("polishing", 0.2));
        assert!(!motion.update("completed", 3.0));
        // 回到录音：新一轮会话。
        assert!(motion.update("recording", 5.0));
        assert_eq!(motion.phase_age(5.5), 0.5);
        // `starting` 也算预备态：同一轮里 starting → recording 不算新会话。
        assert!(!motion.update("recording", 5.6));
        assert!(!motion.update("starting", 6.0));
    }

    #[test]
    fn the_thinking_sweep_starts_fast_and_settles_down() {
        let mut motion = CapsuleMotion::default();
        motion.update("polishing", 100.0);
        assert_eq!(motion.shine_cycle_seconds(100.0), SHINE_FAST_CYCLE_SECONDS);
        assert_eq!(motion.shine_cycle_seconds(101.9), SHINE_FAST_CYCLE_SECONDS);
        assert_eq!(motion.shine_cycle_seconds(102.0), SHINE_SLOW_CYCLE_SECONDS);
        assert_eq!(motion.shine_cycle_seconds(140.0), SHINE_SLOW_CYCLE_SECONDS);
        // 切相位会重新 burst。
        motion.update("recording", 200.0);
        assert_eq!(motion.shine_cycle_seconds(200.1), SHINE_FAST_CYCLE_SECONDS);
    }
}
