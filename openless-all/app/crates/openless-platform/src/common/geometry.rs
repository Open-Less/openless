//! 跨能力共用的几何类型。
//!
//! 坐标一律是物理像素的虚拟桌面坐标系（与胶囊几何、UIA/AX 锚点一致）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn valid(self) -> bool {
        self.right >= self.left
            && self.bottom > self.top
            && self.right.checked_sub(self.left).is_some()
            && self.bottom.checked_sub(self.top).is_some()
    }

    pub fn from_xywh(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        let values = [x, y, x + width, y + height];
        if width < 0.0
            || height <= 0.0
            || !values
                .iter()
                .all(|v| v.is_finite() && *v > i32::MIN as f64 && *v < i32::MAX as f64)
        {
            return None;
        }
        let rect = Self {
            left: x.floor() as i32,
            top: y.floor() as i32,
            right: (x + width).ceil() as i32,
            bottom: (y + height).ceil() as i32,
        };
        rect.valid().then_some(rect)
    }

    pub fn intersects(self, other: Self) -> bool {
        self.left < other.right
            && self.right >= other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }
}


/// UIA/AX 报告的锚点快照。
///
/// 时间戳用于宿主判断"这个锚点是否已经过期"（例如窗口在这之后被移动过）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiaAnchor {
    pub window: usize,
    pub process_id: u32,
    pub window_rect: Rect,
    pub anchor: Option<Rect>,
    pub screen_rect: Option<Rect>,
    pub captured_at: std::time::Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_rect_requires_positive_area() {
        assert!(Rect { left: 0, top: 0, right: 10, bottom: 10 }.valid());
        assert!(!Rect { left: 0, top: 10, right: 10, bottom: 10 }.valid());
        assert!(!Rect { left: 10, top: 0, right: 0, bottom: 10 }.valid());
    }

    #[test]
    fn from_xywh_rejects_non_finite_and_negative_sizes() {
        assert!(Rect::from_xywh(0.0, 0.0, 10.0, 10.0).is_some());
        assert!(Rect::from_xywh(0.0, 0.0, -1.0, 10.0).is_none());
        assert!(Rect::from_xywh(f64::NAN, 0.0, 10.0, 10.0).is_none());
    }
}
