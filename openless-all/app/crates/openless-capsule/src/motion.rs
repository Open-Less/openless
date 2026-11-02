//! Easing and spring integration for capsule transitions. Interrupted animations
//! continue from their current values instead of restarting at their initial state.

/// `cubic-bezier(x1, y1, x2, y2)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ease(pub f32, pub f32, pub f32, pub f32);

pub const LINEAR: Ease = Ease(0.0, 0.0, 1.0, 1.0);
/// CSS `ease`.
pub const EASE: Ease = Ease(0.25, 0.1, 0.25, 1.0);
/// Symmetric acceleration and deceleration for property transitions.
#[cfg(test)]
pub const EASE_IN_OUT: Ease = Ease(0.42, 0.0, 0.58, 1.0);
/// `MORPH` in OpenLessCapsule.tsx.
pub const MORPH: Ease = Ease(0.22, 1.0, 0.36, 1.0);

impl Ease {
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if self == LINEAR || t == 0.0 || t == 1.0 {
            return t;
        }
        let Ease(x1, y1, x2, y2) = self;
        let bezier = |a: f32, b: f32, s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        // x(s) is monotonic for valid CSS curves: bisect for the parameter.
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = (low + high) * 0.5;
            if bezier(x1, x2, mid) < t {
                low = mid;
            } else {
                high = mid;
            }
        }
        bezier(y1, y2, (low + high) * 0.5)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    start: f64,
    delay: f64,
    duration: f64,
    ease: Ease,
}

impl Tween {
    pub fn at(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: 0.0,
            delay: 0.0,
            duration: 0.0,
            ease: LINEAR,
        }
    }

    pub fn value(&self, now: f64) -> f32 {
        let elapsed = now - self.start - self.delay;
        if elapsed < 0.0 {
            return self.from;
        }
        if self.duration <= 0.0 || elapsed >= self.duration {
            return self.to;
        }
        let progress = self.ease.at((elapsed / self.duration) as f32);
        self.from + (self.to - self.from) * progress
    }

    pub fn active(&self, now: f64) -> bool {
        now < self.start + self.delay + self.duration && self.from != self.to
    }

    /// Retarget from the current value; an unchanged target keeps running.
    pub fn set(&mut self, now: f64, to: f32, duration: f64, delay: f64, ease: Ease) {
        if to == self.to {
            return;
        }
        *self = Self {
            from: self.value(now),
            to,
            start: now,
            delay,
            duration,
            ease,
        };
    }

    pub fn jump(&mut self, value: f32) {
        *self = Self::at(value);
    }
}

/// A damped mass-spring system integrated in bounded time steps.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
    mass: f32,
}

impl Spring {
    pub fn new(value: f32, stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness,
            damping,
            mass,
        }
    }

    pub fn step(&mut self, dt: f32) {
        let mut remaining = dt.clamp(0.0, 0.05);
        while remaining > 0.0 {
            let h = remaining.min(1.0 / 240.0);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force / self.mass * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if !self.active() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    pub fn active(&self) -> bool {
        (self.value - self.target).abs() > 0.05 || self.velocity.abs() > 0.5
    }

    pub fn jump(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_hit_their_endpoints_and_the_morph_curve_front_loads_motion() {
        for ease in [LINEAR, EASE, EASE_IN_OUT, MORPH] {
            assert_eq!(ease.at(0.0), 0.0);
            assert_eq!(ease.at(1.0), 1.0);
        }
        assert!((EASE_IN_OUT.at(0.5) - 0.5).abs() < 1e-3);
        // 50% of the 220ms morph has covered most of the distance.
        assert!(MORPH.at(0.5) > 0.85);
    }

    #[test]
    fn an_interrupted_tween_restarts_from_where_it_is() {
        let mut width = Tween::at(116.0);
        width.set(0.0, 80.0, 0.22, 0.0, MORPH);
        let midway = width.value(0.05);
        assert!(midway < 116.0 && midway > 80.0);
        width.set(0.05, 116.0, 0.22, 0.0, MORPH);
        assert_eq!(width.value(0.05), midway);
        assert_eq!(width.value(1.0), 116.0);
        assert!(!width.active(1.0));
    }

    #[test]
    fn delayed_tweens_hold_their_start_value() {
        let mut opacity = Tween::at(0.0);
        opacity.set(1.0, 1.0, 0.08, 0.12, LINEAR);
        assert_eq!(opacity.value(1.1), 0.0);
        assert!((opacity.value(1.16) - 0.5).abs() < 1e-3);
        assert_eq!(opacity.value(1.3), 1.0);
    }

    #[test]
    fn reduced_motion_lands_in_the_same_frame() {
        let mut width = Tween::at(68.0);
        width.set(2.0, 116.0, 0.0, 0.0, MORPH);
        assert_eq!(width.value(2.0), 116.0);
        assert!(!width.active(2.0));
    }

    #[test]
    fn springs_settle_on_their_target() {
        let mut spring = Spring::new(72.0, 330.0, 32.0, 0.9);
        spring.target = 200.0;
        for _ in 0..120 {
            spring.step(1.0 / 60.0);
        }
        assert_eq!(spring.value, 200.0);
        assert!(!spring.active());
    }
}
