//! The one door every zoom goes through — a bar button, a keyboard step, a turn, a
//! resize — which resolves the ask, records the target and starts the tween.

use super::{sane, Advanced, SETTLED_EPSILON, SETTLE_MS, Transition, Zoom};

impl Zoom {
    /// Open a transition to `scale`, or land it in this frame.
    ///
    /// `animate` asks for the tween; a follow never tweens whatever it asks,
    /// because a scale that eases towards a moving window chases it instead of
    /// sitting in it. An untweened landing commits in the same frame, except a
    /// follow's: it lands — the layout has to move with the window — and holds
    /// its commit for the settle window.
    pub fn begin(&mut self, scale: f64, animate: bool, following: bool) -> Advanced {
        let scale = sane(scale);
        // Move the settle deadline before deciding this frame is a no-op:
        // while a burst runs the scale may be pinned (a page at the minimum, a
        // hand-picked zoom the window has caught up with), and a commit landing
        // on a frame the window is still moving rasterises at a size the reader
        // is already past.
        if following {
            self.settle_ms = SETTLE_MS;
        }
        // Already there, or already bound there: nothing to move — and the
        // transaction in flight is left exactly as it is.
        let settled = self.in_flight_target().unwrap_or(self.display);
        if (scale - settled).abs() < SETTLED_EPSILON {
            return Advanced::NONE;
        }
        let animate = animate && !following;
        self.transition = Some(Transition {
            from: self.display,
            to: scale,
            elapsed_ms: 0.0,
            animate,
            following,
        });
        if following {
            // A follow lands in the frame it was asked for, not on the next
            // animation frame: the new size has to be in the same frame as the
            // new width, or the page sits a few pixels wider than its box for
            // a whole frame — a flicker along the whole drag.
            self.display = scale;
            return Advanced::MOVED;
        }
        if !animate {
            // A refit tracking a live resize lands where it was asked to; only
            // a gesture is worth 120 ms.
            self.transition = None;
            self.commit(scale);
            return Advanced::LANDED;
        }
        Advanced::MOVED
    }
}
