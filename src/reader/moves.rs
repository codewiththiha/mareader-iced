//! The moves a message asks for: turn to another page, flip the mode, step the
//! zoom, or land on a fit.

use std::cmp::Ordering;
use std::time::Instant;

use reader_core::view::{self, ViewMode};
use reader_core::zoom_math::FitMode;

use super::zoom::{self, Advanced, Command};
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
        // A gesture animates only if the reader's own motion settings allow it:
        // off, the page lands at its new size in the frame the press arrived in.
        let did = self
            .zoom
            .begin(target.scale, animate && self.motion.zoom, following);
        if did.moved {
            // The instant the tween is measured from. The app's ticks carry
            // their own instants, and this is the one the first of them compares
            // against — a stale one would land the tween in a single frame.
            self.last_tick = Instant::now();
        }
        if did.committed {
            self.pump_frames();
        }
        did
    }

    /// One step forward or back through the book.
    ///
    /// The step clamps to the book rather than wrapping: a reader leaning on the
    /// next button at the end of a book expects to arrive at the end, not at the
    /// beginning. A spread steps by the pair it is showing.
    pub(super) fn turn(&mut self, step: i32) -> Vec<Effect> {
        if !self.document.status.is_ready() || step == 0 {
            return Vec::new();
        }
        let next = self.step_page(step);
        if next == self.viewer.page {
            // A press with nowhere to go writes nothing: the shelf already holds
            // this page, and a read stamp that moves on its own would reorder a
            // shelf the reader never moved on.
            return Vec::new();
        }
        let mut effects = self.go_to_page(next);
        // A keypress is a discrete move: the app is told where it landed now
        // rather than when the quiet timer comes round.
        effects.extend(self.report_progress());
        effects
    }

    /// The page a step lands on: one page at a time, except in a spread, which
    /// steps by the pair on screen.
    fn step_page(&self, step: i32) -> u32 {
        let last = self.document.num_pages.max(1);
        let page = self.viewer.page.clamp(1, last);
        match (self.viewer.mode, step.cmp(&0)) {
            (ViewMode::Spread, Ordering::Less) => view::spread_step_prev(page),
            (ViewMode::Spread, Ordering::Greater) => view::spread_step_next(last, page),
            (_, Ordering::Less) => page.saturating_sub(1).max(1),
            (_, Ordering::Greater) => (page + 1).min(last),
            (_, Ordering::Equal) => page,
        }
    }

    /// Put the reader on `page`: the one door a page change goes through, so a
    /// turn, a jump and — later — an outline entry or a search hit all land the
    /// same way.
    pub(super) fn go_to_page(&mut self, page: u32) -> Vec<Effect> {
        if !self.document.status.is_ready() {
            return Vec::new();
        }
        let page = page.clamp(1, self.document.num_pages.max(1));
        if page == self.viewer.page {
            return Vec::new();
        }
        self.viewer.page = page;
        self.auto_scale_for_page();
        // The scale may have moved with the sheet, so the strip is rebuilt
        // before the jump is measured: a jump aimed with the old page extents
        // lands on the wrong place in the column.
        let mut effects = self.reflow();
        if self.scrolls() {
            if self.zoom.ticking() {
                // The geometry is moving under the strip, so a jump now would be
                // re-anchored onto the page the reader just left; the jump is
                // owed, and taken when the transaction closes.
                self.held.hold();
            } else {
                effects.extend(self.jump_to(page));
            }
        }
        self.pump_frames();
        effects
    }

    /// A page turn may land on a differently sized sheet — a landscape plate
    /// inside a portrait book — and `auto_resize` decides whether the scale
    /// follows it: the fit when one owns the scale, the reader's own zoom when
    /// it does not. Off, the turn touches nothing, and a plate too wide for the
    /// window overflows and pans.
    fn auto_scale_for_page(&mut self) {
        if !self.viewer.auto_resize {
            return;
        }
        let command = if self.viewer.fit == FitMode::None {
            Command::Constrain
        } else {
            Command::Refit
        };
        self.zoom_command(command, false);
    }

    /// Flip the view mode, with the three things the web app's `mode_change`
    /// owed a native reader.
    ///
    /// The incoming strip mounts anchored to the page the reader is on
    /// ([`Reader::reflow`] does that, and the aim stands the scroll→page sync
    /// down until it has landed); the fit is the INCOMING mode's to resolve
    /// rather than the outgoing layout's reinterpreted against a new axis — a
    /// vertical width fit read as a height fit drops the readout by almost half —
    /// and whatever was moving on the surface that just left is dropped, because
    /// that surface is gone. (The fourth was the engine's own raster sweep, which
    /// a DOM view needed because nothing rendered after a flip; here the pump
    /// lets go of what is not on screen on the same frame.)
    pub(super) fn set_mode(&mut self, mode: ViewMode) {
        if self.viewer.mode == mode {
            return;
        }
        self.viewer.mode = mode;
        self.strip = None;
        self.glide = None;
        self.anchor = None;
        match mode {
            // A horizontal strip is one page per item: a width fit resolved
            // against the vertical axis would become a height fit here and halve
            // the readout, so the reader's own scale is what it keeps.
            ViewMode::ScrollHorizontal => self.viewer.fit = FitMode::None,
            ViewMode::Spread => self.viewer.fit = FitMode::Width,
            ViewMode::Single if self.viewer.auto_scale => self.viewer.fit = FitMode::Width,
            _ => {}
        }
        self.zoom_command(Command::Follow, false);
    }
}
