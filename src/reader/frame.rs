//! The frame in flight: the raster on screen, the boxes the page is drawn in, and
//! the request that keeps them in step with the window.

use iced::Size;
use iced::widget::image::Handle;

use crate::formats::pdf::{FrameKey, Request};
use super::Reader;
use super::zoom::Command;

/// The page raster on screen: the texture the page host paints, and the key it
/// answers.
///
/// The key is kept so a frame that belongs to another page — or another device
/// grid, or another session — can be told apart from the one the page host is
/// waiting for; the raster's own pixel box is not, because the page is drawn at
/// the box its scale resolves to and the texture is fitted to it either way.
#[derive(Debug, Clone)]
pub(super) struct Frame {
    pub(super) key: FrameKey,
    pub(super) handle: Handle,
}

impl Reader {
    /// The window changed size.
    ///
    /// The space around the page is a *follow*, not a refit: the layout moves
    /// with the window every time it is reported — a scale that waits for the
    /// drag to end leaves the page wider than its box, and the flex arithmetic
    /// that would have to squish it is exactly what the web app refused to do —
    /// while the crisp raster waits for the burst to go quiet, so a drag costs
    /// one render rather than one per frame. A fit tracks the window; a
    /// hand-picked zoom keeps its own scale and overflows instead, which is the
    /// difference between the two, and the reason this resolves through the
    /// pipeline rather than deciding here.
    pub(super) fn resize(&mut self, size: Size) {
        self.viewer.container = size;
        if self.document.status.is_ready() {
            self.zoom_command(Command::Follow, false);
        }
    }

    /// Ask the engine for the page under the reader's eyes, in the box it
    /// occupies at the settled scale.
    pub(super) fn request_frame(&mut self) {
        if !self.document.status.is_ready() || !self.viewer.measured() {
            return;
        }
        let (width, height) = self.frame_box_px();
        let key = FrameKey::of_css(
            self.viewer.page,
            f64::from(width),
            f64::from(height),
            self.viewer.dpr,
        );
        self.awaiting = Some(key);
        self.engine.send(Request::Frame {
            stamp: self.session,
            key,
        });
    }

    /// The page raster for the page on screen, if the one in hand is that
    /// page's. A frame for the page the reader just left stays out of the way
    /// until its replacement lands.
    pub(super) fn frame_here(&self) -> Option<&Frame> {
        self.frame
            .as_ref()
            .filter(|frame| frame.key.page == self.viewer.page)
    }

    /// The box the page occupies on screen this frame, in CSS px: the settled
    /// box while nothing moves, and the box a transition is passing through
    /// while one does. The raster in hand is drawn up to it — stretched, the way
    /// the web app's page hosts stretched theirs — which is what makes a zoom
    /// read as the paper itself changing size rather than a jump at the end.
    pub(super) fn page_box_px(&self) -> (f32, f32) {
        self.box_at(self.zoom.display)
    }

    /// The box the raster is asked for, in CSS px at the committed scale — the
    /// only scale a frame is ever drawn at, so a zoom in flight never asks the
    /// engine for a size the reader is already past.
    pub(super) fn frame_box_px(&self) -> (f32, f32) {
        self.box_at(self.zoom.committed)
    }

    /// The page's box at `scale`, snapped to the device-pixel grid
    /// (`pdf_core::pixel_grid`): the paint box and the raster box are the same
    /// arithmetic, so the page's edge is a whole device pixel and a page turn
    /// cannot leave a hairline of background along the seam. A capped raster
    /// comes back smaller than this box and is drawn up to it.
    fn box_at(&self, scale: f64) -> (f32, f32) {
        let (width, height) = self
            .viewer
            .page_px(self.document.page_box(self.viewer.page), scale);
        (
            pdf_core::pixel_grid::snap_px(f64::from(width)) as f32,
            pdf_core::pixel_grid::snap_px(f64::from(height)) as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::formats::pdf::FrameKey;
    use crate::reader::frame::Frame;
    use crate::reader::kit::{reader, seeded};
    use iced::Size;
    use iced::widget::image::Handle;

    #[test]
    fn the_page_host_draws_the_frame_it_has_and_the_box_it_expects() {
        // The box on screen never depends on whether a frame has arrived: the
        // page must not change size the moment its raster lands.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let expected = reader.page_box_px();
        assert!(expected.0 > 0.0 && expected.1 > 0.0);
        let key = FrameKey::of_css(1, f64::from(expected.0), f64::from(expected.1), 1.0);
        reader.frame = Some(Frame {
            key,
            handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
        });
        assert!(reader.frame_here().is_some(), "the page's own raster is up");
        assert_eq!(reader.page_box_px(), expected, "the page does not move when its raster lands");
    }
}
