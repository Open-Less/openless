//! 胶囊窗口的几何计算（与窗口框架无关，纯逻辑可单测）。
//!
//! 这些计算原本散在宿主的 `capsule_mirrors.rs` 里，但它们只依赖显示器矩形与 DPI，
//! 与 tauri 无关，因此下沉到这里。宿主只负责把窗口系统的显示器信息转换成
//! [`MonitorRect`]。

use openless_core::shared_types::CapsuleDisplayMode;

/// 最多镜像到几块副屏。
pub const MAX_CAPSULE_MIRRORS: usize = 7;

/// 虚拟桌面坐标系下的一块显示器。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub work_left: i32,
    pub work_top: i32,
    pub work_right: i32,
    pub work_bottom: i32,
    /// 有效 DPI 缩放（1.0 = 96 dpi）。
    pub scale: f64,
    /// 稳定的显示器标识散列；0 表示只能靠外框区分。
    pub identity: u64,
}

impl MonitorRect {
    /// 由窗口系统提供的原始数值构造。
    pub const fn from_parts(
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        work_left: i32,
        work_top: i32,
        work_width: i32,
        work_height: i32,
        scale: f64,
        identity: u64,
    ) -> Self {
        Self {
            left,
            top,
            right: left + width,
            bottom: top + height,
            work_left,
            work_top,
            work_right: work_left + work_width,
            work_bottom: work_top + work_height,
            scale,
            identity,
        }
    }

    /// 工作区（排除任务栏）；系统未报告时退回整块显示器。
    pub fn work_rect(&self) -> (i32, i32, i32, i32) {
        if self.work_right > self.work_left && self.work_bottom > self.work_top {
            (
                self.work_left,
                self.work_top,
                self.work_right,
                self.work_bottom,
            )
        } else {
            (self.left, self.top, self.right, self.bottom)
        }
    }
}

