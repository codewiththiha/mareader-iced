//! The open pipeline's own cases.

use super::*;

use std::path::PathBuf;
use crate::formats::pdf;
use crate::formats::pdf::{Event, FrameKey};
use crate::reader::kit::{reader, reader_of, seeded, tick};
use crate::reader::kit::open as kit_open;
use iced::Size;
use reader_core::settings::Settings;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
fn open() -> Open {
    Open {
        path: PathBuf::from("/store/items/b1/source.pdf"),
        book_id: Some("b1".to_string()),
        row_title: Some("Dune".to_string()),
        resume: 42,
    }
}

/// The name as a surface reads it: refresh, then read — the two writes the
/// reader makes whenever the identity moves.
fn shown(document: &mut Document) -> String {
    document.refresh_name();
    document.name().to_string()
}

#[test]
fn a_row_name_answers_when_the_document_has_none_of_its_own() {
    // A stored copy's address is the store's own `source.pdf`; the row's
    // name is what keeps every copied book from reading "source".
    let mut document = Document::default();
    document.begin(&open());
    assert_eq!(document.name(), "Dune", "the open names the book at once");
    document.row_title = None;
    assert_eq!(shown(&mut document), "source");
}

#[test]
fn the_documents_own_title_outranks_the_rows_name() {
    // The web app's order, and the one the reader's title bar and its
    // floating label both read: the document's `/Title` when it is one
    // worth showing, and the library's name for the row only when it is
    // not.
    let mut document = Document::default();
    document.begin(&open());
    document.title = Some("Dune (deluxe edition)".to_string());
    assert_eq!(shown(&mut document), "Dune (deluxe edition)");
    // A title the filter refuses hands the name back to the row.
    document.title = Some("0321894073.pdf".to_string());
    assert_eq!(shown(&mut document), "Dune");
}

#[test]
fn a_document_that_was_never_measured_still_has_a_box() {
    // The reader lays out against this before the engine has answered, so
    // it must never be a zero — and A4 is what a document with nothing to
    // say is measured as.
    let document = Document::default();
    let box_ = document.page_box(3);
    assert!(measured(box_));
    assert!((box_.width - 595.276).abs() < 0.01);
}

#[test]
fn a_blank_row_name_does_not_win() {
    let mut document = Document::default();
    let mut named = open();
    named.row_title = Some("   ".to_string());
    document.begin(&named);
    assert_eq!(shown(&mut document), "source");
}

#[test]
fn a_fresh_document_is_idle_and_holds_nothing() {
    let mut document = Document::default();
    document.begin(&open());
    assert_eq!(document.status, DocStatus::Opening);
    document.reset();
    assert_eq!(document.status, DocStatus::Idle);
    assert!(document.path.is_none());
    assert_eq!(document.num_pages, 0);
}

#[test]
fn an_open_that_never_finished_writes_nothing_to_the_shelf() {
    // A failed kit_open has no position worth writing: the library already
    // holds the resume point it handed in, and a write here would move the
    // book to page 1.
    let mut reader = reader();
    let effects = reader.begin_open(kit_open());
    assert!(effects.is_empty(), "a document that is still opening is not an error");
    assert!(reader.close().is_empty());
}

#[test]
fn the_seed_scale_is_the_fit_the_resize_will_resolve() {
    // The first frame must already sit where the first refit lands: the
    // same geometry answers both, so a page never jumps a moment after it
    // appears.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    let seeded_scale = reader.zoom.committed;
    assert!(
        (reader.zoom.display - seeded_scale).abs() < 1e-9,
        "the three scales agree the moment a book opens"
    );
    reader.resize(Size::new(800.0, 1000.0));
    assert!((reader.zoom.committed - seeded_scale).abs() < 1e-9);
    assert!((seeded_scale - 800.0 / 612.0).abs() < 1e-9);
}

#[test]
fn a_window_with_no_size_yet_does_not_fit_the_page() {
    // Before the first `Resized`, the container is unmeasured: the page
    // keeps the scale it has rather than being slammed to the minimum.
    let mut reader = reader();
    seeded(&mut reader, 3, 1);
    assert!((reader.zoom.committed - 1.0).abs() < 1e-9);
}

#[test]
fn a_resize_follows_the_window_and_sharpens_once_it_stops() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    reader.resize(Size::new(400.0, 1000.0));
    assert!(
        (reader.zoom.display - 400.0 / 612.0).abs() < 1e-9,
        "the page is in the new window on the frame it was reported"
    );
    assert!(
        (reader.zoom.committed - 800.0 / 612.0).abs() < 1e-9,
        "while the raster waits for the burst to end"
    );
    tick(&mut reader, 200.0);
    assert!((reader.zoom.committed - 400.0 / 612.0).abs() < 1e-9);
}

#[test]
fn a_turn_clamps_at_both_ends_of_the_book() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    reader.turn(-1);
    assert_eq!(reader.viewer.page, 1, "there is no page 0");
    reader.turn(1);
    reader.turn(1);
    reader.turn(1);
    assert_eq!(reader.viewer.page, 3, "and no page 4");
    reader.turn(0);
    assert_eq!(reader.viewer.page, 3);
}

