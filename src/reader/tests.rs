//! The module's tests: the subjects beside this hold the code, and this
//! holds the cases they have to satisfy.

use super::*;

use std::path::PathBuf;

use std::time::Instant;

use iced::Size;

use iced::widget::image::Handle;

use reader_core::zoom_math::FitMode;

use crate::formats::pdf::{self, Engine, Event, FrameKey};

use super::zoom::Command;

use reader_core::settings::Settings;

use std::time::Duration;

/// One animation frame, `delta_ms` after the last: exactly what the app's
/// tick carries while the pipeline owes one.
fn tick(reader: &mut Reader, delta_ms: f64) {
    let now = reader.last_tick + Duration::from_secs_f64(delta_ms / 1000.0);
    reader.update(Message::Tick, now);
}

/// A reader whose engine can never start a worker (see
/// [`Engine::inert`]): these tests are about the surface's own arithmetic,
/// and none of them touches Pdfium.
fn reader_of(settings: &Settings) -> Reader {
    let mut reader = Reader::new(settings);
    reader.engine = Engine::inert();
    reader
}

fn reader() -> Reader {
    let mut settings = Settings::default();
    settings.layout.default_fit = FitMode::Width;
    settings.layout.page_margin = 0.0;
    reader_of(&settings)
}

fn open() -> Open {
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
fn seeded(reader: &mut Reader, pages: u32, resume: u32) {
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

#[test]
fn an_open_that_never_finished_writes_nothing_to_the_shelf() {
    // A failed open has no position worth writing: the library already
    // holds the resume point it handed in, and a write here would move the
    // book to page 1.
    let mut reader = reader();
    let effects = reader.begin_open(open());
    assert!(effects.is_empty(), "a document that is still opening is not an error");
    assert!(reader.close().is_empty());
}

#[test]
fn the_resume_page_is_clamped_to_the_book_that_opened() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 40);
    assert_eq!(reader.viewer.page, 3, "a book with three pages resumes at 3");
    let effects = reader.close();
    match effects.as_slice() {
        [Effect::Record(read)] => {
            assert_eq!((read.page, read.num_pages), (3, 3));
            assert_eq!(read.path, PathBuf::from("/books/one.pdf"));
            assert_eq!(read.title.as_deref(), Some("One"));
        }
        other => panic!("expected one record, got {other:?}"),
    }
}

#[test]
fn a_turn_reports_the_new_position_and_a_dead_press_reports_nothing() {
    // The web app's progress effect wrote the library as the reader moved;
    // natively the move is the reader's report and the write is the app's.
    // A press with nowhere to go reports nothing, so a shelf the reader
    // never moved on does not reorder itself.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    match reader.turn(1).as_slice() {
        [Effect::Progress(read)] => assert_eq!(read.page, 2),
        other => panic!("expected one progress report, got {other:?}"),
    }
    // At the end of the book: no move, no write.
    reader.turn(1);
    assert!(reader.turn(1).is_empty());
    assert_eq!(reader.viewer.page, 3);
}

#[test]
fn a_read_names_the_row_and_only_a_title_worth_persisting() {
    // The row is what keeps a book of the reader's own from handing its
    // position to its twin at the address; the title is the metadata half
    // only, because the address's stem is a display fallback and writing
    // it down would rename a stored book after the store's own layout.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.session += 1;
    reader.document.begin(&Open {
        path: PathBuf::from("/store/items/b1/source.pdf"),
        book_id: Some("b1".to_string()),
        row_title: Some("Dune".to_string()),
        resume: 1,
    });
    reader.seed(pdf::Opened {
        path: PathBuf::from("/store/items/b1/source.pdf"),
        num_pages: 2,
        page_sizes: vec![
            pdf::PageBox {
                width: 612.0,
                height: 792.0,
            };
            2
        ],
        title: Some("Untitled".to_string()),
        author: Some("Frank Herbert".to_string()),
    });
    let read = reader.read().expect("a book is open");
    assert_eq!(read.book_id.as_deref(), Some("b1"));
    assert_eq!(read.title, None, "a placeholder title is not worth writing down");
    assert_eq!(read.author.as_deref(), Some("Frank Herbert"));
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
    reader.begin_open(open());
    reader.turn(1);
    assert_eq!(reader.viewer.page, 1);
}

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

#[test]
fn a_frame_for_another_page_is_not_painted() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
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
    assert!(reader.frame_here().is_none(), "the page on screen is still blank paper");
    assert!(reader.frame.is_none(), "and the raster is not kept either");
}

#[test]
fn an_answer_from_a_session_the_reader_left_is_ignored() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
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
    assert!(reader.frame.is_none());
}

#[test]
fn a_turn_onto_a_different_sheet_refits_when_the_setting_says_so() {
    // A landscape plate inside a portrait book: with `auto_resize` on, the
    // fit follows the sheet under the reader's eyes.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.session += 1;
    reader.document.begin(&open());
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
    reader.document.begin(&open());
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
    let scale = reader.zoom.committed;
    reader.turn(1);
    assert!(
        (reader.zoom.committed - scale).abs() < 1e-9,
        "the reader's own scale is not moved behind their back"
    );
}

