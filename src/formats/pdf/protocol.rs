//! The engine's wire: what the reading surface asks for, and what comes back.
//!
//! The web app crossed a worker boundary to reach pdf.js and got promises
//! back; natively the engine is a thread in this process and the same contract
//! is a pair of plain enums. They stay plain on purpose: no widget types, no
//! locks, nothing that only means something inside the app — the engine's own
//! tests drive the protocol directly, and the worker never sees a surface.
//!
//! Two addresses travel with a request, and they answer different questions:
//!
//! * the **stamp** says which open a message belongs to. Opening a second
//!   document while the first is still resolving is ordinary use; without the
//!   stamp, the loser's answer would seed the winner's page count and resume
//!   the reader at a page that was never in that book.
//! * the **[`FrameKey`]** says which raster is wanted: the page, and the whole
//!   number of device pixels it is to occupy. A page turn, a resize or a
//!   scale-factor change makes the raster in flight obsolete, and the reader
//!   simply stops looking for it — it asks for the key it wants *now*, and an
//!   answer whose key no longer matches is dropped where it lands. That is
//!   pdf.js's per-canvas render bookkeeping, replaced by a value that can be
//!   compared.

use std::path::PathBuf;

/// A page's box in PDF points, which is also its CSS-px box at scale 1: the
/// web app's `page1Size` and `pageHeights` came off pdf.js's scale-1 viewport,
/// where one point is one CSS pixel, and Pdfium reports the same points.
///
/// Called a *box* rather than a size, because a size is what the window and
/// the container have; a box belongs to a sheet of paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBox {
    pub width: f64,
    pub height: f64,
}

/// The sheet the reader falls back to when a document declares nothing usable:
/// A4 in points (210 × 297 mm at 72 dpi), which is the same sheet A4 is in the
/// reflowable family's geometry.
pub const A4_WIDTH: f64 = 595.276;
pub const A4_HEIGHT: f64 = 841.89;

/// A page's box is measured when both sides are positive and finite. Pdfium
/// answers zeroes for a page it cannot describe, and a zero would otherwise
/// reach the fit arithmetic as a division.
pub fn measured(box_: PageBox) -> bool {
    box_.width > 0.0 && box_.height > 0.0 && box_.width.is_finite() && box_.height.is_finite()
}

/// The A4 fallback, as a box.
pub fn fallback_box() -> PageBox {
    PageBox {
        width: A4_WIDTH,
        height: A4_HEIGHT,
    }
}

/// One rasterisation of one page: the page's number and the device-pixel
/// raster it belongs in.
///
/// The key *is* the request and the answer, which is what makes a stale frame
/// harmless. A frame for the page on screen is kept even when the scale has
/// moved on — it is stretched until its replacement lands, which is what a
/// reader expects to see mid-gesture. A frame for any other page is dropped
/// where it lands, and never painted over the page under the reader's eyes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey {
    pub page: u32,
    /// Device pixels, not CSS px: the width the raster is allocated at, which
    /// is the page's CSS width times the scale times the display's ratio.
    pub width: u32,
    pub height: u32,
}

/// The largest raster the engine will make, on either side. Well past the
/// width of any display and well past any page's own proportions at 5×: the
/// guard is there so a deep zoom on a large display asks for a frame the
/// machine can still allocate rather than one that swaps it.
pub const MAX_DEVICE_PX: u32 = 12_000;

/// One side of a raster, in device pixels: the CSS length times the display's
/// ratio, rounded to the whole pixels a bitmap is allocated in, never below
/// one and never past [`MAX_DEVICE_PX`].
pub fn device_px(css: f64, dpr: f64) -> u32 {
    let ratio = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let pixels = (css * ratio).round();
    if pixels.is_finite() {
        (pixels.max(1.0) as u32).min(MAX_DEVICE_PX)
    } else {
        1
    }
}

