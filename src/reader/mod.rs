//! The reading surface: the document on screen and everything that moves it. The
//! reader is one struct the Elm loop updates; it talks to the engine thread
//! (requests out, events in) and to the app (effects out), and never writes to
//! the shelf — the app owns every byte that reaches it.
mod document;
mod page;
mod viewer;
mod zoom;
pub use document::{DocStatus, Document};
pub use viewer::Viewer;
pub use zoom::Command;

mod events;
mod frame;
mod moves;
mod opening;
mod update;

pub use opening::*;
pub use update::*;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::time::Instant;

use frame::Frame;
use reader_core::settings::Settings;
use reader_core::zoom_math::FitMode;
use zoom::Zoom;

use crate::formats::pdf::{Engine, FrameKey};

/// The reader's whole state.
pub struct Reader {
    pub document: Document,
    pub viewer: Viewer,
    /// The three scales and the transition in flight between them — the web
    /// app's zoom pipeline, which is the one owner of how big the page is.
    zoom: Zoom,
    /// The instant the last animation frame was handed out. A tween's progress
    /// is measured from it, so the reader's clock is the app's own frame rate
    /// rather than a timer of its own.
    last_tick: Instant,
    /// The page raster on screen, if any.
    frame: Option<Frame>,
    /// The engine worker, started by the first request it is given.
    engine: Engine,
    /// Whether the engine found a Pdfium library, once it has said. `None`
    /// before its first word — an open that arrives first still goes through,
    /// because the answer may be on its way.
    bound: Option<Result<(), String>>,
    /// The session stamp: which open every message belongs to. Bumped by every
    /// open and every close.
    session: u64,
    /// The raster the reader is waiting for, if any. A frame that answers any
    /// other key is not the one the page on screen is waiting on.
    awaiting: Option<FrameKey>,
}

/// Where the reader is, as the library wants to hear it: which row the open
/// belongs to, what the document calls itself, and how far in the reader got.
///
/// The web app's reading-progress effect watched the page signal and wrote the
/// library as the reader moved; natively the same fact is handed out on
/// request, and the app — which owns the blob, the disk and the stamp — decides
/// what to make of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub path: PathBuf,
    /// The library row this open belongs to, when the reader opened one.
    pub book_id: Option<String>,
    /// The document's own title, when it is one worth persisting: the metadata
    /// half of `reader_core::filename::display_name`, never the address's
    /// stem. A shelf row that filled its name from whatever address the open
    /// ran on would write the store's own `source.pdf` over a stored book.
    pub title: Option<String>,
    pub author: Option<String>,
    pub page: u32,
    pub num_pages: u32,
}

impl Reader {
    /// The reader, with its engine ready but not yet started: no Pdfium is
    /// loaded until the first document is asked for, so a run that never opens
    /// a PDF never touches the engine at all.
    pub fn new(settings: &Settings) -> Self {
        let layout = &settings.layout;
        let mut viewer = Viewer {
            fit: layout.default_fit,
            margin: layout.page_margin,
            auto_resize: layout.auto_resize,
            ..Viewer::default()
        };
        // `None` is not a startup fit: `sanitize` has already replaced a
        // persisted `None` with the default, and a fit of `None` would leave
        // the scale at 1 with nothing owning it.
        if viewer.fit == FitMode::None {
            viewer.fit = FitMode::Width;
        }
        Self {
            document: Document::default(),
            viewer,
            zoom: Zoom::default(),
            last_tick: Instant::now(),
            frame: None,
            engine: Engine::new(),
            bound: None,
            session: 0,
            awaiting: None,
        }
    }

    /// The name the title bar shows for what is open. A borrow rather than a
    /// fresh string: the bar asks for it while it builds its element, and the
    /// document keeps it resolved (see `Document::refresh_name`).
    pub fn name(&self) -> &str {
        self.document.name()
    }

    /// Where the reader is, or `None` while nothing is open — or while the
    /// open never finished, because the library already holds the resume point
    /// it handed in and writing now would move the book to page 1.
    pub fn read(&self) -> Option<Read> {
        let path = self.document.path.clone()?;
        if !self.document.status.is_ready() && self.document.num_pages == 0 {
            return None;
        }
        Some(Read {
            path,
            book_id: self.document.book_id.clone(),
            // The metadata half only: a title the document actually supplied
            // is worth persisting, and the stem of whatever address the open
            // ran on is the fallback's business at the moment it is shown.
            title: reader_core::filename::document_title(self.document.title.as_deref()),
            author: self.document.author.clone(),
            // Clamped by the same rule an open's resume point goes through: a
            // page of 0 or one past the end is a transient that escaped the
            // syncs, and writing it would be the next open's starting point.
            page: self
                .viewer
                .page
                .clamp(1, self.document.num_pages.max(1)),
            num_pages: self.document.num_pages,
        })
    }

    /// The read, as the effect the caller asked for.
    pub(super) fn effect_of(&self, kind: fn(Read) -> Effect) -> Vec<Effect> {
        self.read().map(kind).into_iter().collect()
    }
}
