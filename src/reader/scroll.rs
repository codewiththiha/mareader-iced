//! What the reader does with the strip: mounts it on their page, follows their
//! own scrolling in and out of the page signal, and moves it when a turn, a zoom
//! or a page key asks.

use reader_core::view::{Axis, ViewMode};
use virtual_list::backend::strip::Strip as Windowing;

use super::motion::Glide;
use super::strip::{axis_of, Anchor, Geometry, Landing, Strip};
use super::sync::page_of_dominant;
use super::{Effect, Reader};

/// How far one wheel notch moves the strip, in CSS px — iced's own step for a
/// wheel event, kept so a translated axis feels like the one it borrowed.
const WHEEL_LINE_PX: f64 = 60.0;

impl Reader {
    /// The strip, as the surface draws it.
    pub(super) fn strip_view(&self) -> Option<&Strip> {
        self.strip.as_ref()
    }

    /// Whether a strip belongs on screen: a scrolling mode with a book open.
    pub(super) fn scrolls(&self) -> bool {
        self.viewer.mode.can_scroll() && self.document.status.is_ready()
    }

    /// The window along the strip's own axis: the reading area is the window and
    /// the chrome is an overlay, so nothing is subtracted.
    pub(super) fn strip_viewport(&self) -> f64 {
        f64::from(match axis_of(self.viewer.mode) {
            Axis::Vertical => self.viewer.container.height,
            Axis::Horizontal => self.viewer.container.width,
        })
    }

    /// The geometry the strip is measured against this frame.
    fn geometry(&self) -> Geometry {
        Geometry {
            mode: self.viewer.mode,
            pages: self.document.num_pages,
            scale: self.zoom.display,
            gap: match self.viewer.mode {
                // A horizontal strip's pages are inset by the reader's margin
                // rather than spaced apart: two margins are the spacing.
                ViewMode::ScrollHorizontal => 2.0 * self.viewer.margin,
                _ => self.viewer.gap,
            },
            inset: match self.viewer.mode {
                // The same margin at both ends of the strip, so the first and
                // last page read like the gaps between them; a column runs edge
                // to edge.
                ViewMode::ScrollHorizontal => self.viewer.margin,
                _ => 0.0,
            },
        }
    }

    /// A page's extent along the main axis at `scale`, in the box it is drawn
    /// in — the same snapped arithmetic the raster is asked at, so an offset
    /// lands on the edge the page paints.
    fn page_extent(&self, page: u32, axis: Axis, scale: f64) -> f64 {
        let (width, height) = self.box_of(page, scale);
        f64::from(match axis {
            Axis::Vertical => height,
            Axis::Horizontal => width,
        })
    }

    /// The windowing for a geometry: every page's own extent at its scale, plus
    /// the fixed chrome between them.
    fn windowing(&self, geometry: Geometry) -> Windowing {
        let axis = axis_of(geometry.mode);
        Windowing::new(
            (1..=geometry.pages).map(|page| self.page_extent(page, axis, geometry.scale)),
            geometry.gap,
        )
    }

    /// Bring the strip in step with the reader's geometry: mount it on the page
    /// the reader is on, hold their place across a rescale, or let it go when
    /// nothing scrolls here.
    pub(super) fn reflow(&mut self) -> Vec<Effect> {
        let viewport = self.strip_viewport();
        // A mount lands against the viewport, and a landing measured against a
        // window nobody has sized yet is a place the reader was never at: the
        // mount waits for the first `Resized`, which the app reports before
        // anything can be opened on it.
        if !self.viewer.measured() {
            return Vec::new();
        }
        if !self.scrolls() {
            // The surface and everything moving on it belonged to the mode that
            // just left: an aim with no widget under it, and a glide toward a
            // page the paginated view is already showing.
            self.strip = None;
            self.glide = None;
            self.anchor = None;
            return Vec::new();
        }
        let geometry = self.geometry();
        let mut effects = Vec::new();
        match self.strip.as_ref().map(Strip::geometry) {
            None => {
                let mut strip = Strip::new(geometry, self.windowing(geometry));
                // A fresh strip mounts on the page the reader is on — the resume
                // point of an open, or wherever a flip into the strip left them.
                let landing = Landing::of(geometry.mode);
                let offset = strip.offset_of(self.viewer.page, viewport, landing);
                strip.adopt(offset, viewport);
                self.strip = Some(strip);
                effects.extend(self.aim());
            }
            Some(held) if held != geometry => {
                let windowing = self.windowing(geometry);
                let strip = self.strip.as_mut().expect("a strip is above");
                strip.rescale(geometry, windowing, viewport);
                // A rescale is a follow rather than a gesture: it lands where it
                // was asked to, instead of easing over moving geometry.
                self.glide = None;
                effects.extend(self.aim());
            }
            Some(_) => {}
        }
        self.grow_window(viewport);
        self.pump_frames();
        effects
    }

