//! The curves things move along.
//!
//! The bar's motion was written as CSS `cubic-bezier` timing functions, and
//! how something settles is most of how it feels. These are the same curves,
//! solved rather than approximated, so a marker that slid a certain way in the
//! webview slides that way here.

/// A CSS `cubic-bezier(x1, y1, x2, y2)`, as a function from progress in time
/// to progress in space.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |progress| {
        let progress = progress.clamp(0., 1.);
        let axis = |t: f32, a: f32, b: f32| {
            let inverse = 1. - t;
            3. * inverse * inverse * t * a + 3. * inverse * t * t * b + t * t * t
        };

        // The curve is given as x(t) and y(t); what is wanted is y at the t
        // where x is the progress. x is monotonic for valid control points,
        // so halving the interval finds it, and twelve halvings is finer than
        // a pixel on any screen.
        let (mut low, mut high) = (0_f32, 1_f32);
        for _ in 0..12 {
            let middle = (low + high) / 2.;
            if axis(middle, x1, x2) < progress {
                low = middle;
            } else {
                high = middle;
            }
        }
        axis((low + high) / 2., y1, y2)
    }
}

/// Arrives quickly and settles slowly: what nearly everything on the bar
/// uses, `cubic-bezier(0.2, 0, 0, 1)` in the stylesheet it came from.
pub fn settle() -> impl Fn(f32) -> f32 {
    cubic_bezier(0.2, 0., 0., 1.)
}

/// The curves by name, for something that has to remember which one it is
/// on: a closure cannot be kept in a value that is copied about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Arrives quickly and settles slowly. Nearly everything.
    Settle,
    /// Decelerates into place: something coming onto the screen.
    Arrive,
    /// Accelerates out of it: something going.
    Leave,
    /// The stylesheet's plain `ease-out` and `ease-in`, for a fade.
    Out,
    In,
}

impl Curve {
    pub fn at(self, progress: f32) -> f32 {
        match self {
            Curve::Settle => settle()(progress),
            Curve::Arrive => cubic_bezier(0.05, 0.7, 0.1, 1.)(progress),
            Curve::Leave => cubic_bezier(0.3, 0., 0.8, 0.15)(progress),
            Curve::Out => cubic_bezier(0., 0., 0.58, 1.)(progress),
            Curve::In => cubic_bezier(0.42, 0., 1., 1.)(progress),
        }
    }
}

/// A number on its way from one value to another, by the clock.
///
/// For whatever has several things moving at once, at different speeds, any
/// of which may be sent somewhere else before it arrives: a toast sliding in
/// while it grows and fades, and then told to leave. Whoever draws with one
/// asks for another frame while it is not `done`, and for none once it is.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    started: std::time::Instant,
    length: std::time::Duration,
    curve: Curve,
}

impl Tween {
    /// At rest, at `value`.
    pub fn still(value: f32) -> Tween {
        Tween {
            from: value,
            to: value,
            started: std::time::Instant::now(),
            length: std::time::Duration::ZERO,
            curve: Curve::Settle,
        }
    }

    pub fn value(&self) -> f32 {
        if self.done() {
            return self.to;
        }
        let along = self.started.elapsed().as_secs_f32() / self.length.as_secs_f32();
        self.from + (self.to - self.from) * self.curve.at(along)
    }

    pub fn done(&self) -> bool {
        self.started.elapsed() >= self.length
    }

    /// Where it is going, whether or not it has got there.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// Heads for `to` from wherever it has got to, which is what makes
    /// changing its mind mid-way a bend rather than a jump.
    pub fn go(&mut self, to: f32, length: std::time::Duration, curve: Curve) {
        *self = Tween { from: self.value(), to, started: std::time::Instant::now(), length, curve };
    }

    /// There, without the journey.
    pub fn jump(&mut self, to: f32) {
        *self = Tween::still(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tween_at_rest_is_where_it_was_put_and_wants_no_frames() {
        let tween = Tween::still(7.);
        assert_eq!(tween.value(), 7.);
        assert!(tween.done());
    }

    #[test]
    fn a_tween_sent_somewhere_starts_from_where_it_had_got_to() {
        let mut tween = Tween::still(0.);
        tween.go(100., std::time::Duration::from_secs(60), Curve::Settle);
        assert!(!tween.done());
        assert_eq!(tween.target(), 100.);
        assert!(tween.value() < 5., "a minute-long journey has barely begun, got {}", tween.value());

        let so_far = tween.value();
        tween.go(-100., std::time::Duration::from_secs(60), Curve::Settle);
        assert!((tween.value() - so_far).abs() < 1., "turning round is a bend, not a jump");

        tween.jump(3.);
        assert!(tween.done());
        assert_eq!(tween.value(), 3.);
    }

    #[test]
    fn every_named_curve_starts_at_nothing_and_ends_at_everything() {
        for curve in [Curve::Settle, Curve::Arrive, Curve::Leave, Curve::Out, Curve::In] {
            assert!(curve.at(0.).abs() < 0.002, "{curve:?} starts at {}", curve.at(0.));
            assert!((curve.at(1.) - 1.).abs() < 0.002, "{curve:?} ends at {}", curve.at(1.));
        }
    }

    #[test]
    fn the_curve_starts_and_ends_where_it_should() {
        let ease = settle();
        assert!(ease(0.).abs() < 0.001);
        assert!((ease(1.) - 1.).abs() < 0.001);
    }

    #[test]
    fn settle_is_most_of_the_way_there_early() {
        // The whole character of this curve: far along in space while still
        // early in time, then a long soft landing.
        let ease = settle();
        assert!(ease(0.3) > 0.6, "got {}", ease(0.3));
        assert!(ease(0.5) > ease(0.3));
    }

    #[test]
    fn the_linear_curve_is_the_identity() {
        let linear = cubic_bezier(0.25, 0.25, 0.75, 0.75);
        for step in 0..=10 {
            let t = step as f32 / 10.;
            assert!((linear(t) - t).abs() < 0.01);
        }
    }
}
