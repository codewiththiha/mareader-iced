//! The rasters on screen: one per mounted page, the boxes they are drawn in, and
//! the cover over a fresh mount until the reader's own page has painted.

use std::time::Instant;

use iced::Size;
use iced::widget::image::Handle;

use crate::formats::pdf::{FrameKey, Request};

use super::{Command, Reader};

/// How many rasters may be in flight at once. Every answer allocates a page in
/// device pixels, and the engine serves them in the order they were asked for:
/// two lets the page under the reader's eyes be asked for behind the read-ahead
/// without a whole window's worth of full-page bitmaps in flight together.
const IN_FLIGHT: usize = 2;

/// How long a fresh mount may stay covered. The net under the paint-driven
/// lift: a render that never arrives must not leave the reader looking at paper
/// instead of the book they opened.
const COVER_GRACE_MS: u64 = 900;

/// One page raster: the texture a page host paints, and the key it answers.
#[derive(Debug, Clone)]
pub(super) struct Frame {
    pub(super) key: FrameKey,
    pub(super) handle: Handle,
}

/// The rasters the reader holds: one per page that is mounted, and the keys the
/// engine has been asked for and has not answered yet.
///
/// The mount window is the whole of the reader's raster memory: nothing is kept
/// for a page that is not on screen, so a fling through a book costs its windows
/// and not its pages.
#[derive(Debug, Default)]
pub(super) struct Frames {
    held: Vec<Frame>,
    asked: Vec<FrameKey>,
}

impl Frames {
    /// The raster for `page`, whatever box it was drawn at: a page keeps the one
    /// in hand — stretched to the size on screen — until its replacement lands,
    /// which is what keeps a page turn or a zoom from blinking.
    pub(super) fn get(&self, page: u32) -> Option<&Frame> {
        self.held.iter().find(|frame| frame.key.page == page)
    }

    /// Whether the raster in hand is exactly this key: the page, at the size the
    /// engine was last asked for it.
    fn current(&self, key: FrameKey) -> bool {
        self.get(key.page).is_some_and(|frame| frame.key == key)
    }

    /// Whether this key has been asked for and not answered.
    fn asked(&self, key: FrameKey) -> bool {
        self.asked.contains(&key)
    }

    fn ask(&mut self, key: FrameKey) {
        self.asked.push(key);
    }

    /// Take an answer. `false` for a key nobody is waiting for — a raster whose
    /// page, size or device grid has been superseded since it was asked for — so
    /// it is dropped where it lands rather than kept.
    pub(super) fn take(&mut self, key: FrameKey) -> bool {
        match self.asked.iter().position(|asked| *asked == key) {
            Some(at) => {
                self.asked.remove(at);
                true
            }
            None => false,
        }
    }

    /// Keep an answered raster under its page, replacing that page's old one.
    /// A raster the reader is waiting on takes the head of the store, so the
    /// one before it is not found first when both are for pages on screen.
    pub(super) fn hold(&mut self, frame: Frame) {
        match self
            .held
            .iter()
            .position(|held| held.key.page == frame.key.page)
        {
            Some(at) => {
                self.held[at] = frame;
            }
            None => self.held.push(frame),
        }
    }

    /// Let go of everything that is not on screen any more, and of every answer
    /// still owed for one.
    fn retain(&mut self, keep: &[u32]) {
        self.held.retain(|frame| keep.contains(&frame.key.page));
        self.asked.retain(|key| keep.contains(&key.page));
    }

    pub(super) fn clear(&mut self) {
        self.held.clear();
        self.asked.clear();
    }
}

impl Reader {
    /// The box a page occupies at `scale`, in CSS px, snapped to the device-
    /// pixel grid (`pdf_core::pixel_grid`): the paint box and the raster box are
    /// the same arithmetic, so a page's edge is a whole device pixel and a page
    /// turn cannot leave a hairline of background along the seam.
    pub(super) fn box_of(&self, page: u32, scale: f64) -> (f32, f32) {
        let (width, height) = self.viewer.page_px(self.document.page_box(page), scale);
        (
            pdf_core::pixel_grid::snap_px(f64::from(width)) as f32,
            pdf_core::pixel_grid::snap_px(f64::from(height)) as f32,
        )
    }

