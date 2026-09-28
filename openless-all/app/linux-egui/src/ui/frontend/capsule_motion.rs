//! 胶囊相位时序：状态切换时的入场 / 收尾节奏，逐项对齐 Tauri。
//!
//! Tauri 那边这些节奏由 CSS / React 生命周期承担（`cap-shine` 的 0.9s→2.4s burst、
//! `VoiceOrbStage` 里 wave 的 `opacity .6s ease-out .55s`）。egui 没有 CSS，节奏只能自己
//! 算，所以集中在这里并配单测 —— 这样「和 Tauri 不一致」是能被断言发现的，而不是靠肉眼。

/// 经典药丸 thinking 文案的扫光：进入转写/润色的头 2 秒走快速 burst（Tauri 用 0.9s/周期
/// 提示「流式刚开始」），之后回落到 2.4s 的稳态。切回其它状态也复位成快速，下次重新 burst。
pub const SHINE_BURST_SECONDS: f64 = 2.0;
pub const SHINE_FAST_CYCLE_SECONDS: f64 = 0.9;
pub const SHINE_SLOW_CYCLE_SECONDS: f64 = 2.4;

/// wave → orb 的交叉淡出（Tauri `opacity .6s ease-out .55s`）：切态后先保持可见 0.55s，
/// 再用 0.6s 淡出；Tauri 另外在 1.3s 后卸载 canvas，这里淡完就不再画（等价且更省）。
pub const WAVE_FADE_DELAY_SECONDS: f64 = 0.55;
pub const WAVE_FADE_SECONDS: f64 = 0.6;

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
            // 刚从录音切走：wave 的淡出从这一刻起算。
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

    /// wave 在本帧的整体不透明度：`None` = 不用再画（思考态且淡出已结束）。
    pub fn wave_opacity(&self, now: f64) -> Option<f32> {
        let left_at = self.recording_left_at?;
        let age = now - left_at;
        if age < WAVE_FADE_DELAY_SECONDS {
            return Some(1.0);
        }
        let elapsed = age - WAVE_FADE_DELAY_SECONDS;
        if elapsed >= WAVE_FADE_SECONDS {
            return None;
        }
        let t = (elapsed / WAVE_FADE_SECONDS).clamp(0.0, 1.0) as f32;
        // CSS `ease-out` 的近似：起步快、收尾慢。
        let eased = 1.0 - (1.0 - t).powi(3);
        Some(1.0 - eased)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_stays_opaque_during_the_fade_delay() {
        let mut motion = CapsuleMotion::default();
        motion.update("recording", 0.0);
        motion.update("polishing", 1.0);
        assert_eq!(motion.wave_opacity(1.0), Some(1.0));
        assert_eq!(motion.wave_opacity(1.5), Some(1.0));
    }

    #[test]
    fn the_wave_fade_covers_half_a_second_and_then_stops() {
        let mut motion = CapsuleMotion::default();
        motion.update("recording", 0.0);
        motion.update("polishing", 10.0);
        // 延迟期内完全不透明。
        assert_eq!(motion.wave_opacity(10.3), Some(1.0));
        assert_eq!(motion.wave_opacity(10.55), Some(1.0));
        // 淡出中：单调下降、且始终落在 (0, 1) 之间。
        let quarter = motion.wave_opacity(10.55 + 0.15).unwrap();
        let half = motion.wave_opacity(10.55 + 0.3).unwrap();
        let late = motion.wave_opacity(10.55 + 0.55).unwrap();
        assert!(
            quarter < 1.0 && quarter > half && half > late && late > 0.0,
            "{quarter} {half} {late}"
        );
        // 淡完就不再画。
        assert_eq!(motion.wave_opacity(10.55 + WAVE_FADE_SECONDS), None);
        assert_eq!(motion.wave_opacity(12.0), None);
    }

    #[test]
    fn no_fade_is_scheduled_without_a_recording_phase() {
        let mut motion = CapsuleMotion::default();
        // 直接进思考态（例如错过了录音相位）：没有 wave 可淡出。
        motion.update("polishing", 5.0);
        assert_eq!(motion.wave_opacity(5.0), None);
        assert_eq!(motion.wave_opacity(9.0), None);
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
