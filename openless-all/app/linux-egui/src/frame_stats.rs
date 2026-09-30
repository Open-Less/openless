//! 帧率探针（QA 用，默认关闭）。
//!
//! `OPENLESS_FPS_LOG=1` 时每个渲染进程每秒往日志写一行帧统计（帧数、fps、最慢一帧），
//! 用来现场回答「这台机器能跑到多少帧」。`OPENLESS_FPS_SETTINGS=1` 额外让主窗口
//! 启动时就打开设置面板，直接测最重的那条路径（磨砂背板：离屏 4×MSAA 重绘 + 高斯模糊）。
//!
//! 两个开关都只读环境变量；未开启时每次调用只做一次 `OnceLock` 读，日志一条不发。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 统计窗口：每满这么久写一行。1s 足够稳，又能在一次测量里拿到多行。
const REPORT_WINDOW: Duration = Duration::from_secs(1);

const FPS_LOG_ENV: &str = "OPENLESS_FPS_LOG";
const SETTINGS_ENV: &str = "OPENLESS_FPS_SETTINGS";

struct Window {
    label: &'static str,
    started: Instant,
    last: Instant,
    frames: u32,
    worst: Duration,
}

impl Window {
    fn new(label: &'static str, now: Instant) -> Self {
        Self {
            label,
            started: now,
            last: now,
            frames: 0,
            worst: Duration::ZERO,
        }
    }
}

/// 记一帧；跨过 [`REPORT_WINDOW`] 时返回 `(帧数, 窗口时长, 最慢一帧)` 并重置窗口。
fn record(window: &mut Window, now: Instant) -> Option<(u32, Duration, Duration)> {
    window.worst = window.worst.max(now.saturating_duration_since(window.last));
    window.last = now;
    window.frames += 1;
    let elapsed = now.saturating_duration_since(window.started);
    if elapsed < REPORT_WINDOW {
        return None;
    }
    let report = (window.frames, elapsed, window.worst);
    *window = Window::new(window.label, now);
    Some(report)
}

fn env_flag(name: &str) -> bool {
    matches!(std::env::var(name).as_deref(), Ok("1") | Ok("true"))
}

/// 帧率日志是否开启。
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_flag(FPS_LOG_ENV))
}

/// 启动即打开设置面板（只影响测量环境）。
pub fn settings_open_on_start() -> bool {
    env_flag(SETTINGS_ENV)
}

/// 每个渲染帧调用一次。`label` 区分进程（主窗 / 弹窗 / 胶囊）。
///
/// 每个进程只有一个渲染面，所以内部只保留一份窗口；`label` 变了就重新开始统计。
pub fn tick(label: &'static str) {
    if !enabled() {
        return;
    }
    let now = Instant::now();
    let mut slot = WINDOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let stale = !matches!(slot.as_ref(), Some(window) if window.label == label);
    if stale {
        *slot = Some(Window::new(label, now));
    }
    let Some(window) = slot.as_mut() else {
        return;
    };
    if let Some((frames, elapsed, worst)) = record(window, now) {
        let message = format!(
            "[fps] {label}: {frames} frames / {:.0}ms = {:.1} fps, worst frame {:.1}ms",
            elapsed.as_secs_f64() * 1000.0,
            frames as f64 / elapsed.as_secs_f64(),
            worst.as_secs_f64() * 1000.0,
        );
        // 胶囊走原生 layer-shell 路径时不装文件日志器，`log::info!` 会被直接丢弃；
        // 那种情况下探针退回 stderr，保证任何启动方式都能拿到数字。
        if log::log_enabled!(log::Level::Info) {
            log::info!("{message}");
        } else {
            eprintln!("{message}");
        }
    }
}

static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_reported_once_per_window() {
        let start = Instant::now();
        let mut window = Window::new("test", start);
        // 第 1 帧只是开窗，不满一秒不报。
        assert!(record(&mut window, start).is_none());
        for frame in 1..100 {
            assert!(record(
                &mut window,
                start + Duration::from_millis(frame as u64 * 10)
            )
            .is_none());
        }
        let report = record(&mut window, start + Duration::from_millis(1000));
        let (frames, elapsed, worst) = report.expect("the window closes after one second");
        assert_eq!(frames, 101);
        assert_eq!(elapsed, Duration::from_millis(1000));
        assert_eq!(worst, Duration::from_millis(10));
        // 窗口被重置：下一帧重新计数，不会把上一秒的帧数带上。
        assert!(record(&mut window, start + Duration::from_millis(1010)).is_none());
        let (frames, _, _) = record(&mut window, start + Duration::from_millis(2020))
            .expect("the next window reports its own frames");
        assert_eq!(frames, 2);
    }

    #[test]
    fn the_worst_frame_is_the_longest_interval_in_the_window() {
        let start = Instant::now();
        let mut window = Window::new("test", start);
        record(&mut window, start);
        record(&mut window, start + Duration::from_millis(10));
        // 一帧慢了 200ms。
        record(&mut window, start + Duration::from_millis(210));
        let (frames, _, worst) =
            record(&mut window, start + Duration::from_millis(1000)).expect("report");
        assert_eq!(frames, 4);
        // 最慢间隔包含「进入这次上报自身的那个间隔」：静止态不重绘时它会被拉长，
        // 这是真实语义（慢帧与空闲都看得见），读数字时和 fps 一起看。
        assert_eq!(worst, Duration::from_millis(790));
    }

    #[test]
    fn a_new_label_starts_a_fresh_window() {
        let start = Instant::now();
        let slot = Some(Window::new("main", start));
        let stale = !matches!(slot.as_ref(), Some(window) if window.label == "popup");
        assert!(stale, "a different label must reset the statistics");
    }
}
