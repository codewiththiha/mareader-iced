//! The scroll↔page rules, as much of them as a native reader has: which page a
//! strip's window puts under the reader's eyes, and the one thing a zoom
//! transaction may take from a page turn.
//!
//! The web app's `navigation_sync` carried more than this. Its jump gate held a
//! page write, replayed it, and kept an echo flag, because a DOM scroll offset
//! is written asynchronously by the browser and read back a frame later — and
//! its `awaiting_anchor` settle loop existed because a freshly mounted strip
//! reports an offset that is not the reader's page yet. An iced scroll offset is
//! widget state: a report is always that state, and the strip's own belief
//! already holds the anchor ([`super::strip::Anchor`]). What is left is the
//! arithmetic and the hold.

/// The page a strip's dominant item (0-based) names, clamped into the book.
///
/// The naive `index + 1` is a footgun: a strip with no window yet can report a
/// sentinel, and the arithmetic can wrap — landing on page 0, which the library
/// would then persist as the reading position. Clamping makes a momentary
/// no-window read harmless instead of destructive.
pub(super) fn page_of_dominant(dominant: usize, num_pages: u32) -> u32 {
    let raw = dominant.saturating_add(1) as u64;
    raw.clamp(1, u64::from(num_pages.max(1))) as u32
}

/// How many quiet frames a scroll-driven page change waits before it is worth
/// reporting.
///
/// The app writes the whole library on a report, and a fling crosses a page many
/// times a second; a page turn is a keypress and reports at once.
const QUIET_FRAMES: u8 = 12;

/// What the app has been told about the reader's position, and the quiet a
/// scroll-driven change waits out before it is told again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Progress {
    /// The page the app already holds.
    told: u32,
    /// Frames since a scroll last moved the page.
    quiet: u8,
}

impl Default for Progress {
    fn default() -> Self {
        Self { told: 1, quiet: 0 }
    }
}

impl Progress {
    /// A document opened: the open's own record already wrote this page, so
    /// nothing is owed until the reader moves off it.
    pub(super) fn seed(&mut self, page: u32) {
        self.told = page;
        self.quiet = 0;
    }

    /// A scroll moved the page: the write is owed, and waits for the scroll to
    /// stop.
    pub(super) fn moved(&mut self) {
        self.quiet = 0;
    }

    /// One frame of quiet; true on the one frame the scroll has been still long
    /// enough to be worth writing down, and false after it — the write it asked
    /// for is the only one, and the frames that follow have nothing to say.
    pub(super) fn settled(&mut self) -> bool {
        if self.quiet >= QUIET_FRAMES {
            return false;
        }
        self.quiet += 1;
        self.quiet >= QUIET_FRAMES
    }

    /// Whether the write is still owed.
    pub(super) fn owes(&self, page: u32) -> bool {
        self.told != page
    }

    /// The page was reported.
    pub(super) fn told(&mut self, page: u32) {
        self.told = page;
        self.quiet = 0;
    }
}

/// The jump a page turn owes while a zoom transaction holds the geometry.
///
/// A turn landing mid-transaction cannot move the strip: the transaction is
/// rewriting its extents every frame, and a scroll commanded into that is
/// re-anchored onto the page the reader just left. Dropping the turn is worse —
/// it would never happen at all — so the jump is held here and taken on the
/// frame the transaction closes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct HeldJump {
    held: bool,
}

impl HeldJump {
    /// A turn landed mid-transaction: its jump is held, and the reader's newest
    /// page is the one it owes (the page itself is already written).
    pub(super) fn hold(&mut self) {
        self.held = true;
    }

    /// Whether a turn is waiting for the geometry to settle.
    pub(super) fn waiting(&self) -> bool {
        self.held
    }

    /// Take the held jump. `false` when nothing was waiting, which is what keeps
    /// a zoom commit from scrolling the page back under the reader's eyes.
    pub(super) fn take(&mut self) -> bool {
        core::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view-mode-change regression: a strip reporting a sentinel index (no
    /// window resolved yet) maps to a real page, never to 0.
    #[test]
    fn a_sentinel_dominant_index_never_becomes_page_zero() {
        assert_eq!(page_of_dominant(usize::MAX, 300), 300);
        assert_eq!(page_of_dominant(0, 300), 1);
        assert_eq!(page_of_dominant(999, 50), 50);
        assert_eq!(page_of_dominant(999, 0), 1);
        assert_eq!(page_of_dominant(41, 300), 42);
    }

    /// A transaction that closes with nothing held moves nothing — the invariant
    /// a zoom commit depends on.
    #[test]
    fn a_transaction_closing_alone_moves_nothing() {
        let mut gate = HeldJump::default();
        assert!(!gate.waiting());
        assert!(!gate.take(), "a commit with no turn behind it scrolls nothing");
    }

    /// A scroll's page is written once the scroll has stopped, and a page that
    /// comes back to where the app already holds it owes nothing.
    #[test]
    fn a_scrolls_page_is_written_once_it_is_quiet() {
        let mut progress = Progress::default();
        progress.seed(7);
        assert!(!progress.owes(7), "the open's own record already wrote this page");
        progress.moved();
        assert!(progress.owes(8), "the page the scroll landed on is owed");
        assert!(!progress.owes(7), "and the page the app already holds is not");
        for _ in 0..QUIET_FRAMES - 1 {
            assert!(!progress.settled(), "still moving");
        }
        assert!(progress.settled(), "quiet long enough");
        assert!(!progress.settled(), "and the frame it fires on is the only one");
        progress.told(8);
        assert!(!progress.owes(8), "the write landed");
        // Every scroll that moves the page again re-arms the one window rather
        // than queueing a write per page.
        progress.moved();
        progress.moved();
        for _ in 0..QUIET_FRAMES - 1 {
            assert!(!progress.settled());
        }
        assert!(progress.settled());
    }

    /// A turn mid-transaction waits for the commit, and the commit lands it once.
    #[test]
    fn a_turn_mid_transaction_lands_on_the_commit() {
        let mut gate = HeldJump::default();
        gate.hold();
        assert!(gate.waiting());
        assert!(gate.waiting(), "frames pass, the debt stands");
        assert!(gate.take(), "the commit takes it");
        assert!(!gate.take(), "and it is not taken twice");
    }
}
