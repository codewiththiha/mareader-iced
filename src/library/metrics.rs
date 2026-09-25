//! The level's arithmetic: how many columns fit, how wide the content is, and
//! whether the view still reports the width it was told.

use library_core::view::LibraryView;

/// The grid's geometry, straight off grid.css: 152px tracks with a 24px
/// gutter decide how many columns a width holds, rows sit 32px apart, and
/// the level's content keeps to a 1152px column with 24px of air either
/// side.
pub(super) const TRACK_MIN: f32 = 152.0;

pub(super) const COL_GAP: f32 = 24.0;

pub(super) const CONTENT_MAX: f32 = 1152.0;

pub(super) const CONTENT_PAD: f32 = 24.0;

// The level's vertical rhythm, and the reveal's too: a scroll to a cell is
// asking these to hold, so both read the one copy.
pub(super) const ROW_GAP: f32 = 32.0;

pub(super) const CONTENT_TOP: f32 = 32.0;

/// How many tracks fit a width: the grid.css `minmax(9.5rem, 1fr)`
/// auto-fit rule, floored.
pub(super) fn auto_columns(width: f32) -> usize {
    let tracks = ((width + COL_GAP) / (TRACK_MIN + COL_GAP)).floor() as i64;
    tracks.max(1) as usize
}

/// The grid's own pair of layout facts: the track count and the cell the
/// width says — one spelling so the reveal's offset and the scaled cells
/// come out of the same numbers.
pub fn content_metrics(display_width: f32, pinned: Option<u8>) -> (usize, f32) {
    grid_metrics(content_width(display_width), pinned)
}

/// The content column's width for a window: the level's air on both sides,
/// shared by the grid and the reveal's offsets.
pub(super) fn content_width(display_width: f32) -> f32 {
    (display_width.min(CONTENT_MAX) - CONTENT_PAD * 2.0).max(TRACK_MIN)
}

/// The track count and cell width for a column, by the same arithmetic at
/// every caller: the grid paints with these, and `content_metrics` hands the
/// identical pair to the reveal.
pub(super) fn grid_metrics(inner_width: f32, pinned: Option<u8>) -> (usize, f32) {
    let tracks = match pinned {
        Some(count) => usize::from(count).max(1),
        None => auto_columns(inner_width),
    };
    let cell = (inner_width - COL_GAP * (tracks as f32 - 1.0)) / tracks as f32;
    (tracks, cell)
}

/// The view's live column count for a window width — the report the grid
/// sends back to the view so the stepper's `+` starts from what the shelf
/// shows. Only asked while the columns are on auto.
pub fn report_fit(view: &mut LibraryView, window_width: f32) -> bool {
    if view.columns.is_some() {
        return false;
    }
    let raw = (auto_columns(content_width(window_width)) as i64).clamp(1, 255) as u8;
    let fit = LibraryView::clamped_fit(raw);
    if view.auto_fit == fit {
        return false;
    }
    view.report_auto_fit(fit);
    true
}
