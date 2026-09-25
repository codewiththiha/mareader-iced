//! Fixtures the cases across this module share.

use std::path::PathBuf;
use std::time::Duration;
use crate::formats::pdf;
use crate::formats::pdf::Engine;
use crate::reader::Reader;
use crate::reader::opening::Open;
use crate::reader::update::Message;
use reader_core::settings::Settings;
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

pub(super) fn reader() -> Reader {
    let mut settings = Settings::default();
    settings.layout.default_fit = FitMode::Width;
    settings.layout.page_margin = 0.0;
    reader_of(&settings)
}

pub(super) fn open() -> Open {
    Open {
        path: PathBuf::from("/books/one.pdf"),
        book_id: None,
        row_title: None,
        resume: 1,
    }
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
}
