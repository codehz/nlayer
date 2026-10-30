//! Animation drivers. Both are closed-form functions of elapsed time, so the
//! result does not depend on frame rate or dropped frames.
//!
//! Extracted from `nsh-core/src/animation/driver.rs`: timing and spring only
//! (no decay), with the easing curves v2 exposes.

use super::ops::{AnimationId, Driver, Easing, StepPosition};

/// Spring rest thresholds (value units and units per second).
const REST_DISPLACEMENT: f32 = 0.001;
const REST_VELOCITY: f32 = 0.001;

/// One animation running on a value.
#[derive(Debug, Clone)]
pub(crate) struct Running {
    pub(crate) id: AnimationId,
    driver: Driver,
    from: f32,
    velocity: f32,
    /// First frame time (seconds); set on the first step.
    start: Option<f64>,
}

impl Running {
    pub(crate) fn new(id: AnimationId, driver: Driver, from: f32, velocity: f32) -> Self {
        let velocity = match driver {
            Driver::Spring {
                velocity: Some(v), ..
            } => v,
            _ => velocity,
        };
        Self {
            id,
            driver,
            from,
            velocity,
            start: None,
        }
    }

    /// Advances to `now` (seconds). Returns `(value, velocity, finished)`.
    pub(crate) fn step(&mut self, now: f64) -> (f32, f32, bool) {
        let start = *self.start.get_or_insert(now);
        let t = (now - start).max(0.0) as f32;
        match self.driver {
            Driver::Timing {
                to,
                duration_ms,
                easing,
                delay_ms,
            } => {
                let t_ms = t * 1000.0 - delay_ms;
                if t_ms < 0.0 || (t_ms == 0.0 && duration_ms > 0.0) {
                    return (self.from, 0.0, false);
                }
                if t_ms >= duration_ms {
                    return (to, 0.0, true);
                }
                let p = ease(easing, t_ms / duration_ms);
                (self.from + (to - self.from) * p, 0.0, false)
            }
            Driver::Spring {
                to,
                stiffness,
                damping,
                mass,
                ..
            } => {
                let x0 = self.from - to;
                let (x, v) = spring(x0, self.velocity, stiffness, damping, mass, t);
                if x.abs() < REST_DISPLACEMENT && v.abs() < REST_VELOCITY {
                    (to, 0.0, true)
                } else {
                    (to + x, v, false)
                }
            }
        }
    }
}

/// The easing curve at `p` (`p` in 0..=1).
pub(crate) fn ease(easing: Easing, p: f32) -> f32 {
    match easing {
        Easing::Linear => p,
        Easing::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, p),
        Easing::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, p),
        Easing::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, p),
        Easing::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, p),
        Easing::Bezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, p),
        Easing::Steps(count, position) => steps(count, position, p),
    }
}

fn steps(count: u32, position: StepPosition, p: f32) -> f32 {
    let count = count.max(1);
    let (jumps, offset) = match position {
        StepPosition::JumpStart => (count, 1),
        StepPosition::JumpEnd => (count, 0),
        StepPosition::JumpNone => (count.saturating_sub(1).max(1), 0),
        StepPosition::JumpBoth => (count + 1, 1),
    };
    let step = (p * count as f32).floor() as i64 + offset as i64;
    (step.clamp(0, jumps as i64) as f32) / jumps as f32
}

/// CSS's `cubic-bezier(x1, y1, x2, y2)` at `p`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, p: f32) -> f32 {
    let x1 = x1.clamp(0.0, 1.0);
    let x2 = x2.clamp(0.0, 1.0);
    let sample = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let derivative = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let mut t = p;
    for _ in 0..8 {
        let error = sample(t, x1, x2) - p;
        if error.abs() < 1e-6 {
            return sample(t, y1, y2).clamp(0.0, 1.0);
        }
        let slope = derivative(t, x1, x2);
        if slope.abs() < 1e-6 {
            break;
        }
        t -= error / slope;
    }
    let (mut low, mut high) = (0.0, 1.0);
    let mut t = p;
    for _ in 0..32 {
        let x = sample(t, x1, x2);
        if (x - p).abs() < 1e-6 {
            break;
        }
        if x < p {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }
    sample(t, y1, y2).clamp(0.0, 1.0)
}

