//! Where the reader is, and how big the page under their eyes is drawn.
//!
//! A port of the web app's `ViewerSignals` — the page, the view mode, the fit
//! mode — plus the one piece of arithmetic that decides a scale: [`FitDims`],
//! which was `src/zoom/target.rs` in the web tree and is kept apart from the
//! state here for the same reason it was kept apart there. The seed scale at
//! open and the live refit on a window resize must answer with the same number,
//! and one definition of what a fit measures against is the only way they can.
//!
//! The scales themselves are deliberately NOT here: they belong to the zoom
//! pipeline ([`super::zoom`]), which keeps the reader's own, the one on screen
//! and the one the rasters are crisp at apart. The viewer answers "what does
//! this fit want, given this window and this sheet", and every caller hands it
//! the scale it is measuring from.

use iced::Size;
use reader_core::view::ViewMode;
use reader_core::zoom_math::{FitMode, clamp_scale, fit_scale};

use crate::formats::pdf::PageBox;

/// Everything about the reader's view of the document that is not a fact
/// about the document.
#[derive(Debug, Clone, Copy)]
pub struct Viewer {
    /// The 1-based page under the reader's eyes.
    pub page: u32,
    pub mode: ViewMode,
    /// The fit mode in force. `None` means the scale is the reader's own.
    pub fit: FitMode,
    /// The reading area, in CSS px: the window, less the space a docked rail
    /// takes. The title bar is an overlay, so it is not subtracted.
    pub container: Size,
    /// The display's scale factor — what turns a CSS-px box into the whole
    /// number of device pixels a frame is rasterised into.
    pub dpr: f64,
    /// The horizontal inset around a page, from the settings.
    pub margin: f64,
    /// The fixed chrome between pages in the continuous column: the settings'
    /// gap, zero when `no_gap` is on. It never scales with the pages.
    pub gap: f64,
    /// Whether a page turn may move the scale: on, arriving at a differently
    /// sized sheet re-resolves the fit (the web app's `auto_resize`); off, a
    /// page turn touches nothing and a wide plate overflows and scrolls.
    pub auto_resize: bool,
    /// Whether a mode flip may move the scale: on, a flip back into a paginated
    /// mode takes the width fit (the web app's `auto_scale`); off, the flip
    /// touches nothing and the reader's own scale is kept.
    pub auto_scale: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            page: 1,
            // The continuous column, which is the web app's own default: a
            // book opens into the whole document and the reader scrolls it.
            mode: ViewMode::default(),
            fit: FitMode::Width,
            container: Size::new(0.0, 0.0),
            dpr: 1.0,
            margin: 0.0,
            gap: crate::reader::strip::PAGE_GAP,
            auto_resize: true,
            auto_scale: true,
        }
    }
}

impl Viewer {
    /// Whether the container has been measured. A fit against an unmeasured
    /// window would slam the page to the minimum scale, so nothing is fitted
    /// until the window has said how big it is.
    pub fn measured(&self) -> bool {
        self.container.width > 1.0 && self.container.height > 1.0
    }

    /// The scale a fit mode wants right now, or the scale the caller measured
    /// from when no fit owns it or nothing can be measured yet.
    ///
    /// Every mode resolves through it, the continuous column included: a page in
    /// the strip is the same page, and the fit that owns the scale must answer
    /// with the one number wherever the mode puts it. (A reflowable document is
    /// the exception, and it is a fact about the DOCUMENT rather than the mode:
    /// there is no page to fit, the window *is* the page, and type size belongs
    /// to the typography settings. That branch arrives with the reflow
    /// increments.)
    pub fn resolved_scale(&self, box_: PageBox, current: f64) -> f64 {
        if !self.measured() {
            return clamp_scale(current);
        }
        match FitDims::from_geometry(self.mode, self.container, self.margin, box_) {
            Some(dims) => dims.fit(self.fit, current),
            None => clamp_scale(current),
        }
    }

    /// Back to the first page with nothing else moved.
    ///
    /// The web app's `reset_position`, and the split it drew: what is
    /// *document-scoped* goes (the page under the reader's eyes; the scroll, the
    /// selection and the mount anchor arrive with their own increments), and
    /// what is the *reader's* stays — the mode, the fit and the scale are the
    /// reader's own layout, and the next document inherits them.
    pub fn reset_position(&mut self) {
        self.page = 1;
    }

    /// The page's box in CSS px at `scale`: the settled box at the committed
    /// scale, and the stretched box a tween is passing through at the display
    /// one.
    pub fn page_px(&self, box_: PageBox, scale: f64) -> (f32, f32) {
        let w = box_.width * scale;
        let h = box_.height * scale;
        (w.max(1.0) as f32, h.max(1.0) as f32)
    }
}

/// The plain-geometry inputs of a fit computation, separated from the state so
/// the arithmetic is unit-testable on the host — the web app's `FitDims`,
/// ported with its tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitDims {
    /// Usable container width (the reader's margin removed), `>= 1`.
    pub cw_eff: f64,
    /// Usable container height (`>= 1`); the full window height in every mode,
    /// the title bar being a hover-revealed overlay rather than a band.
    pub ch_eff: f64,
    /// The sheet being fitted. A spread doubles the width it has to fit; the
    /// horizontal strip still lays out one page per item.
    pub pw_eff: f64,
    pub ph_eff: f64,
    /// Whether the strip runs horizontally. In that mode Fit Page uses the
    /// viewport's height, while Fit Width still means the width of one page.
    pub horizontal: bool,
}

