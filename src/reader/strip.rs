//! The continuous strip: the ported windowing, the fixed chrome between pages,
//! the position the reader's own scroll sits at, and the aim that lands a mount.
//!
//! Three facts are kept apart on purpose, because they disagree for a frame or
//! two after anything moves:
//!
//! * the WINDOWING — every page's extent at the live scale, from
//!   `virtual-list` — is what the surface's spacers and the raster pump read;
//! * the OFFSET is what the reader believes the strip is scrolled to, written by
//!   the reader's own commands and by the surface's reports;
//! * the AIM ([`Anchor`]) is a position that was commanded and that the surface
//!   has not reported back yet.
//!
//! The web app's strip did the same job out of DOM measurements; the ported
//! windowing already knows every extent from the page boxes, so nothing here
//! waits on a layout report to know how tall the book is.

use std::cell::Cell;

use reader_core::view::{Axis, ViewMode};
use virtual_list::backend::strip::Strip as Windowing;
use virtual_list::{Budget, Window};

/// The gap between pages in the continuous column, in CSS px. `no_gap` takes it
/// to nothing; it is fixed chrome and never scales with the pages.
pub(super) const PAGE_GAP: f64 = 24.0;

/// Comfortable read-ahead: half a window each way, at most three pages mounted —
/// what is on screen and about one either side. Each mounted page is a whole
/// raster, so this ceiling is the reader's engine queue as well as its memory.
const BUDGET: Budget = Budget::screenfuls(0.5, 3);

/// How many frames an aim is posted for, and how close a report has to be to it
/// to count as the aim arriving.
const ANCHOR_POSTS: u8 = 3;
const LANDED_EPSILON: f64 = 1.0;

/// The axis a mode scrolls along.
pub(super) fn axis_of(mode: ViewMode) -> Axis {
    if mode == ViewMode::ScrollHorizontal {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

/// Where a jump puts the page: the column aligns its head with the window's, the
/// horizontal strip centres it — the web app's own per-axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Landing {
    Head,
    Middle,
}

impl Landing {
    /// The landing a mode uses.
    pub(super) fn of(mode: ViewMode) -> Self {
        match axis_of(mode) {
            Axis::Vertical => Landing::Head,
            Axis::Horizontal => Landing::Middle,
        }
    }

    /// Where an item of `size` starting at `start` sits for the page to land.
    fn place(self, start: f64, size: f64, viewport: f64) -> f64 {
        match self {
            Landing::Head => start,
            Landing::Middle => start - (viewport - size) / 2.0,
        }
    }
}

/// Everything the windowing and the page boxes are built from. A strip is
/// rebuilt only when one of these moves — which is also what makes a zoom and a
/// rescale the same rebuild as a mode change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Geometry {
    pub mode: ViewMode,
    pub pages: u32,
    /// The scale the pages are drawn at: the LIVE one, so a zoom moves the pages
    /// on the same frame it moves the scale.
    pub scale: f64,
    /// The fixed chrome between two pages: the column's gap, or the two margins
    /// a horizontal strip's pages are inset by.
    pub gap: f64,
    /// The chrome before the first page and after the last: a horizontal
    /// strip's papers are inset by the reader's margin at both ends, a column's
    /// run edge to edge.
    pub inset: f64,
}

/// A position the strip was told to sit at.
///
/// The posts are bounded and a report that agrees with the aim releases them: an
/// iced scroll offset is clamped against the bounds the widget had when the
/// command landed, and a strip's bounds only grow a frame after the geometry
/// that moved them — so the first post of a mount or a rescale can be clamped
/// away, and one that lands is echoed back. Past the posts, a report that still
/// disagrees is the reader's own hand: the aim is dropped and the report wins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Anchor {
    target: f64,
    posts: u8,
}

impl Anchor {
    pub(super) fn aim(target: f64) -> Self {
        Self {
            target,
            posts: ANCHOR_POSTS,
        }
    }

    /// The offset to post again, while the aim still has posts left.
    pub(super) fn repost(&mut self) -> Option<f64> {
        if self.posts == 0 {
            return None;
        }
        self.posts -= 1;
        Some(self.target)
    }

    /// Whether this report is the aim itself, arrived.
    pub(super) fn landed(&self, offset: f64) -> bool {
        (offset - self.target).abs() <= LANDED_EPSILON
    }

    /// Whether the aim is out of posts — the point past which the surface's
    /// reports are the reader's again.
    pub(super) fn spent(&self) -> bool {
        self.posts == 0
    }
}

