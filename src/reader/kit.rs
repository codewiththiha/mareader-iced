//! Fixtures the cases across this module share.

use std::path::PathBuf;
use std::time::Duration;
use crate::formats::pdf;
use crate::formats::pdf::Engine;
use crate::reader::Reader;
use crate::reader::opening::Open;
use crate::reader::update::Message;
use reader_core::settings::Settings;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

/// One animation frame, `delta_ms` after the last: exactly what the app's
/// tick carries while the pipeline owes one.
pub(super) fn tick(reader: &mut Reader, delta_ms: f64) {
    let now = reader.last_tick + Duration::from_secs_f64(delta_ms / 1000.0);
    reader.update(Message::Tick, now);
}

/// A reader whose engine can never start a worker (see
/// [`Engine::inert`]): these tests are about the surface's own arithmetic,
/// and none of them touches Pdfium.
pub(super) fn reader_of(settings: &Settings) -> Reader {
    let mut reader = Reader::new(settings);
    reader.engine = Engine::inert();
    reader
}

/// A reader on one page of A4, in the mode most of these cases are about: the
/// paginated one. The continuous column — which is what a reader actually opens
/// into — has its own fixtures below.
pub(super) fn reader() -> Reader {
    let mut settings = Settings::default();
    settings.layout.default_fit = FitMode::Width;
    settings.layout.page_margin = 0.0;
    let mut reader = reader_of(&settings);
    reader.viewer.mode = ViewMode::Single;
    reader
}

pub(super) fn page(w: f64, h: f64) -> pdf::PageBox {
    pdf::PageBox {
        width: w,
        height: h,
    }
}

pub(super) fn open() -> Open {
    Open {
        path: PathBuf::from("/books/one.pdf"),
        book_id: None,
        row_title: None,
        resume: 1,
    }
}

/// A reader on a book of `pages` A4-portrait pages, in the mode given, with a
/// measured window: the fixture every surface case starts from.
///
/// The window is exactly one A4 page wide and the scale is pinned at 1:1 as the
/// reader's own: a page's box IS the page's own numbers, so a case can name an
/// offset as the arithmetic of page extents and gaps — and a mode flip, which
/// re-resolves the width fit, resolves it to the same 1:1.
pub(super) fn pages_of(pages: u32, resume: u32, mode: ViewMode) -> Reader {
    let mut reader = reader();
    reader.viewer.container = iced::Size::new(612.0, 1000.0);
    reader.viewer.mode = mode;
    reader.viewer.fit = FitMode::None;
    seeded(&mut reader, pages, resume);
    reader.zoom.initialize(1.0);
    reader
}

/// The strip, brought in step with a reader a case has just set up: what the
/// update loop does at the end of every message.
pub(super) fn strip(reader: &mut Reader) -> Vec<crate::reader::Effect> {
    reader.reflow()
}

/// A document seeded by hand: the engine is not asked for anything, so
/// these tests are about the reader's own arithmetic and never touch
/// Pdfium.
pub(super) fn seeded(reader: &mut Reader, pages: u32, resume: u32) {
    reader.session += 1;
    let mut opened = open();
    opened.resume = resume;
    reader.document.begin(&opened);
    reader.seed(pdf::Opened {
        path: PathBuf::from("/books/one.pdf"),
        num_pages: pages,
        page_sizes: vec![
            pdf::PageBox {
                width: 612.0,
                height: 792.0,
            };
            pages as usize
        ],
        title: Some("One".to_string()),
        author: None,
    });
    // The fixture's document has painted: the cover over a fresh mount has its
    // own cases, and every other case here wants the book itself.
    reader.cover = None;
}
