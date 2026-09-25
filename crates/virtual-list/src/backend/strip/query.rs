//! The strip's read-only questions: offsets, sizes, and the windows of items
//! a scrollport needs.

use crate::window::{Budget, Window};

use super::{Strip, StripBackend};

impl Strip {
    /// Offset of the start of item `index`.
    ///
    /// Returns `0.0` for an empty strip, and the total extent for an index at
    /// or past the end, so callers can position a trailing spacer without a
    /// bounds check.
    #[inline]
    pub fn offset(&self, index: usize) -> f64 {
        StripBackend::offset(self, index)
    }

    /// Size of item `index`, or `0.0` if out of range.
    #[inline]
    pub fn size(&self, index: usize) -> f64 {
        StripBackend::size(self, index)
    }

    /// Total extent of the column: every item plus the gaps between them, with
    /// no trailing gap. `0.0` when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        StripBackend::total(self)
    }

    /// Average item extent — resolves [`crate::Overscan::Items`] budgets.
    pub fn mean_size(&self) -> f64 {
        StripBackend::mean_size(self)
    }

    /// Index of the item whose span contains `pos`.
    ///
    /// `pos` is treated as the *leading edge* of a viewport: an item ending
    /// exactly at `pos` has scrolled out, and a position inside a gap
    /// resolves to the item below — the same strict top edge
    /// [`overlapping`](Self::overlapping) uses, so "the item at the top of
    /// the scrollport" and "the first visible item" never disagree by one.
    /// Positions past the end resolve to the last item, so this always names
    /// a real item (given a non-empty strip); `0` when empty.
    ///
    /// `partition_point` over `i64` sub-pixels — `O(log n)` per call. For
    /// continuous scrolling prefer [`Strip::index_at_hinted`], amortized
    /// `O(1)` when the position is the same as or adjacent to the previous
    /// frame.
    pub fn index_at(&self, pos: f64) -> usize {
        StripBackend::index_at(self, pos)
    }

    /// [`Strip::index_at`] with a **hint** — the previous frame's result,
    /// checked first. Continuous scrolling (trackpad, wheel, line-scroll)
    /// almost always lands on the same index or one step away, reducing the
    /// `O(log n)` search to amortized `O(1)`. A hint wrong by more than a
    /// step or two (scrollbar drag, jump-to-anchor) falls back to a
    /// **galloping** search: probe 1, 2, 4, 8, ... steps away to bracket the
    /// answer, then binary-search inside it — worst case `O(log n)`, best
    /// case one integer comparison.
    ///
    /// The hint is updated in place so callers can keep it across frames.
    /// Delegates to the [`StripBackend`] override — the same galloping
    /// search every generic hinted windowing path over this strip runs.
    pub fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        StripBackend::index_at_hinted(self, pos, hint)
    }

    /// Inclusive range of items overlapping the span `[top, top + extent)`.
    ///
    /// An item that ends exactly at `top` has scrolled out and is excluded; an
    /// item that starts exactly at the bottom edge is also excluded because the
    /// lower bound is half-open. Returns `None` for an empty strip, a span with
    /// no extent, or when the span lies entirely within a gap.
    pub fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        crate::backend::overlapping(self, top, extent)
    }

    /// Inclusive range of items that are at least partly on screen.
    ///
    /// Shorthand for [`overlapping`](Self::overlapping) with the raw viewport.
    #[inline]
    pub fn visible(&self, scroll_top: f64, viewport: f64) -> Option<Window> {
        crate::backend::visible(self, scroll_top, viewport)
    }

    /// Inclusive range of items to keep mounted.
    ///
    /// The window is everything overlapping
    /// `[scroll_top - look, scroll_top + viewport + look]` where
    /// `look` is derived from [`Budget::overscan`], trimmed to
    /// `budget.max_items`.
    ///
    /// Two invariants hold for any `budget`:
    ///
    /// - every partly-visible item is always included, so no budget can blank
    ///   out what the reader is looking at;
    /// - trimming drops the item furthest from the viewport first and prefers
    ///   to keep the item below, so the next item the reader reaches is the
    ///   last one evicted.
    pub fn window(&self, scroll_top: f64, viewport: f64, budget: Budget) -> Option<Window> {
        crate::backend::window(self, scroll_top, viewport, budget)
    }

    /// [`Strip::window`] using a hinted overlap search (amortized O(1) when
    /// `hint` is the previous frame's first mounted index). Delegates to the
    /// [`StripBackend`] default — the shared hinted windowing, which seeds
    /// its leading-edge search with this strip's galloping `index_at_hinted`
    /// and shares the budget trim with the unhinted path.
    pub fn window_hinted(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        StripBackend::window_hinted(self, scroll_top, viewport, budget, hint)
    }

    /// Index of the item occupying most of the viewport (area-of-viewport).
    ///
    /// Not "the item at the top edge": shrinking every item (zooming out)
    /// slides more of the *previous* item into the top of the viewport, so
    /// the top-edge answer keeps changing even though the reader never moved.
    /// Area degrades gracefully at both extremes — one item filling the
    /// screen trivially wins; with several visible, the one you see most of
    /// wins — and a jump aligning item `i` with the top still reports `i`.
    ///
    /// Ties go to the lower index. Falls back to [`index_at`](Self::index_at)
    /// when the viewport has no extent.
    pub fn dominant(&self, scroll_top: f64, viewport: f64) -> usize {
        crate::backend::dominant(self, scroll_top, viewport)
    }

}