/// One continuous book: every page's extent at the scale they are drawn at, and
/// the position the reader's own scroll sits at.
#[derive(Debug)]
pub(super) struct Strip {
    windowing: Windowing,
    geometry: Geometry,
    offset: f64,
    /// The mounted window, kept so the surface's spacers and the raster pump
    /// read the one answer.
    mounted: Option<Window>,
    /// The windowing search's seed: the last frame's first mounted item.
    hint: Cell<usize>,
}

impl Strip {
    pub(super) fn new(geometry: Geometry, windowing: Windowing) -> Self {
        Self {
            windowing,
            geometry,
            offset: 0.0,
            mounted: None,
            hint: Cell::new(0),
        }
    }

    pub(super) fn geometry(&self) -> Geometry {
        self.geometry
    }

    pub(super) fn axis(&self) -> Axis {
        axis_of(self.geometry.mode)
    }

    pub(super) fn offset(&self) -> f64 {
        self.offset
    }

    /// The window the surface mounts, if it has one: a book with no pages has
    /// nothing to mount and nothing to sync.
    pub(super) fn mounted(&self) -> Option<Window> {
        self.mounted
    }

    /// Where an item starts, in the coordinates the surface and the commands
    /// speak, and how big it is along the main axis.
    pub(super) fn span(&self, index: usize) -> (f64, f64) {
        (
            self.geometry.inset + self.windowing.offset(index),
            self.windowing.size(index),
        )
    }

    /// The strip's whole extent, leading and trailing chrome included.
    pub(super) fn total(&self) -> f64 {
        self.windowing.total() + 2.0 * self.geometry.inset
    }

    /// The highest offset the strip can hold: its content, less a window.
    pub(super) fn max_offset(&self, viewport: f64) -> f64 {
        (self.total() - viewport).max(0.0)
    }

    /// Whether this report adds nothing to where the reader believes they are.
    pub(super) fn agrees(&self, offset: f64) -> bool {
        (offset - self.offset).abs() <= LANDED_EPSILON
    }

    /// Take a position as the reader's own — a report from the surface, or a
    /// command about to be posted. Clamped to what the strip can reach, which is
    /// what the surface does with it either way, so the belief and the surface
    /// cannot drift apart.
    pub(super) fn adopt(&mut self, offset: f64, viewport: f64) -> f64 {
        self.offset = offset.clamp(0.0, self.max_offset(viewport));
        self.offset
    }

    /// The 0-based item of a 1-based page, clamped into the book.
    pub(super) fn index(&self, page: u32) -> usize {
        (page.max(1) - 1) as usize
    }

    /// The offset that puts `page` in front of the reader.
    pub(super) fn offset_of(&self, page: u32, viewport: f64, landing: Landing) -> f64 {
        let index = self.index(page).min(self.windowing.len().saturating_sub(1));
        let (start, size) = self.span(index);
        landing
            .place(start, size, viewport)
            .clamp(0.0, self.max_offset(viewport))
    }

    /// The 0-based page under the middle of the window — the item the reader is
    /// actually looking at.
    pub(super) fn dominant(&self, viewport: f64) -> usize {
        // The windowing's own frame starts at the first item; the reader's starts
        // at the leading chrome.
        self.windowing
            .dominant(self.offset - self.geometry.inset, viewport)
    }

    /// Resolve the mounted window for this frame.
    pub(super) fn rewindow(&mut self, viewport: f64) {
        let mut hint = self.hint.get();
        // The windowing's own frame, so the window is the reader's window.
        let at = self.offset - self.geometry.inset;
        self.mounted = self.windowing.window_hinted(at, viewport.max(1.0), BUDGET, &mut hint);
        self.hint.set(hint);
    }

    /// Move onto a new geometry, holding the document point under the middle of
    /// the window still, and answer where the strip now sits.
    ///
    /// The pages scale and the chrome does not, so the point is carried through
    /// [`reader_core::view::anchored_position`] rather than by scaling the
    /// offset: a zoom would otherwise walk the reader out of their place a page
    /// gap at a time.
    pub(super) fn rescale(&mut self, geometry: Geometry, windowing: Windowing, viewport: f64) -> f64 {
        let middle = viewport / 2.0;
        let centre = self.offset + middle;
        // Into the windowing's own frame, which starts at the first item.
        let index = self.windowing.index_at(centre - self.geometry.inset);
        let (above, size) = self.span(index);
        let gap = self.windowing.gap();
        let factor = if self.geometry.scale > 0.0 {
            geometry.scale / self.geometry.scale
        } else {
            1.0
        };
        let anchored = reader_core::view::anchored_position(
            size,
            above,
            above - index as f64 * gap,
            gap,
            centre,
            factor,
            index,
        );
        self.windowing = windowing;
        self.geometry = geometry;
        self.adopt(anchored - middle, viewport)
    }
}

#[cfg(test)]
mod tests;
