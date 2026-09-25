//! What the engine thread reports — a document opened, a raster ready, a session
//! that ended — and the seeding an open answers with.

use std::time::Instant;

use iced::widget::image::Handle;

use crate::formats::pdf::{self, Event, Request};
use crate::ui::toast::Tone;
use super::document::DocStatus;
use super::frame::Frame;
use super::{Effect, Reader};

impl Reader {
    /// An answer from the engine.
    pub(super) fn on_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Bound { result } => {
                let failed = result.clone().err();
                self.bound = Some(result);
                match (failed, self.document.status) {
                    // The engine has no library and a document is waiting on
                    // it: that wait is over, and the sentence is the reason.
                    (Some(message), DocStatus::Opening) => {
                        self.document.status = DocStatus::Error;
                        self.document.error = Some(message.clone());
                        vec![Effect::Toast(Tone::Error, message)]
                    }
                    _ => Vec::new(),
                }
            }
            Event::Opened { stamp, opened } => {
                if stamp != self.session {
                    // A document the reader has already left. Its geometry
                    // would be written over the book they are reading now.
                    return Vec::new();
                }
                self.seed(opened);
                // The chapter tree is asked for only now: page 1 can be painted
                // without it, and resolving a textbook's destinations is not
                // free.
                self.engine.send(Request::Outline {
                    stamp: self.session,
                });
                self.pump_frames();
                // The open writes a read point at once, so a reader who quits
                // with the book still open does not lose their place — and so
                // a file the library has never seen joins it as a linked row
                // wearing the name the document gave.
                self.effect_of(Effect::Record)
            }
            Event::OpenFailed { stamp, message } => {
                if stamp != self.session {
                    return Vec::new();
                }
                self.document.status = DocStatus::Error;
                self.document.error = Some(message.clone());
                // Nothing is coming to paint over the cover.
                self.cover = None;
                vec![Effect::Toast(Tone::Error, message)]
            }
            Event::Frame {
                stamp,
                key,
                width,
                height,
                pixels,
            } => {
                if stamp != self.session || !self.frames.take(key) {
                    // Another book's answer, or a raster whose page, size or
                    // device grid has been superseded since it was asked for:
                    // it is dropped where it lands rather than drawn over the
                    // page on screen.
                    return Vec::new();
                }
                self.frames.hold(Frame {
                    key,
                    handle: Handle::from_rgba(width, height, pixels),
                });
                // The reader's own page has painted, so the cover has nothing
                // left to hide.
                self.painted(key.page);
                Vec::new()
            }
            Event::FrameFailed {
                stamp, key, message, ..
            } => {
                if stamp != self.session {
                    return Vec::new();
                }
                self.frames.take(key);
                // A page that will never paint must not leave the cover on it.
                self.frame_failed(key.page);
                vec![Effect::Toast(Tone::Error, message)]
            }
            Event::PageText { .. } | Event::TextFailed { .. } => {
                // Text answers belong to the search index, which is built on
                // the first search (3e). Nothing asks for one yet, so nothing
                // can arrive here — the arm exists because the protocol is one
                // enum the engine owns, not a per-increment view of it.
                Vec::new()
            }
            Event::Outline { stamp, entries } => {
                if stamp != self.session {
                    return Vec::new();
                }
                self.document.outline =
                    pdf_core::outline::to_nodes(entries, self.document.num_pages);
                Vec::new()
            }
        }
    }

    /// The document is open: seed everything the fresh mount reads, in the
    /// order that makes each step safe — the web app's `seed`, with the search
    /// and gloss state it also reset belonging to their own increments.
    pub(super) fn seed(&mut self, opened: pdf::Opened) {
        let page1 = opened.first_page();
        self.document.path = Some(opened.path);
        self.document.num_pages = opened.num_pages;
        self.document.sizes = opened.page_sizes;
        self.document.title = opened.title;
        self.document.author = opened.author;
        self.document.outline.clear();
        self.document.error = None;
        // The document's own title has arrived, and with it the name the bar
        // shows: the metadata now outranks the row's name for the rest of the
        // read.
        self.document.refresh_name();
        // The resume point is clamped to the book that actually opened: a
        // re-edited document may have fewer pages than the shelf remembers, and
        // a saved 0 must never resume before the book.
        self.viewer.page = self.document.resume.clamp(1, self.document.num_pages.max(1));
        // The seed scale is the same fit the first live refit will resolve, so
        // the first frame already sits where the fit is going to land instead
        // of jumping to it a moment later — and every scale is seeded at once,
        // so nothing is left holding the previous book's number.
        let seeded = self.viewer.resolved_scale(page1, self.zoom.committed);
        self.zoom.initialize(seeded);
        // Nothing of the last document survives: not its rasters, not the strip
        // built for its pages, not the motion that was running on them.
        self.frames.clear();
        self.strip = None;
        self.glide = None;
        self.anchor = None;
        self.held = super::sync::HeldJump::default();
        // The mount is covered until the page the reader left off at has
        // painted: an open races to the resume point, and the race is not
        // something to watch.
        self.cover = Some(Instant::now());
        self.progress.seed(self.viewer.page);
        // `Ready` LAST: it is what makes the surface ask for a frame, and
        // everything a frame is drawn against is already in place.
        self.document.status = DocStatus::Ready;
    }
}