impl FrameKey {
    /// The raster for a box measured in logical px — the length the layout is
    /// about to draw, rather than a page box and a scale.
    ///
    /// There is one constructor on purpose, and it is this one, because the
    /// device grid is the whole point: the reader snaps the box it draws to
    /// whole device pixels *before* it asks, so the raster comes back exactly
    /// as wide as the rectangle it fills instead of a fraction of a pixel away
    /// from it. A page box and a scale would have to be multiplied out to the
    /// same length here anyway — and any caller that computed that product
    /// itself would be one rounding away from asking for a raster it cannot
    /// draw square.
    pub fn of_css(page: u32, width: f64, height: f64, dpr: f64) -> Self {
        Self {
            page,
            width: device_px(width, dpr),
            height: device_px(height, dpr),
        }
    }
}

/// Everything the reader needs the moment a document opens: what the web app's
/// `OpenResult` carried, filled from Pdfium rather than from pdf.js.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub path: PathBuf,
    pub num_pages: u32,
    /// Per page, in document order. A page whose box Pdfium refused carries
    /// `0 × 0`; the reader treats that as "unmeasured" and falls back to page
    /// 1, which is what the web app did when its two arrays and the page count
    /// disagreed.
    pub page_sizes: Vec<PageBox>,
    /// The document's own metadata, which is *not* what the title bar shows:
    /// the library's name for the book wins over the file's, exactly as it did
    /// on the shelf.
    pub title: Option<String>,
    pub author: Option<String>,
}

impl Opened {
    /// Page 1's box, or the A4 fallback a document with no measurable page
    /// gets. The reading surface asks for this before any frame has landed.
    pub fn first_page(&self) -> PageBox {
        self.page_sizes
            .first()
            .copied()
            .filter(|box_| measured(*box_))
            .unwrap_or_else(fallback_box)
    }
}

/// What the reading surface asks of the engine.
#[derive(Debug, Clone)]
pub enum Request {
    /// Load a document, replacing whatever was open.
    Open { stamp: u64, path: PathBuf },
    /// Let the open document go. The engine keeps its binding and stays ready:
    /// closing back to the shelf and opening another book is the ordinary way
    /// through the app. Nothing is answered, so there is nothing to stamp.
    Close,
    /// Rasterise one page into a box of whole device pixels.
    Frame { stamp: u64, key: FrameKey },
    /// Extract a page's text, grouped into runs. Asked for lazily (the search
    /// index is built on the first search), never during an open.
    #[allow(dead_code)] // the lane is ported and engine-tested; the search asks next
    Text { stamp: u64, page: u32 },
    /// The document's chapter tree. Asked for once, after the first page is on
    /// screen rather than during the open: a textbook's outline means a
    /// bookmark walk, and the sidebar that shows it arrives in 3d.
    Outline { stamp: u64 },
}

