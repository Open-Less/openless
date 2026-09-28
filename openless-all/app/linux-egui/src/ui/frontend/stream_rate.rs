//! 流式输出速率 → 思考光点转速。
//!
//! 用户诉求：润色时圆点要「跟着大模型的吐字速度转」——输出快就转得快，模型卡住就慢下来。
//! Tauri 版没有速率反馈（`VoiceOrbStage` 只按状态给固定 `speed`：thinking 1.5 / 收尾 1.0），
//! 所以这里的基线刻意定成 Tauri 的 1.5：**没有速率数据时观感与 Tauri 完全一致**，只有真的
//! 收到流式增量（Core 的 `PolishDelta` / `TranscriptDelta`）才会往上加。
//!
//! 两端分工：
//!   · 宿主（`main.rs`）用 [`StreamRate`] 把增量字符数按滑动窗口折算成「字符/秒」下发；
//!   · 胶囊窗口用 [`OrbSpeed`] 接住这个速率、把时间戳记成「刚收到」，再本地按时间衰减 ——
//!     流一停（网关卡住 / 模型想事情）就慢慢回到基线转速，不需要宿主额外发心跳。

use std::collections::VecDeque;

/// 采样窗口：只统计最近这么多秒内收到的字符。
const WINDOW_SECONDS: f64 = 0.5;
/// 低于该速率不加成：输出很慢时保持基线转速，避免圆点「抖一下快一下」。
const RATE_FLOOR: f32 = 4.0;
/// 达到该速率即给到最高转速（中文 40 字/秒、英文 100+ 字符/秒已经算飞快）。
const RATE_AT_MAX: f32 = 60.0;
/// 基线转速：Tauri `VoiceOrbStage` thinking 态的 `speed={1.5}`。
pub const BASE_SPEED: f32 = 1.5;
/// 最高转速。再多也只到这里，免得圆点转成电风扇。
pub const MAX_SPEED: f32 = 2.6;
/// 速率不再更新后的衰减时间常数（秒）。
const DECAY_SECONDS: f64 = 0.45;
/// 转速跟随速率的平滑系数（越大越跟手；1.5 是 Tauri 里 `speed` 的固有平滑）。
const SPEED_SMOOTHING: f32 = 6.0;
/// 速率被视为「变了」的最小差值：宿主每条增量都会重算，这条只是防止浮点噪声。
const RATE_EPSILON: f32 = 1e-4;

/// 速率 → 转速。没有数据 / 数据过小时返回 [`BASE_SPEED`]，高到 [`MAX_SPEED`] 封顶。
pub fn speed_for_rate(rate: f32) -> f32 {
    if !rate.is_finite() || rate <= RATE_FLOOR {
        return BASE_SPEED;
    }
    let t = ((rate - RATE_FLOOR) / (RATE_AT_MAX - RATE_FLOOR)).clamp(0.0, 1.0);
    BASE_SPEED + (MAX_SPEED - BASE_SPEED) * t
}

/// 宿主侧：把流式增量的字符数折算成「字符/秒」。
#[derive(Debug, Clone, Default)]
pub struct StreamRate {
    /// `(时刻秒, 字符数)`；窗口外的样本在读取时丢弃。
    samples: VecDeque<(f64, usize)>,
}

impl StreamRate {
    /// 记下一段流式增量。`now` 是宿主自己的单调时钟（秒）。
    pub fn observe(&mut self, now: f64, chars: usize) {
        if chars == 0 {
            return;
        }
        self.samples.push_back((now, chars));
    }

    /// 便捷入口：直接数一段文本的字符数（增量文本通常是 UTF-8 片段，按字符计更稳定）。
    pub fn observe_text(&mut self, now: f64, text: &str) {
        self.observe(now, text.chars().count());
    }

    /// 当前速率（字符/秒）。窗口内没有样本就是 0，调用方据此回落基线转速。
    pub fn rate(&mut self, now: f64) -> f32 {
        self.prune(now);
        let chars: usize = self.samples.iter().map(|(_, chars)| *chars).sum();
        if chars == 0 {
            return 0.0;
        }
        chars as f32 / WINDOW_SECONDS as f32
    }

    /// 新会话 / 新阶段开始时清空，避免上一轮的样本把首帧抬起来。
    pub fn reset(&mut self) {
        self.samples.clear();
    }

