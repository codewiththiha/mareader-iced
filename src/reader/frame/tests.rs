//! The raster store's own cases: what is kept, what is let go, and what the pump
//! asks for.

use iced::Size;
use iced::widget::image::Handle;
use std::time::{Duration, Instant};

use super::*;
use crate::formats::pdf::Event;
use crate::reader::kit::{open as kit_open, pages_of, reader, seeded};
use reader_core::view::ViewMode;

/// A raster of a page, at whatever box it claims.
fn raster(page: u32, width: u32, height: u32) -> Frame {
    Frame {
        key: FrameKey {
            page,
            width,
            height,
        },
        handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
    }
}

#[test]
fn a_page_keeps_the_raster_it_has_until_the_page_changes() {
    // The box is not part of the question a page host asks: a raster drawn for
    // another size is stretched to the box on screen until its replacement
    // lands, which is what keeps a page turn or a zoom from blinking.
    let mut frames = Frames::default();
    frames.hold(raster(3, 100, 200));
    let held = frames.get(3).expect("page three has its raster");
    assert_eq!(held.key.width, 100);
    assert!(frames.get(4).is_none(), "and page four has none");
}

#[test]
fn a_superseded_answer_is_dropped_where_it_lands() {
    let mut frames = Frames::default();
    let wanted = FrameKey {
        page: 3,
        width: 100,
        height: 200,
    };
    let stale = FrameKey {
        page: 3,
        width: 50,
        height: 100,
    };
    frames.ask(wanted);
    assert!(frames.asked(wanted));
    // The geometry moved on before the engine answered: the raster it sends is
    // no longer the one a page is waiting for.
    assert!(!frames.take(stale), "an answer nobody asked for is not wanted");
    assert!(frames.take(wanted), "the one that was asked for is");
    assert!(!frames.asked(wanted), "and it is no longer owed");
    assert!(!frames.current(wanted), "an answer is not a raster until it lands");
    frames.hold(raster(3, 100, 200));
    assert!(frames.current(wanted));
}

#[test]
fn the_rasters_are_let_go_with_the_pages_that_are_no_longer_mounted() {
    let mut frames = Frames::default();
    frames.hold(raster(1, 10, 10));
    frames.hold(raster(2, 10, 10));
    let in_flight = FrameKey {
        page: 9,
        width: 10,
        height: 10,
    };
    frames.ask(in_flight);
    frames.retain(&[1, 2]);
    assert!(frames.get(1).is_some());
    assert!(frames.get(2).is_some());
    // A raster still in flight for a page nobody is looking at is dropped where
    // it lands, so what was asked for it goes too.
    assert!(!frames.asked(in_flight));
}

#[test]
fn the_pump_asks_once_for_the_page_on_screen_and_lets_go_of_the_one_behind() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 5, 3);
    reader.pump_frames();
    assert_eq!(reader.frame_pages(), vec![3], "one page at a time");
    assert!(reader.frames.asked(reader.frame_key(3)));
    reader.pump_frames();
    assert_eq!(reader.frames.asked.len(), 1, "an identical ask is not repeated");
    // A page the reader left is not asked for again, and its raster is let go.
    reader.frames.hold(raster(3, 10, 10));
    reader.turn(1);
    reader.pump_frames();
    assert!(reader.frames.get(3).is_none(), "page three is off screen");
    assert!(reader.frames.asked(reader.frame_key(4)));
}

#[test]
fn the_column_asks_for_the_page_under_the_readers_eyes_first() {
    let mut reader = pages_of(40, 30, ViewMode::ScrollVertical);
    reader.reflow();
    reader.pump_frames();
    let asked = reader.frames.asked.clone();
    assert!(!asked.is_empty());
    assert_eq!(asked[0].page, 30, "the raster being waited on is the one in front of them");
    assert!(asked.len() <= IN_FLIGHT, "and the queue stays the size of the window's head");
    assert_eq!(reader.frame_pages()[0], 30, "the window is ordered outward from it");
}

/// An open raises the cover: the reader's page is up before the race to it is
/// something they can watch.
#[test]
fn an_open_raises_the_cover_on_the_mount_it_is_racing() {
    use crate::formats::pdf;
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    reader.session += 1;
    reader.document.begin(&kit_open());
    reader.seed(pdf::Opened {
        path: std::path::PathBuf::from("/books/one.pdf"),
        num_pages: 5,
        page_sizes: vec![pdf::PageBox { width: 612.0, height: 792.0 }; 5],
        title: None,
        author: None,
    });
    assert!(reader.cover_up());
}

#[test]
fn the_cover_lifts_when_the_page_the_reader_is_on_has_painted() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 5, 3);
    reader.cover = Some(Instant::now());
    reader.painted(1);
    assert!(reader.cover_up(), "another page's paint is not the reader's");
    reader.painted(3);
    assert!(!reader.cover_up());
}

#[test]
fn a_cover_that_never_paints_lifts_on_its_own_clock() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 5, 3);
    let raised = Instant::now();
    reader.cover = Some(raised);
    reader.cover_expired(raised + Duration::from_millis(COVER_GRACE_MS - 1));
    assert!(reader.cover_up(), "the net has not expired");
    reader.cover_expired(raised + Duration::from_millis(COVER_GRACE_MS));
    assert!(!reader.cover_up(), "and a render nobody is coming to make is a wait, not a cover");
}

#[test]
fn a_failed_render_lifts_the_cover_over_the_page_it_failed_on() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 5, 3);
    reader.cover = Some(Instant::now());
    reader.frame_failed(2);
    assert!(reader.cover_up());
    reader.frame_failed(3);
    assert!(!reader.cover_up());
}

#[test]
fn the_page_host_draws_the_frame_it_has_and_the_box_it_expects() {
    // The box on screen never depends on whether a raster has arrived: the page
    // must not change size the moment its pixels do.
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 3, 1);
    let expected = reader.page_box_px();
    assert!(expected.0 > 0.0 && expected.1 > 0.0);
    let key = reader.frame_key(1);
    reader.frames.hold(Frame {
        key,
        handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
    });
    assert!(reader.frame_here(1).is_some(), "the page's own raster is up");
    assert_eq!(reader.page_box_px(), expected, "and the page has not moved");
}

#[test]
fn a_frame_answer_for_another_session_or_a_key_nobody_asked_for_is_dropped() {
    let mut reader = reader();
    reader.viewer.container = Size::new(800.0, 1000.0);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 3, 1);
    reader.pump_frames();
    let key = reader.frame_key(1);
    let answer = |stamp: u64, key: FrameKey| Event::Frame {
        stamp,
        key,
        width: key.width,
        height: key.height,
        pixels: vec![0, 0, 0, 0],
    };
    let stale_session = reader.session + 1;
    assert!(reader.on_event(answer(stale_session, key)).is_empty());
    assert!(reader.frame_here(1).is_none(), "another book's answer is thrown away");
    let unasked = FrameKey {
        page: 2,
        width: key.width,
        height: key.height,
    };
    assert!(reader.on_event(answer(reader.session, unasked)).is_empty());
    assert!(reader.frame_here(2).is_none());
    assert!(reader.on_event(answer(reader.session, key)).is_empty());
    assert!(reader.frame_here(1).is_some(), "and the one that was asked for lands");
}