#[test]
fn a_step_moves_the_page_at_once_and_sharpens_it_when_it_lands() {
    // The gesture's contract, and the whole reason three scales are kept:
    // the paper grows under the reader's eyes while the raster on it is the
    // one that was already crisp, and the engine is asked exactly once — at
    // the end, for the size the page actually came to rest at.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    let before = reader.zoom.committed;
    reader.update(Message::Zoom(Command::Step(1)), Instant::now());
    assert!(reader.needs_tick(), "a gesture owes frames");
    assert!(
        (reader.zoom.display - before).abs() < 1e-9,
        "the tween opens where the eye already is"
    );
    // The tween's first frame: the paper grows, and the raster on it is
    // still the one drawn for the size the engine was last asked at.
    tick(&mut reader, 60.0);
    assert!(reader.zoom.display > before, "the page grew");
    assert!(
        (reader.zoom.committed - before).abs() < 1e-9,
        "and the raster is drawn for the size it was asked at"
    );
    assert!(
        reader.page_box_px().0 > reader.frame_box_px().0,
        "the paper on screen is the bigger of the two"
    );
    // The rung above a 800/612 fit width.
    tick(&mut reader, 200.0);
    assert!((reader.zoom.committed - 1.5).abs() < 1e-9);
    assert!((reader.zoom.display - 1.5).abs() < 1e-9);
    assert!(!reader.needs_tick());
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
fn a_hand_picked_zoom_is_not_shrunk_back_to_the_fit_ceiling() {
    // The ceiling a manual zoom resolves to is what the reader chose, not
    // fit width: a page zoomed in on keeps its scale and overflows with a
    // scroll affordance (3c's), rather than snapping back to fit.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    reader.update(Message::Zoom(Command::Step(1)), Instant::now());
    tick(&mut reader, 200.0);
    assert!((reader.zoom.desired - 1.5).abs() < 1e-9);
    assert_eq!(reader.viewer.fit, FitMode::None, "the gesture dropped the fit");
    // A window the fit would now answer differently: the reader's own zoom
    // is what the page keeps.
    reader.resize(Size::new(1400.0, 1000.0));
    assert!((reader.zoom.display - 1.5).abs() < 1e-9);
    assert!((reader.zoom.committed - 1.5).abs() < 1e-9, "nothing moved, so nothing renders");
}

#[test]
fn choosing_a_fit_answers_in_the_frame_it_lands() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    reader.update(Message::Zoom(Command::Step(1)), Instant::now());
    tick(&mut reader, 200.0);
    assert_eq!(reader.viewer.fit, FitMode::None);
    // A click, not a gesture: the page is in its new size on the frame the
    // answer landed (the raster sharpens on the held commit behind it).
    reader.update(Message::Fit(FitMode::Page), Instant::now());
    assert_eq!(reader.viewer.fit, FitMode::Page);
    assert!(
        (reader.zoom.display - 1000.0 / 792.0).abs() < 1e-9,
        "the whole sheet shows: the height is the binding side"
    );
    tick(&mut reader, 200.0);
    assert!((reader.zoom.committed - 1000.0 / 792.0).abs() < 1e-9);
    assert!((reader.zoom.desired - 1000.0 / 792.0).abs() < 1e-9, "a fit owns the ceiling too");
}

#[test]
fn closing_keeps_the_reader_s_own_zoom_and_drops_what_was_moving() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    seeded(&mut reader, 3, 1);
    reader.update(Message::Zoom(Command::Step(1)), Instant::now());
    tick(&mut reader, 200.0);
    reader.turn(1);
    let scale = reader.zoom.committed;
    // A gesture still in flight when the reader leaves: dropped, or it
    // would keep asking the engine for frames of a page nobody is reading.
    reader.update(Message::Zoom(Command::Step(-1)), Instant::now());
    assert!(reader.needs_tick());
    reader.close();
    assert!(!reader.needs_tick());
    assert_eq!(reader.viewer.page, 1);
    assert!(
        (reader.zoom.committed - scale).abs() < 1e-9,
        "the reader's own zoom survives the book"
    );
}

#[test]
fn the_blank_state_before_an_open_has_one_open_page() {
    // Nothing open: the viewer still answers with a page 1 box, because the
    // surface is built before the first answer arrives.
    let reader = reader();
    assert_eq!(reader.viewer.page, 1);
    assert!(reader.frame_here().is_none());
    let (w, h) = reader.page_box_px();
    assert!(w > 0.0 && h > 0.0);
    // Nothing is open, so nothing names the reader's route: the bar falls
    // back to the app's own title.
    assert!(!reader.document.is_open());
    assert_eq!(reader.name(), "");
}