    /// The box the page under the reader's eyes occupies this frame: the settled
    /// box while nothing moves, and the box a transition is passing through while
    /// one does.
    pub(super) fn page_box_px(&self) -> (f32, f32) {
        self.box_of(self.viewer.page, self.zoom.display)
    }

    /// The key a page's raster is asked for and answered under.
    pub(super) fn frame_key(&self, page: u32) -> FrameKey {
        let (width, height) = self.box_of(page, self.zoom.committed);
        FrameKey::of_css(page, f64::from(width), f64::from(height), self.viewer.dpr)
    }

    /// The raster the reader holds for a page, if any.
    pub(super) fn frame_here(&self, page: u32) -> Option<&Frame> {
        self.frames.get(page)
    }

    /// The pages whose rasters belong on screen, the one under the reader's eyes
    /// first: the engine serves what they are looking at before the read-ahead
    /// around it.
    pub(super) fn frame_pages(&self) -> Vec<u32> {
        if !self.document.status.is_ready() {
            return Vec::new();
        }
        let last = self.document.num_pages.max(1);
        if self.viewer.mode == reader_core::view::ViewMode::Single {
            return vec![self.viewer.page.clamp(1, last)];
        }
        if self.viewer.mode.is_paginated() {
            // A spread shows the pair its start opens: the page the reader is on
            // and the one beside it, left first.
            let left = reader_core::view::spread_start(self.viewer.page).clamp(1, last);
            return (left..=last.min(left + 1)).collect();
        }
        let anchor = self.viewer.page.clamp(1, last);
        let mounted: Vec<u32> = match self.strip.as_ref().and_then(|strip| strip.mounted()) {
            Some(window) => window.iter().map(|index| index as u32 + 1).collect(),
            None => vec![anchor],
        };
        let mut pages: Vec<u32> = mounted.into_iter().filter(|page| *page <= last).collect();
        pages.dedup();
        pages.sort_by_key(|page| page.abs_diff(anchor));
        pages
    }

    /// Ask for the rasters of the pages on screen that are missing, and let go of
    /// the ones that are not.
    pub(super) fn pump_frames(&mut self) {
        if !self.document.status.is_ready() || !self.viewer.measured() {
            return;
        }
        let pages = self.frame_pages();
        self.frames.retain(&pages);
        for page in pages {
            let key = self.frame_key(page);
            if self.frames.current(key) || self.frames.asked(key) {
                continue;
            }
            if self.frames.asked.len() >= IN_FLIGHT {
                break;
            }
            self.frames.ask(key);
            self.engine.send(Request::Frame {
                stamp: self.session,
                key,
            });
        }
    }

    /// Whether the cover is still over the reading area: a fresh mount hides the
    /// strip while it races to the reader's own page, so what they see first is
    /// the page they left off at rather than the top of the book.
    pub(super) fn cover_up(&self) -> bool {
        self.cover.is_some()
    }

    /// The paint that lifts the cover: the raster of the page the reader is on
    /// has landed, so what is behind the cover is their page and not a blank.
    pub(super) fn painted(&mut self, page: u32) {
        if self.cover.is_some() && page == self.viewer.page {
            self.cover = None;
        }
    }

    /// A render that failed answers the page just as a paint does: the cover
    /// over a fresh mount must not wait on a raster that is never coming.
    pub(super) fn frame_failed(&mut self, page: u32) {
        self.painted(page);
    }

    /// The net: a render that never reports must not strand the cover.
    pub(super) fn cover_expired(&mut self, now: Instant) {
        if let Some(raised) = self.cover
            && now.saturating_duration_since(raised).as_millis() as u64 >= COVER_GRACE_MS
        {
            self.cover = None;
        }
    }

    /// The window changed size.
    ///
    /// The space around the page is a *follow*, not a refit: the layout moves
    /// with the window every time it is reported — a scale that waited for the
    /// drag to end would leave the page wider than its box, and the flex
    /// arithmetic that would have to squish it is exactly what the web app
    /// refused to do — while the crisp raster waits for the burst to go quiet,
    /// so a drag costs one render rather than one per frame.
    pub(super) fn resize(&mut self, size: Size) {
        self.viewer.container = size;
        if self.document.status.is_ready() {
            // The strip is reconciled with the new window by the update loop,
            // on this same frame.
            self.zoom_command(Command::Follow, false);
        }
    }
}

#[cfg(test)]
mod tests;
