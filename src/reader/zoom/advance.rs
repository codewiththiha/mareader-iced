//! The tween's clock: step it once per frame, and land it when it arrives.

use super::{ease_out_cubic, Advanced, TWEEN_MS, Zoom};

impl Zoom {
    /// One animation frame, `delta_ms` after the last.
    pub fn advance(&mut self, delta_ms: f64) -> Advanced {
        let Some(mut t) = self.transition else {
            return Advanced::NONE;
        };
        if t.following {
            // A held follow has already landed; what is left is its deadline,
            // re-armed by every post, so a burst re-arms one window rather than
            // queueing a commit per frame — the commit happens once, at the
            // size the window stopped at.
            self.settle_ms -= delta_ms.max(0.0);
            if self.settle_ms <= 0.0 {
                self.transition = None;
                self.settle_ms = 0.0;
                self.commit(t.to);
                return Advanced::LANDED;
            }
            return Advanced::NONE;
        }
        t.elapsed_ms += delta_ms.max(0.0);
        // The transition's own guard, the web app's tween loop kept the same
        // one: a transaction opened without a tween has no midpoint to
        // interpolate through — its first frame is its last.
        let progress = if t.animate {
            (t.elapsed_ms / TWEEN_MS).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.display = t.from + (t.to - t.from) * ease_out_cubic(progress);
        if progress >= 1.0 {
            let to = t.to;
            self.transition = None;
            self.commit(to);
            return Advanced::LANDED;
        }
        self.transition = Some(t);
        Advanced::MOVED
    }

    /// Every transaction ends here: the committed scale moves onto the target,
    /// and the display scale with it — a tween's last frame interpolated to
    /// exactly this number, and an untweened landing wrote it outright.
    pub(super) fn commit(&mut self, to: f64) {
        self.display = to;
        self.committed = to;
    }
}