    fn prune(&mut self, now: f64) {
        while let Some((at, _)) = self.samples.front() {
            if now - *at > WINDOW_SECONDS {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }
}

/// 胶囊窗口侧：接住宿主下发的速率，并在没有新数据时按时间衰减回基线。
#[derive(Debug, Clone, Default)]
pub struct OrbSpeed {
    /// 最近一次收到的速率；`None` = 还没有过数据。
    payload: Option<f32>,
    /// 速率上一次发生变化的时刻（秒，胶囊窗口自己的单调时钟）。
    updated_at: f64,
    /// 当前平滑后的转速。
    speed: f32,
    /// 是否已经有初值（首帧直接落在目标上，避免入场时从 0 往上爬）。
    primed: bool,
}

impl OrbSpeed {
    /// 喂入宿主下发的速率并推进一帧，返回本帧该用的转速。
    ///
    /// 只有速率**变化**时才刷新时间戳：宿主每条流式增量都会重算速率，数值几乎每次都会
    /// 变，所以「不变」基本等于流停了；一直一模一样（例如被 clamp 在顶）时本地衰减会略微
    /// 回退，收到下一条又会跟上，观感上察觉不到。
    pub fn tick(&mut self, now: f64, dt: f32, payload: Option<f32>) -> f32 {
        let current = match payload {
            Some(rate) if rate.is_finite() => rate.max(0.0),
            _ => 0.0,
        };
        let changed = match self.payload {
            Some(previous) => (previous - current).abs() > RATE_EPSILON,
            None => true,
        };
        if changed {
            self.updated_at = now;
        }
        self.payload = Some(current);

        let age = (now - self.updated_at).max(0.0);
        let decayed = current as f64 * (-age / DECAY_SECONDS).exp();
        let target = speed_for_rate(decayed as f32);
        if !self.primed {
            self.speed = target;
            self.primed = true;
            return self.speed;
        }
        let dt = dt.clamp(0.0, 0.1);
        self.speed += (target - self.speed) * (1.0 - (-dt * SPEED_SMOOTHING).exp());
        self.speed
    }

    /// 离开思考态时复位，下次进入从头开始（先基线，再随增量爬升）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// 进程内单调时钟（秒）。宿主与胶囊窗口各自在自己的进程里用它对齐 `observe` / `tick`。
pub fn now_seconds() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_meter_reports_no_rate() {
        let mut meter = StreamRate::default();
        assert_eq!(meter.rate(0.0), 0.0);
    }

    #[test]
    fn the_rate_counts_characters_inside_the_window() {
        let mut meter = StreamRate::default();
        // 窗口 0.5s：0.0s 收到 10 字、0.1s 收到 10 字 → 20 字 / 0.5s = 40 字每秒。
        meter.observe(0.0, 10);
        meter.observe(0.1, 10);
        assert!((meter.rate(0.1) - 40.0).abs() < 0.01);
        // 窗口外的样本过期：0.6s 时只剩第二笔。
        assert!((meter.rate(0.6) - 20.0).abs() < 0.01);
    }

    #[test]
    fn text_is_counted_in_characters_not_bytes() {
        let mut meter = StreamRate::default();
        meter.observe_text(0.0, "润色结果");
        assert!((meter.rate(0.0) - 8.0).abs() < 0.01);
    }

    #[test]
    fn stale_samples_expire_and_reset_clears_everything() {
        let mut meter = StreamRate::default();
        meter.observe(0.0, 100);
        assert_eq!(meter.rate(2.0), 0.0);
        meter.observe(2.0, 100);
        meter.reset();
        assert_eq!(meter.rate(2.0), 0.0);
    }

    #[test]
    fn speed_without_data_matches_the_tauri_baseline() {
        let mut orb = OrbSpeed::default();
        for frame in 0..30 {
            let now = 0.1 + f64::from(frame) * 0.016;
            let speed = orb.tick(now, 0.016, None);
            assert!((speed - BASE_SPEED).abs() < 1e-6);
        }
    }

    #[test]
    fn speed_rises_with_the_rate_and_is_clamped() {
        assert_eq!(speed_for_rate(0.0), BASE_SPEED);
        assert_eq!(speed_for_rate(RATE_FLOOR), BASE_SPEED);
        assert_eq!(speed_for_rate(1_000.0), MAX_SPEED);
        let slow = speed_for_rate(15.0);
        let fast = speed_for_rate(45.0);
        assert!(
            slow > BASE_SPEED && slow < fast && fast < MAX_SPEED,
            "{slow} {fast}"
        );
    }

    #[test]
    fn speed_decays_back_to_the_baseline_when_the_stream_stalls() {
        let mut orb = OrbSpeed::default();
        let mut now = 0.0;
        // 一段高速输出：转速爬上去。
        for _ in 0..40 {
            // 每帧速率都略有变化，模拟宿主每条增量重算出来的值。
            now += 0.016;
            orb.tick(now, 0.016, Some(55.0 + (now * 10.0).sin() as f32));
        }
        let hot = orb.speed;
        assert!(hot > BASE_SPEED + 0.2, "expected a visible spin-up: {hot}");
        // 流停了：不再有新速率，转速应该慢慢回到基线。
        for _ in 0..200 {
            now += 0.016;
            orb.tick(now, 0.016, None);
        }
        assert!((orb.speed - BASE_SPEED).abs() < 0.02, "speed={}", orb.speed);
        // 复位后立刻回到基线（下次思考从头开始）。
        orb.reset();
        assert_eq!(orb.tick(now, 0.016, None), BASE_SPEED);
    }

    #[test]
    fn a_fresh_rate_after_a_stall_climbs_again() {
        let mut orb = OrbSpeed::default();
        let mut now = 0.0;
        for _ in 0..30 {
            now += 0.016;
            orb.tick(now, 0.016, Some(50.0));
        }
        for _ in 0..60 {
            now += 0.016;
            orb.tick(now, 0.016, Some(50.0));
        }
        let after_stall = orb.speed;
        for _ in 0..30 {
            now += 0.016;
            orb.tick(now, 0.016, Some(50.0 + (now * 7.0).sin() as f32));
        }
        assert!(orb.speed > after_stall, "{} -> {}", after_stall, orb.speed);
    }
}