    /// Resolve the mounted window for this frame.
    fn grow_window(&mut self, viewport: f64) {
        if let Some(strip) = self.strip.as_mut() {
            strip.rewindow(viewport);
        }
    }

    /// Command a position, and remember it as the reader's own so the window,
    /// the rasters and the surface all read one number.
    fn post(&mut self, offset: f64) -> Option<Effect> {
        let viewport = self.strip_viewport();
        let strip = self.strip.as_mut()?;
        let offset = strip.adopt(offset, viewport);
        let axis = strip.axis();
        self.grow_window(viewport);
        Some(Effect::Scroll { axis, offset })
    }

    /// Aim the strip at where it already sits — a mount, or a place held across
    /// a rescale.
    fn aim(&mut self) -> Option<Effect> {
        let offset = self.strip.as_ref()?.offset();
        self.anchor = Some(Anchor::aim(offset));
        self.anchor_effect()
    }

    /// The aim's next post, if it still has one.
    ///
    /// While the cover is up the aim posts nothing at all, and keeps its posts:
    /// the cover is exactly the stretch where the surface has not been built, a
    /// command posted to a widget that does not exist yet is nothing, and the
    /// posts are all the aim gets.
    pub(super) fn anchor_effect(&mut self) -> Option<Effect> {
        if self.cover_up() {
            return None;
        }
        let offset = self.anchor.as_mut()?.repost()?;
        self.post(offset)
    }

    /// Every frame of a gliding page turn: the strip is commanded to where the
    /// glide has reached, so the pages it passes are asked for as they come into
    /// view rather than all at once when it lands.
    pub(super) fn glide_effect(&mut self, delta_ms: f64) -> Vec<Effect> {
        let Some(mut glide) = self.glide else {
            return Vec::new();
        };
        let (offset, landed) = glide.step(delta_ms);
        self.glide = if landed { None } else { Some(glide) };
        let effects: Vec<Effect> = self.post(offset).into_iter().collect();
        self.pump_frames();
        effects
    }

    /// Move the reader to `page` in a scrolling mode.
    ///
    /// A short jump glides — a strip has no page edge to flash, so a turn has to
    /// show the reader that it travelled. A long jump lands at once, and even
    /// then it is aimed rather than shouted: one post can be clamped away by a
    /// surface that has just changed size.
    pub(super) fn jump_to(&mut self, page: u32) -> Vec<Effect> {
        // In a continuous mode the page and the strip's position are one fact,
        // so the jump moves both: a caller that only wanted the surface moved
        // would be asking for a scroll the reader's page disagrees with.
        self.viewer.page = page.clamp(1, self.document.num_pages.max(1));
        let viewport = self.strip_viewport();
        let landing = Landing::of(self.viewer.mode);
        let Some(strip) = self.strip.as_ref() else {
            return Vec::new();
        };
        let from = strip.offset();
        let to = strip.offset_of(page, viewport, landing);
        self.anchor = None;
        let mut effects = if self.motion.glides(to - from, viewport) {
            self.glide = Some(Glide::begin(from, to));
            // The first frame of the glide is where the strip already is: posting
            // it now means the surface has the position before the next step.
            self.glide_effect(0.0)
        } else {
            self.glide = None;
            self.aim_to(to)
        };
        // A jump is a page asked for rather than scrolled to, so the app is told
        // where the reader landed on the same frame the strip lands.
        effects.extend(self.report_progress());
        effects
    }