/// 用显示器名称生成稳定标识（FNV-1a）。
pub fn monitor_identity(name: Option<&str>) -> u64 {
    let Some(name) = name else {
        return 0;
    };
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

/// 点所在显示器。
pub fn monitor_at_point(monitors: &[MonitorRect], x: i32, y: i32) -> Option<MonitorRect> {
    monitors
        .iter()
        .copied()
        .find(|m| x >= m.left && x < m.right && y >= m.top && y < m.bottom)
}

/// 把胶囊贴在显示器工作区底部中央。
///
/// `logical_*` 是 DPI 无关尺寸，按该显示器自己的 scale 换算，混合 DPI 下尺寸才正确。
/// 返回 `(physical_width, physical_height, x, y)`。
pub fn capsule_placement(
    logical_width: f64,
    logical_height: f64,
    logical_bottom_gap: f64,
    monitor: MonitorRect,
) -> (u32, u32, i32, i32) {
    let scale = monitor.scale;
    let phys_w = (logical_width * scale).round() as i32;
    let phys_h = (logical_height * scale).round() as i32;
    let (work_l, work_t, work_r, work_b) = monitor.work_rect();
    let x = work_l + ((work_r - work_l - phys_w) / 2).max(0);
    let y = work_b - phys_h - (logical_bottom_gap * scale).round() as i32;
    let (x, y) = clamp_to_monitor(x, y, phys_w, phys_h, work_l, work_t, work_r, work_b);
    (phys_w.max(1) as u32, phys_h.max(1) as u32, x, y)
}

/// 把窗口拉回显示器工作区（放不下时贴边）。
pub fn clamp_to_monitor(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work_l: i32,
    work_t: i32,
    work_r: i32,
    work_b: i32,
) -> (i32, i32) {
    let max_x = (work_r - width).max(work_l);
    let max_y = (work_b - height).max(work_t);
    (x.clamp(work_l, max_x), y.clamp(work_t, max_y))
}

/// 需要镜像到的副屏：仅在显式选择「所有屏幕」且排除主屏时返回。
pub fn mirror_targets(
    mode: CapsuleDisplayMode,
    monitors: &[MonitorRect],
    primary: MonitorRect,
) -> Vec<MonitorRect> {
    if mode != CapsuleDisplayMode::AllScreens {
        return Vec::new();
    }
    monitors
        .iter()
        .copied()
        .filter(|m| {
            (m.left, m.top, m.right, m.bottom)
                != (primary.left, primary.top, primary.right, primary.bottom)
        })
        .take(MAX_CAPSULE_MIRRORS)
        .collect()
}

/// 一次性浮层的目标显示器（与镜像共用同一套选择规则）。
pub fn display_targets(
    mode: CapsuleDisplayMode,
    monitors: &[MonitorRect],
    primary: MonitorRect,
) -> Vec<MonitorRect> {
    let mut targets = vec![primary];
    targets.extend(mirror_targets(mode, monitors, primary));
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(left: i32, top: i32, scale: f64, identity: u64) -> MonitorRect {
        MonitorRect::from_parts(left, top, 1920, 1080, left, top, 1920, 1040, scale, identity)
    }

    #[test]
    fn work_rect_falls_back_to_full_monitor_when_unreported() {
        let mut m = monitor(0, 0, 1.0, 1);
        m.work_right = 0;
        m.work_bottom = 0;
        assert_eq!(m.work_rect(), (0, 0, 1920, 1080));
    }

    #[test]
    fn placement_centers_on_the_work_area_bottom() {
        let m = monitor(0, 0, 2.0, 1);
        let (w, h, x, y) = capsule_placement(200.0, 40.0, 8.0, m);
        assert_eq!((w, h), (400, 80));
        // 工作区 1920 宽、底部 1040，水平居中。
        assert_eq!(x, (1920 - 400) / 2);
        assert_eq!(y, 1040 - 80 - 16);
    }

    #[test]
    fn placement_is_clamped_into_negative_origin_monitors() {
        let m = monitor(-1920, -200, 1.0, 2);
        let (w, _, x, y) = capsule_placement(400.0, 100.0, 0.0, m);
        assert_eq!(w, 400);
        assert!(x >= -1920);
        assert!(y >= -200);
    }

    #[test]
    fn monitor_at_point_matches_half_open_bounds() {
        let monitors = [monitor(0, 0, 1.0, 1), monitor(1920, 0, 1.0, 2)];
        assert_eq!(monitor_at_point(&monitors, 0, 0).map(|m| m.identity), Some(1));
        assert_eq!(
            monitor_at_point(&monitors, 1920, 10).map(|m| m.identity),
            Some(2)
        );
        assert!(monitor_at_point(&monitors, 5000, 5000).is_none());
    }

    #[test]
    fn mirrors_only_on_opt_in_and_never_the_primary() {
        let primary = monitor(0, 0, 1.0, 1);
        let second = monitor(1920, 0, 1.0, 2);
        let monitors = [primary, second];
        assert!(mirror_targets(CapsuleDisplayMode::PrimaryScreen, &monitors, primary).is_empty());
        let mirrors = mirror_targets(CapsuleDisplayMode::AllScreens, &monitors, primary);
        assert_eq!(mirrors.len(), 1);
        assert_eq!(mirrors[0].identity, 2);
    }

    #[test]
    fn display_targets_always_include_the_primary() {
        let primary = monitor(0, 0, 1.0, 1);
        let targets = display_targets(CapsuleDisplayMode::PrimaryScreen, &[primary], primary);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].identity, 1);
    }

    #[test]
    fn identity_hash_is_stable_and_zero_for_missing_names() {
        assert_eq!(monitor_identity(None), 0);
        assert_eq!(monitor_identity(Some("DISPLAY1")), monitor_identity(Some("DISPLAY1")));
        assert_ne!(monitor_identity(Some("DISPLAY1")), monitor_identity(Some("DISPLAY2")));
    }
}
