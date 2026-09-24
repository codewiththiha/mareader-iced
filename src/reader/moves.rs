//! The moves a message asks for: turn to another page or sheet, step the zoom, or
//! land on a fit.

use std::time::Instant;

use reader_core::zoom_math::FitMode;

use super::zoom::{Advanced, Command};
use super::zoom;
use super::{Effect, Reader};

impl Reader {
    /// Post one zoom intent: resolve it against the state it lands in, record
    /// what the resolution decided, and open the transition it asks for.
    ///
    /// The one door every zoom goes through — the bar's buttons, the keyboard's
    /// rungs, the window's own follow and a page turn's re-fit — so a fit
    /// cannot race a gesture along a second path.
    pub(super) fn zoom_command(&mut self, command: Command, animate: bool) -> Advanced {
        let sheet = self.document.page_box(self.viewer.page);
        let Some(target) = zoom::resolve(&self.viewer, &self.zoom, sheet, command) else {
            return Advanced::NONE;
        };
        if self.zoom.note(target) {
            // A hand-picked scale owns the zoom now: the fit mode steps aside,
            // or it would fight the gesture on the next window change.
            self.viewer.fit = FitMode::None;
        }
        let following = command == Command::Follow;
        let did = self.zoom.begin(target.scale, animate, following);
        if did.moved {
            // The instant the tween is measured from. The app's ticks carry
            // their own instants, and this is the one the first of them compares
            // against — a stale one would land the tween in a single frame.
            self.last_tick = Instant::now();
        }
        if did.committed {
            self.request_frame();
        }
        did
    }

    /// One page forward or back.
    ///
    /// The step clamps to the book rather than wrapping: a reader leaning on
    /// the next button at the end of a book expects to arrive at the end, not
    /// at the beginning. Spread stepping and the horizontal strip's own
    /// ordering arrive with those modes in 3c.
    pub(super) fn turn(&mut self, step: i32) -> Vec<Effect> {
        if !self.document.status.is_ready() || step == 0 {
            return Vec::new();
        }
        let last = self.document.num_pages.max(1);
        let next = self
            .viewer
            .page
            .saturating_add_signed(step)
            .clamp(1, last);
        if next == self.viewer.page {
            // A press with nowhere to go writes nothing: the shelf already
            // holds this page, and a read stamp that moves on its own would
            // reorder a shelf the reader never moved on.
            return Vec::new();
        }
        self.viewer.page = next;
        // A page turn may land on a differently sized sheet — a landscape plate
        // inside a portrait book — and the web app's `auto_resize` is what
        // decides whether the scale follows it. On, the new sheet is resolved
        // the way the window's own changes are: the fit when one owns the
        // scale, the reader's own zoom when it does not. Off, the turn touches
        // nothing, and a plate too wide for the window overflows and scrolls.
        //
        // Untweened, deliberately: the answer is not a gesture but the page
        // arriving — the same reasoning that keeps a window resize finding the
        // new size in the frame it was reported. (The web app debounced this by
        // its settle window because a SCROLL moves pages continuously; a strip
        // that does arrives in 3c, and the debounce belongs with it.)
        let moved = if self.viewer.auto_resize {
            let command = if self.viewer.fit == FitMode::None {
                Command::Constrain
            } else {
                Command::Refit
            };
            self.zoom_command(command, false).committed
        } else {
            false
        };
        if !moved {
            // The page under the reader's eyes changed, so a raster is owed for
            // it whether or not the zoom moved the scale the old one was drawn
            // at.
            self.request_frame();
        }
        self.effect_of(Effect::Progress)
    }
}
