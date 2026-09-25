//! The rail's open and close: the window that widens out of the left edge over
//! the web app's 300 ms, and the switch that skips the motion.

/// How long the rail takes to open or close — the web aside's `duration-300`.
pub(super) const SLIDE_MS: f64 = 300.0;

/// How long a floating rail waits before closing after the pointer leaves it —
/// long enough to cross the panel's edge, short enough to feel like dismissal.
pub(super) const HOVER_GRACE_MS: f64 = 250.0;

/// How much of the rail is on screen: 0 at the window's edge, 1 fully open.
///
/// The web app reached the same number through a CSS width transition; here the
/// reader's frame clock steps it, which is why the motion is a value rather than
/// a style. It is a fraction of the rail's own width, so the rail's contents
/// never reflow on the way in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Slide {
    from: f32,
    to: f32,
    elapsed_ms: f64,
}

impl Slide {
    /// A rail that has never opened.
    pub(super) fn closed() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            elapsed_ms: SLIDE_MS,
        }
    }

    /// Begin a move to `target`, from wherever the window is now. `freeze` lands
    /// it in the frame it was asked in: the animations switch is read at the
    /// gesture, so a disabled motion is a landing rather than a longer wait.
    pub(super) fn begin(&mut self, target: f32, freeze: bool) {
        self.from = self.factor();
        self.to = target;
        self.elapsed_ms = if freeze { SLIDE_MS } else { 0.0 };
    }

    /// Where the window is right now.
    pub(super) fn factor(&self) -> f32 {
        let t = (self.elapsed_ms / SLIDE_MS) as f32;
        self.from + (self.to - self.from) * ease(t)
    }

    /// Whether the motion has frames left to run.
    pub(super) fn moving(&self) -> bool {
        self.elapsed_ms < SLIDE_MS
    }

    /// One frame of the motion.
    pub(super) fn step(&mut self, delta_ms: f64) {
        if self.moving() {
            self.elapsed_ms += delta_ms.max(0.0);
        }
    }
}

/// The web chrome's `ease-in-out`, kept symmetric: the rail leaves the edge
/// slowly, crosses at speed and settles, and closes the same way back.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slide_runs_from_where_the_window_is_to_where_it_was_asked() {
        let mut slide = Slide::closed();
        assert_eq!(slide.factor(), 0.0);
        assert!(!slide.moving(), "a rail at rest owes nothing");
        slide.begin(1.0, false);
        assert_eq!(slide.factor(), 0.0, "the first frame is where it already was");
        assert!(slide.moving());
        slide.step(SLIDE_MS / 2.0);
        let half = slide.factor();
        assert!(half > 0.0 && half < 1.0, "half the clock is part of the way");
        slide.step(SLIDE_MS);
        assert_eq!(slide.factor(), 1.0, "the clock runs out on the open rail");
        assert!(!slide.moving());
    }

    #[test]
    fn a_close_turns_around_mid_flight_and_a_frozen_one_lands_at_once() {
        let mut slide = Slide::closed();
        slide.begin(1.0, false);
        slide.step(SLIDE_MS / 2.0);
        let turned = slide.factor();
        slide.begin(0.0, false);
        assert!((slide.factor() - turned).abs() < 1e-6, "the close starts where the open got to");
        slide.step(SLIDE_MS);
        assert_eq!(slide.factor(), 0.0);

        slide.begin(1.0, true);
        assert_eq!(slide.factor(), 1.0, "no motion, no wait");
        assert!(!slide.moving());
        assert_eq!(ease(-1.0), 0.0, "a clock that overran is clamped");
        assert_eq!(ease(2.0), 1.0);
    }
}
