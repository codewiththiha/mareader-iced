//! What a navigation key MEANS in the mode it lands in, as a pure function —
//! the web app's `keymap::resolve`, ported with its cases.
//!
//! One of the reference's inputs has no native counterpart and is gone with it:
//! the DOM's "Space activates the focused button". iced buttons are clicked, not
//! keyed, so there is no second owner of the key to guard against.

use iced::keyboard::{Key, key};

use reader_core::view::ViewMode;

/// Which way a navigation key points: `-1` back/up/left, `1` forward/down/right.
type Dir = i32;

/// One thing the reader asked the strip to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Nav {
    /// Turn to the page before the one the reader is on.
    PagePrev,
    /// Turn to the page after it.
    PageNext,
    /// A reading nudge along the strip's own axis.
    Line(Dir),
    /// A near-screen step along it.
    PageStep(Dir),
}

/// Resolve a keypress against the mode it landed in. `None` is a key the reader
/// has no use for.
///
/// Left/right turn pages everywhere except the horizontal strip, where they are
/// its axis; up/down turn pages in the paginated modes and nudge the column in
/// the continuous one; the page keys step whichever strip scrolls and do nothing
/// at all in the paginated modes, where a page is already a screen.
pub(super) fn resolve(key: &Key, shift: bool, mode: ViewMode) -> Option<Nav> {
    match key {
        Key::Named(key::Named::ArrowLeft) => {
            Some(if mode == ViewMode::ScrollHorizontal {
                Nav::Line(-1)
            } else {
                Nav::PagePrev
            })
        }
        Key::Named(key::Named::ArrowRight) => {
            Some(if mode == ViewMode::ScrollHorizontal {
                Nav::Line(1)
            } else {
                Nav::PageNext
            })
        }
        Key::Named(key::Named::ArrowUp) => match mode {
            ViewMode::ScrollVertical => Some(Nav::Line(-1)),
            mode if mode.is_paginated() => Some(Nav::PagePrev),
            _ => None,
        },
        Key::Named(key::Named::ArrowDown) => match mode {
            ViewMode::ScrollVertical => Some(Nav::Line(1)),
            mode if mode.is_paginated() => Some(Nav::PageNext),
            _ => None,
        },
        Key::Named(key::Named::PageUp) => mode.can_scroll().then_some(Nav::PageStep(-1)),
        Key::Named(key::Named::PageDown) => mode.can_scroll().then_some(Nav::PageStep(1)),
        Key::Named(key::Named::Space) => mode
            .can_scroll()
            .then_some(Nav::PageStep(if shift { -1 } else { 1 })),
        _ => None,
    }
}

/// One arrow tap is a reading nudge, not a page jump: 8% of the window, clamped
/// to the band a line scroll lives in. The web app arrived at the same numbers
/// after 15% made the arrows feel like paging.
pub(super) fn line_step(viewport: f64) -> f64 {
    (viewport * 0.08).clamp(40.0, 80.0)
}

/// The page keys: most of a window, with a sliver of overlap so the reader does
/// not lose the line they just read.
pub(super) fn page_step(viewport: f64) -> f64 {
    (viewport * 0.9).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(named: key::Named) -> Key {
        Key::Named(named)
    }

    #[test]
    fn arrows_turn_pages_in_the_paginated_modes() {
        for mode in [ViewMode::Single, ViewMode::Spread] {
            let down = resolve(&key(key::Named::ArrowDown), false, mode);
            assert_eq!(down, Some(Nav::PageNext));
            assert_eq!(resolve(&key(key::Named::ArrowUp), false, mode), Some(Nav::PagePrev));
            assert_eq!(resolve(&key(key::Named::ArrowLeft), false, mode), Some(Nav::PagePrev));
        }
    }

    #[test]
    fn arrows_scroll_the_strip_in_the_continuous_modes() {
        let vertical = ViewMode::ScrollVertical;
        let across = ViewMode::ScrollHorizontal;
        assert_eq!(resolve(&key(key::Named::ArrowDown), false, vertical), Some(Nav::Line(1)));
        assert_eq!(resolve(&key(key::Named::ArrowRight), false, across), Some(Nav::Line(1)));
        // Left/right still turn pages while the column is the scroll axis, and
        // up/down do nothing in the horizontal strip: its axis is the other one.
        assert_eq!(resolve(&key(key::Named::ArrowRight), false, vertical), Some(Nav::PageNext));
        assert_eq!(resolve(&key(key::Named::ArrowDown), false, across), None);
    }

    #[test]
    fn space_pages_the_strip_and_shift_space_pages_back() {
        let vertical = ViewMode::ScrollVertical;
        assert_eq!(resolve(&key(key::Named::Space), false, vertical), Some(Nav::PageStep(1)));
        assert_eq!(resolve(&key(key::Named::Space), true, vertical), Some(Nav::PageStep(-1)));
        assert_eq!(resolve(&key(key::Named::Space), false, ViewMode::Single), None);
    }

    #[test]
    fn the_page_keys_step_the_strip_and_are_dead_in_the_paginated_modes() {
        assert_eq!(resolve(&key(key::Named::PageDown), false, ViewMode::Single), None);
        assert_eq!(
            resolve(&key(key::Named::PageUp), false, ViewMode::ScrollVertical),
            Some(Nav::PageStep(-1))
        );
    }

    #[test]
    fn an_unmapped_key_is_left_alone() {
        let key = Key::Character("q".into());
        assert_eq!(resolve(&key, false, ViewMode::ScrollVertical), None);
    }

    #[test]
    fn a_line_step_is_a_reading_nudge_and_a_page_step_is_most_of_a_window() {
        assert!((line_step(900.0) - 72.0).abs() < 0.01);
        assert_eq!(line_step(200.0), 40.0, "never smaller than a native line");
        assert_eq!(line_step(2000.0), 80.0, "never a sixth of a tall window");
        assert!((page_step(800.0) - 720.0).abs() < 0.01);
        assert_eq!(page_step(0.0), 1.0, "a step is never nothing");
    }
}