impl FitDims {
    /// The fit geometry for a page in a container. `None` when either
    /// container dimension is unmeasured — fitting to a placeholder would slam
    /// the page to the minimum scale, which is what the web app's guard was
    /// for.
    pub fn from_geometry(
        mode: ViewMode,
        container: Size,
        margin: f64,
        box_: PageBox,
    ) -> Option<Self> {
        let (cw, ch) = (f64::from(container.width), f64::from(container.height));
        if !(cw > 1.0 && ch > 1.0) {
            return None;
        }
        Some(Self {
            cw_eff: (cw - 2.0 * margin).max(1.0),
            ch_eff: ch.max(1.0),
            pw_eff: if mode == ViewMode::Spread {
                box_.width * 2.0
            } else {
                box_.width
            },
            ph_eff: box_.height,
            horizontal: mode == ViewMode::ScrollHorizontal,
        })
    }

    /// The scale a fit mode wants. The horizontal strip has one page per item:
    /// Fit Width uses that page's width, while Fit Page keeps the height-fit
    /// behaviour that shows a whole page at once.
    pub fn fit(&self, fit: FitMode, current: f64) -> f64 {
        if self.horizontal && fit == FitMode::Page {
            return clamp_scale(self.ch_eff / self.ph_eff.max(1.0));
        }
        fit_scale(fit, self.cw_eff, self.ch_eff, self.pw_eff, self.ph_eff, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::kit::page;

    fn viewer(mode: ViewMode, cw: f32, ch: f32, margin: f64) -> Viewer {
        Viewer {
            mode,
            margin,
            container: Size::new(cw, ch),
            ..Viewer::default()
        }
    }

    #[test]
    fn a_fit_width_spans_the_container_with_no_leftover_pan_space() {
        let v = viewer(ViewMode::Single, 1000.0, 800.0, 0.0);
        assert!((v.resolved_scale(page(500.0, 700.0), 1.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_reader_margin_comes_off_the_width_only() {
        let v = viewer(ViewMode::Single, 1000.0, 800.0, 20.0);
        // 1000 - 2 x 20 is the usable width; the height is untouched.
        assert!((v.resolved_scale(page(480.0, 700.0), 1.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn fit_page_takes_the_smaller_of_the_two_ratios() {
        let mut v = viewer(ViewMode::Single, 1000.0, 400.0, 0.0);
        v.fit = FitMode::Page;
        // The height is the binding constraint here: 400/700 < 1000/500.
        assert!((v.resolved_scale(page(500.0, 700.0), 1.0) - 400.0 / 700.0).abs() < 1e-9);
    }

    #[test]
    fn a_spread_fits_two_pages_across() {
        let single = viewer(ViewMode::Single, 1024.0, 768.0, 0.0);
        let spread = viewer(ViewMode::Spread, 1024.0, 768.0, 0.0);
        let twice = spread.resolved_scale(page(612.0, 792.0), 1.0) * 2.0;
        assert!((twice - single.resolved_scale(page(612.0, 792.0), 1.0)).abs() < 1e-9);
    }

    #[test]
    fn an_unmeasured_window_is_not_fitted() {
        // The window has not reported its size yet: the page keeps the scale it
        // has rather than being slammed to the minimum by a fit against zero.
        let v = Viewer::default();
        assert!(!v.measured());
        assert!((v.resolved_scale(page(500.0, 700.0), 1.4) - 1.4).abs() < 1e-9);
    }

    #[test]
    fn the_column_fits_its_pages_like_any_other_mode() {
        // The continuous strip resolves through the same fit: a page in the
        // column is the same page, and the fit that owns the scale has to answer
        // with one number wherever the mode puts it.
        let v = viewer(ViewMode::ScrollVertical, 1000.0, 800.0, 0.0);
        assert!((v.resolved_scale(page(500.0, 700.0), 1.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn leaving_a_book_takes_the_page_and_keeps_the_reader_s_own_layout() {
        // What is the reader's own survives a close — the mode, the fit and the
        // margin here, the scales in `Zoom` (see the reader's own test).
        let mut v = viewer(ViewMode::Single, 1000.0, 800.0, 12.0);
        v.page = 40;
        v.fit = FitMode::Page;
        v.reset_position();
        assert_eq!(v.page, 1);
        assert_eq!(v.fit, FitMode::Page, "the fit they chose survives the book");
        assert_eq!(v.margin, 12.0);
    }

    #[test]
    fn a_page_box_on_screen_is_the_box_at_the_scale() {
        let v = Viewer::default();
        assert_eq!(v.page_px(page(600.0, 800.0), 1.5), (900.0, 1200.0));
    }

    #[test]
    fn a_degenerate_page_still_occupies_a_pixel() {
        // A page Pdfium could not measure must not lay out as nothing; the
        // reader answers with the A4 fallback before this point, and the guard
        // is here for the paths that reach it directly.
        let v = Viewer::default();
        assert_eq!(v.page_px(page(0.0, 0.0), 1.0), (1.0, 1.0));
    }

    #[test]
    fn the_ladder_clamps_a_fit_that_would_leave_the_page_unreadable() {
        // A one-pixel-wide window would ask for a scale far below the minimum.
        let v = viewer(ViewMode::Single, 2.0, 2.0, 0.0);
        let floor = reader_core::zoom_math::MIN_SCALE;
        assert!((v.resolved_scale(page(1000.0, 1400.0), 1.0) - floor).abs() < 1e-9);
    }
}