/// The damped spring's displacement and velocity at `t` seconds.
fn spring(x0: f32, v0: f32, k: f32, c: f32, m: f32, t: f32) -> (f32, f32) {
    // f64 so an overdamped `sinh` does not overflow to infinity before its
    // exponential decay cancels it.
    let (x0, v0, k, c, m, t) = (
        x0 as f64,
        v0 as f64,
        k.max(0.0) as f64,
        c.max(0.0) as f64,
        (m.max(1e-6)) as f64,
        t as f64,
    );
    let w0 = (k / m).sqrt();
    if w0 == 0.0 {
        return ((x0 + v0 * t) as f32, v0 as f32);
    }
    let zeta = c / (2.0 * (k * m).sqrt());
    let (x, v) = if zeta < 1.0 {
        let wd = w0 * (1.0 - zeta * zeta).sqrt();
        if wd == 0.0 {
            (x0, v0)
        } else {
            let a = x0;
            let b = (v0 + zeta * w0 * x0) / wd;
            let e = (-zeta * w0 * t).exp();
            let (s, co) = (wd * t).sin_cos();
            let x = e * (a * co + b * s);
            let v = e * ((b * wd - zeta * w0 * a) * co - (a * wd + zeta * w0 * b) * s);
            (x, v)
        }
    } else if (zeta - 1.0).abs() < 1e-9 {
        let e = (-w0 * t).exp();
        let b = v0 + w0 * x0;
        (e * (x0 + b * t), e * (b - w0 * (x0 + b * t)))
    } else {
        let wd = w0 * (zeta * zeta - 1.0).sqrt();
        let a = x0;
        let b = (v0 + zeta * w0 * x0) / wd;
        let e = (-zeta * w0 * t).exp();
        let (s, co) = ((wd * t).sinh(), (wd * t).cosh());
        let x = e * (a * co + b * s);
        let v = e * ((b * wd - zeta * w0 * a) * co + (a * wd - zeta * w0 * b) * s);
        (x, v)
    };
    (x as f32, v as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing(to: f32, duration_ms: f32, easing: Easing) -> Driver {
        Driver::Timing {
            to,
            duration_ms,
            easing,
            delay_ms: 0.0,
        }
    }

    #[test]
    fn timing_reaches_its_target_on_time() {
        let mut run = Running::new(1, timing(1.0, 1000.0, Easing::Linear), 0.0, 0.0);
        assert_eq!(run.step(10.0), (0.0, 0.0, false));
        assert_eq!(run.step(10.5), (0.5, 0.0, false));
        assert_eq!(run.step(11.0), (1.0, 0.0, true));
    }

    #[test]
    fn timing_waits_out_its_delay() {
        let driver = Driver::Timing {
            to: 1.0,
            duration_ms: 100.0,
            easing: Easing::Linear,
            delay_ms: 50.0,
        };
        let mut run = Running::new(1, driver, 0.0, 0.0);
        assert_eq!(run.step(0.0).0, 0.0);
        assert_eq!(run.step(0.075).0, 0.25);
        assert_eq!(run.step(0.15), (1.0, 0.0, true));
    }

    #[test]
    fn easing_curves_are_pinned_at_both_ends() {
        for easing in [
            Easing::Linear,
            Easing::Ease,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
            Easing::Bezier(0.2, 0.8, 0.2, 1.0),
            Easing::Steps(4, StepPosition::JumpEnd),
        ] {
            assert!((ease(easing, 0.0) - 0.0).abs() < 1e-5, "{easing:?}");
            assert!((ease(easing, 1.0) - 1.0).abs() < 1e-5, "{easing:?}");
        }
    }

    #[test]
    fn springs_settle_in_all_damping_regimes() {
        for damping in [5.0, 20.0, 60.0] {
            let driver = Driver::Spring {
                to: 10.0,
                stiffness: 180.0,
                damping,
                mass: 1.0,
                velocity: Some(0.0),
            };
            let mut run = Running::new(1, driver, 0.0, 0.0);
            assert!(run.step(0.0).0 < 10.0);
            let (value, velocity, done) = run.step(10.0);
            assert!(done, "damping {damping} did not settle");
            assert_eq!(value, 10.0);
            assert_eq!(velocity, 0.0);
        }
    }

    #[test]
    fn an_underdamped_spring_overshoots() {
        let driver = Driver::Spring {
            to: 1.0,
            stiffness: 180.0,
            damping: 8.0,
            mass: 1.0,
            velocity: Some(0.0),
        };
        let mut run = Running::new(1, driver, 0.0, 0.0);
        let peak = (1..120)
            .map(|i| run.step(i as f64 / 60.0).0)
            .fold(0.0f32, f32::max);
        assert!(peak > 1.05, "peak {peak}");
    }

    #[test]
    fn steps_jump_at_their_boundaries() {
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpEnd), 0.0), 0.0);
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpEnd), 0.24), 0.0);
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpEnd), 0.26), 0.25);
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpStart), 0.0), 0.25);
        assert_eq!(ease(Easing::Steps(4, StepPosition::JumpNone), 1.0), 1.0);
    }
}