#[test]
fn a_turn_before_the_book_is_ready_does_nothing() {
    let mut reader = reader();
    reader.begin_open(kit_open());
    reader.turn(1);
    assert_eq!(reader.viewer.page, 1);
}

#[test]
fn a_raster_nobody_asked_for_is_dropped_where_it_lands() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 3, 1);
    let (w, h) = reader.page_box_px();
    let stale = FrameKey::of_css(2, f64::from(w), f64::from(h), 1.0);
    let effects = reader.on_event(Event::Frame {
        stamp: reader.session,
        key: stale,
        width: stale.width,
        height: stale.height,
        pixels: vec![0, 0, 0, 0],
    });
    assert!(effects.is_empty());
    assert!(reader.frame_here(2).is_none(), "the page is still blank paper");
    assert!(reader.frame_here(1).is_none(), "and the raster is not kept either");
}

#[test]
fn an_answer_from_a_session_the_reader_left_is_ignored() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 3, 1);
    let stale = reader.session + 7;
    let key = FrameKey::of_css(1, 800.0, 1035.0, 1.0);
    let effects = reader.on_event(Event::Frame {
        stamp: stale,
        key,
        width: key.width,
        height: key.height,
        pixels: vec![0, 0, 0, 0],
    });
    assert!(effects.is_empty());
    assert!(reader.frame_here(1).is_none());
}

#[test]
fn a_turn_onto_a_different_sheet_refits_when_the_setting_says_so() {
    // A landscape plate inside a portrait book: with `auto_resize` on, the
    // fit follows the sheet under the reader's eyes.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.session += 1;
    reader.document.begin(&kit_open());
    reader.seed(pdf::Opened {
        path: PathBuf::from("/books/one.pdf"),
        num_pages: 2,
        page_sizes: vec![
            pdf::PageBox {
                width: 612.0,
                height: 792.0,
            },
            pdf::PageBox {
                width: 1224.0,
                height: 792.0,
            },
        ],
        title: None,
        author: None,
    });
    // The cover over a fresh mount has its own cases; here the book is the
    // subject.
    reader.cover = None;
    assert!((reader.zoom.committed - 800.0 / 612.0).abs() < 1e-9);
    reader.turn(1);
    assert!(
        (reader.zoom.committed - 800.0 / 1224.0).abs() < 1e-9,
        "the landscape plate is fitted on its own terms"
    );
    assert!(!reader.needs_tick(), "and it lands in the frame the turn did");
}

#[test]
fn a_turn_keeps_the_scale_when_the_reader_asked_it_to() {
    // The other half of `auto_resize`: off, a wide plate overflows and
    // scrolls (3b's affordance) rather than shrinking the text.
    let mut settings = Settings::default();
    settings.layout.default_fit = FitMode::Width;
    settings.layout.auto_resize = false;
    let mut reader = reader_of(&settings);
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.session += 1;
    reader.document.begin(&kit_open());
    reader.seed(pdf::Opened {
        path: PathBuf::from("/books/one.pdf"),
        num_pages: 2,
        page_sizes: vec![
            pdf::PageBox {
                width: 612.0,
                height: 792.0,
            },
            pdf::PageBox {
                width: 1224.0,
                height: 792.0,
            },
        ],
        title: None,
        author: None,
    });
    reader.cover = None;
    let scale = reader.zoom.committed;
    reader.turn(1);
    assert!(
        (reader.zoom.committed - scale).abs() < 1e-9,
        "the reader's own scale is not moved behind their back"
    );
}

#[test]
fn a_window_drag_moves_the_page_every_frame_and_rasterises_once_at_the_end() {
    // The follow's contract: the layout is in the new window on the frame
    // it was reported — a scale that waited for the drag to end would leave
    // the page wider than its box — while the RENDER waits, so a drag costs
    // one raster pass rather than one per frame.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    let before = reader.zoom.committed;
    reader.resize(Size::new(600.0, 1000.0));
    assert!((reader.zoom.display - 600.0 / 612.0).abs() < 1e-9);
    reader.resize(Size::new(400.0, 1000.0));
    assert!((reader.zoom.display - 400.0 / 612.0).abs() < 1e-9);
    assert!((reader.zoom.committed - before).abs() < 1e-9, "still no raster");
    // Each frame of the burst pushes the deadline back…
    tick(&mut reader, 100.0);
    assert!((reader.zoom.committed - before).abs() < 1e-9);
    assert!(reader.needs_tick(), "the burst is not over");
    // …and the quiet at the end commits once, at the size it stopped at.
    tick(&mut reader, 200.0);
    assert!((reader.zoom.committed - 400.0 / 612.0).abs() < 1e-9);
    assert!(!reader.needs_tick());
}

#[test]
fn the_blank_state_before_an_open_has_one_open_page() {
    // Nothing kit_open: the viewer still answers with a page 1 box, because the
    // surface is built before the first answer arrives.
    let reader = reader();
    assert_eq!(reader.viewer.page, 1);
    assert!(reader.frame_here(1).is_none());
    let (w, h) = reader.page_box_px();
    assert!(w > 0.0 && h > 0.0);
    // Nothing is kit_open, so nothing names the reader's route: the bar falls
    // back to the app's own title.
    assert!(!reader.document.is_open());
    assert_eq!(reader.name(), "");
}