    /// Aim the strip at a position it is not at yet — the landing a long jump
    /// would otherwise miss.
    fn aim_to(&mut self, offset: f64) -> Vec<Effect> {
        self.anchor = Some(Anchor::aim(offset));
        self.post(offset).into_iter().collect()
    }

    /// The reader's own scroll: the surface moved under their hands, so whatever
    /// was gliding or waiting to land is theirs now.
    pub(super) fn scrolled(&mut self, along: f64) -> Vec<Effect> {
        if let Some(anchor) = self.anchor {
            // A report that disagrees while the aim has posts left is the
            // surface still holding the bounds it had a frame ago, not the
            // reader moving; past the posts it is the reader.
            if !anchor.landed(along) && !anchor.spent() {
                return Vec::new();
            }
            self.anchor = None;
        }
        // The surface repeating the position the reader just gave it — a glide's
        // own frame, a mount's post — is not news. It matters because the
        // alternative is cancelling the glide that is still running.
        if self.strip.as_ref().is_some_and(|strip| strip.agrees(along)) {
            return Vec::new();
        }
        self.glide = None;
        let viewport = self.strip_viewport();
        if let Some(strip) = self.strip.as_mut() {
            strip.adopt(along, viewport);
        }
        self.grow_window(viewport);
        self.pump_frames();
        self.page_from_scroll();
        Vec::new()
    }

    /// A near-window step along the strip: the page keys and Space page the
    /// column, with a sliver of overlap so the reader does not lose the line
    /// they just read.
    pub(super) fn scroll_by(&mut self, delta: f64) -> Vec<Effect> {
        let viewport = self.strip_viewport();
        let Some(strip) = self.strip.as_ref() else {
            return Vec::new();
        };
        let to = (strip.offset() + delta).clamp(0.0, strip.max_offset(viewport));
        self.glide = None;
        self.aim_to(to)
    }

    /// The page the strip's window puts under the reader's eyes.
    pub(super) fn dominant_page(&self) -> u32 {
        let Some(strip) = self.strip.as_ref() else {
            return self.viewer.page;
        };
        page_of_dominant(strip.dominant(self.strip_viewport()), self.document.num_pages)
    }

    /// A wheel notch, as the reader's own scroll.
    ///
    /// A wheel is a vertical gesture, and iced hands it to the axis it points
    /// at — so a horizontal strip hears nothing from it, exactly as the web
    /// app's own scroller did not. There the shell installed a listener that
    /// wrote `scrollLeft` from the wheel whenever the strip fitted vertically;
    /// here the app forwards the notches the surface did not take, and the
    /// translation is the reader's own, because the mode and the axis are.
    pub(super) fn wheel(&mut self, lines: f32) -> Vec<Effect> {
        if self.viewer.mode != ViewMode::ScrollHorizontal {
            return Vec::new();
        }
        self.scroll_by(f64::from(lines) * WHEEL_LINE_PX)
    }

    /// The page the reader's own scrolling came to rest on, if it is news.
    ///
    /// A paginated mode pans a page that overflows; it never turns one. And a
    /// zoom is rewriting the geometry under the window while a turn is still
    /// owed its jump: the dominant page right now is nobody's answer.
    fn page_from_scroll(&mut self) {
        if !self.scrolls() || self.zoom.ticking() || self.held.waiting() {
            return;
        }
        let page = self.dominant_page();
        if page == self.viewer.page {
            return;
        }
        self.viewer.page = page;
        self.pump_frames();
        // A scroll crosses a book in a second and every crossing is a page: the
        // write waits for the reader to stop (see `Reader::on_tick`).
        self.progress.moved();
    }
}