/// What the engine answers.
#[derive(Debug, Clone)]
pub enum Event {
    /// The engine's first word: whether a Pdfium library was found at all.
    /// Sent once, before any request is served — so a machine without Pdfium
    /// gets a reader that says so, instead of one that waits on frames that
    /// nobody is left to render.
    Bound { result: Result<(), String> },
    Opened { stamp: u64, opened: Opened },
    OpenFailed { stamp: u64, message: String },
    /// One page's pixels: RGBA, four bytes to the pixel, `width * height * 4`
    /// long. The size is the raster's own, which is the requested box in every
    /// case but a cap.
    Frame {
        stamp: u64,
        key: FrameKey,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    FrameFailed {
        stamp: u64,
        key: FrameKey,
        message: String,
    },
    #[allow(dead_code)] // the lane is ported and engine-tested; the search asks next
    PageText {
        stamp: u64,
        page: u32,
        runs: Vec<super::text::Run>,
    },
    #[allow(dead_code)] // the lane is ported and engine-tested; the search asks next
    TextFailed {
        stamp: u64,
        page: u32,
        message: String,
    },
    /// The chapter tree, flattened in document order — the shape
    /// `pdf_core::outline::to_nodes` cleans into the reader's own nodes.
    Outline {
        stamp: u64,
        entries: Vec<pdf_core::outline::OutlineEntry>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: f64, h: f64) -> PageBox {
        PageBox {
            width: w,
            height: h,
        }
    }

    #[test]
    fn a_raster_is_a_whole_number_of_device_pixels() {
        // 612 × 792 pt at 100%, on a 1× display: the page's own box.
        assert_eq!(device_px(612.0, 1.0), 612);
        assert_eq!(device_px(792.0, 1.0), 792);
        // The same page on a 2× display, fitted to an 800 px column: 800/612 =
        // 1.3068…, and both sides round to whole pixels of the grid.
        assert_eq!(device_px(800.0, 2.0), 1600);
        assert_eq!(device_px(792.0 * (800.0 / 612.0), 2.0), 2071);
        let key = FrameKey::of_css(2, 800.0, 792.0 * (800.0 / 612.0), 2.0);
        assert_eq!(key, FrameKey { page: 2, width: 1600, height: 2071 });
        assert_eq!(key.width as usize * key.height as usize * 4, 1600 * 2071 * 4);
    }

    #[test]
    fn the_box_a_raster_fills_is_the_box_it_was_asked_for() {
        // The round trip is the whole point of the key: a raster asked for in
        // device pixels occupies the logical box the fit resolved, so the page
        // cannot shift by a fraction of a pixel when its frame lands.
        let css = (595.276 * 1.25, 841.89 * 1.25);
        let key = FrameKey::of_css(3, css.0, css.1, 1.5);
        let back = (f64::from(key.width) / 1.5, f64::from(key.height) / 1.5);
        assert!((back.0 - css.0).abs() <= 1.0 / 1.5);
        assert!((back.1 - css.1).abs() <= 1.0 / 1.5);
    }

    #[test]
    fn a_degenerate_ask_still_produces_a_pixel() {
        assert_eq!(device_px(0.0, 2.0), 1);
        assert_eq!(device_px(f64::NAN, 1.0), 1);
        // A display that reported no ratio (some headless environments do)
        // must not turn a page into nothing.
        assert_eq!(device_px(100.0, 0.0), 100);
        let key = FrameKey::of_css(1, 0.0, 0.0, 0.0);
        assert_eq!((key.width, key.height), (1, 1));
    }

    #[test]
    fn the_ceiling_is_where_the_memory_is() {
        // 5× on a 4K-wide display: the sides are capped rather than the machine
        // being asked for gigabytes of RGBA.
        assert_eq!(device_px(2000.0 * 5.0, 2.0), MAX_DEVICE_PX);
        assert_eq!(device_px(3000.0 * 5.0, 2.0), MAX_DEVICE_PX);
    }

    #[test]
    fn the_first_page_is_the_geometry_a_fit_can_use() {
        let opened = Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: 3,
            page_sizes: vec![page(612.0, 792.0), page(1224.0, 792.0), page(0.0, 0.0)],
            title: Some("One".to_string()),
            author: None,
        };
        assert_eq!(opened.first_page().width, 612.0);
        assert!(measured(opened.first_page()));
    }

    #[test]
    fn a_document_with_no_measured_page_still_answers() {
        // A file whose pages Pdfium refuses entirely: the reader gets A4 — a
        // page it can lay out — rather than a division by zero.
        let opened = Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: 4,
            page_sizes: vec![page(0.0, 0.0); 4],
            title: None,
            author: None,
        };
        let box_ = opened.first_page();
        assert!(measured(box_));
        assert_eq!(box_.width, A4_WIDTH);
        // And a book with no geometry at all answers the same way: the fit has
        // something to divide by from the first frame.
        let empty = Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: 2,
            page_sizes: Vec::new(),
            title: None,
            author: None,
        };
        assert_eq!(empty.first_page(), fallback_box());
    }
}
