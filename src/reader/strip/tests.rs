//! The strip's own cases: where it mounts, how it holds a place across a
//! rescale, and what a report from the surface is allowed to move.
//!
//! A book of A4 pages at their own size — 612x792 — in a measured window, with
//! `PAGE_GAP` between them.

use iced::keyboard::{Key, key};
use reader_core::zoom_math::FitMode;

use super::*;
use crate::reader::kit::{pages_of, strip, tick};
use crate::reader::{Effect, Message};

/// The offset the column puts the head of `page` at.
fn head_of(page: u32) -> f64 {
    f64::from(page - 1) * (792.0 + PAGE_GAP)
}

/// A command to put the column at `offset`.
fn at(offset: f64) -> Effect {
    Effect::Scroll {
        axis: Axis::Vertical,
        offset,
    }
}

/// A command to put the horizontal strip at `offset`.
fn across(offset: f64) -> Effect {
    Effect::Scroll {
        axis: Axis::Horizontal,
        offset,
    }
}

#[test]
fn the_column_mounts_the_whole_book_and_builds_only_what_is_on_screen() {
    let mut reader = pages_of(40, 1, ViewMode::ScrollVertical);
    let effects = strip(&mut reader);
    let held = reader.strip_view().expect("a scrolling mode has a strip");
    assert_eq!(held.axis(), Axis::Vertical);
    assert_eq!(held.geometry().pages, 40);
    // The book: forty A4 pages and the gaps between them.
    assert_eq!(held.total(), 40.0 * 792.0 + 39.0 * PAGE_GAP);
    // The window: the pages the 1000px viewport covers, plus the half-window of
    // read-ahead either side.
    assert_eq!(held.mounted().map(|window| (window.first, window.last)), Some((0, 1)));
    assert_eq!(reader.dominant_page(), 1);
    // The reader is on page one, which is where a fresh surface already sits —
    // but it is still told, because the surface starts at the top of whatever
    // book it is handed.
    assert_eq!(effects, vec![at(0.0)]);
    assert!(reader.needs_tick(), "an aim owes the frames its retries ride on");
}

#[test]
fn the_mount_lands_on_the_page_the_reader_left_off_at() {
    let mut reader = pages_of(40, 30, ViewMode::ScrollVertical);
    let effects = strip(&mut reader);
    let held = reader.strip_view().expect("a strip");
    assert_eq!(held.offset(), head_of(30));
    assert_eq!(reader.dominant_page(), 30, "the page under their eyes is the one they left");
    assert_eq!(effects, vec![at(head_of(30))]);
}

#[test]
fn an_aim_is_posted_for_a_few_frames_and_released_by_its_own_report() {
    let mut reader = pages_of(40, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    // A mount posts against a surface that may not exist yet, and against
    // bounds that may have moved since: the aim is posted again until one of
    // them lands.
    assert!(reader.anchor_effect().is_some());
    assert!(reader.anchor_effect().is_some());
    assert!(reader.anchor_effect().is_none(), "the posts run out");
    // The surface agreeing with the aim is the one report that is definitely not
    // news, and it is also what releases the aim.
    assert!(reader.scrolled(0.0).is_empty());
    assert_eq!(reader.viewer.page, 1);
}

#[test]
fn a_stale_report_while_the_aim_is_being_posted_is_not_the_reader_s_place() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    // A long jump lands at once (nothing is gliding), and the surface reports
    // the offset it was still holding from before: that is not the reader
    // turning back to page one.
    assert!(!reader.jump_to(8).is_empty());
    assert_eq!(reader.viewer.page, 8, "the jump is the reader's");
    assert!(reader.scrolled(head_of(1)).is_empty(), "the stale report is ignored");
    assert_eq!(reader.viewer.page, 8);
    assert_eq!(reader.strip_view().unwrap().offset(), head_of(8));
}

#[test]
fn once_the_aim_is_spent_the_readers_own_scroll_is_the_page() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    reader.jump_to(8);
    while reader.anchor_effect().is_some() {}
    // The reader takes over: the surface is theirs now, and the page follows it
    // — once the scroll has stopped for long enough to be worth writing.
    assert!(reader.scrolled(head_of(3)).is_empty());
    assert_eq!(reader.viewer.page, 3);
    assert_eq!(reader.strip_view().unwrap().offset(), head_of(3));
    assert!(reader.needs_tick(), "the write is owed");
    for _ in 0..14 {
        tick(&mut reader, 16.0);
    }
    assert!(!reader.needs_tick());
}

#[test]
fn a_scrolls_page_is_reported_once_the_scroll_has_stopped() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    while reader.anchor_effect().is_some() {}
    reader.scrolled(head_of(2));
    // The app writes the whole library on a report, and a fling crosses pages
    // far faster than that is worth doing: the write waits for the quiet.
    let mut reported = Vec::new();
    for _ in 0..14 {
        let now = reader.last_tick + std::time::Duration::from_millis(16);
        reported.extend(reader.update(Message::Tick, now));
    }
    match reported.as_slice() {
        [Effect::Progress(read)] => assert_eq!(read.page, 2),
        other => panic!("expected exactly one progress report, got {other:?}"),
    }
}

#[test]
fn a_turn_in_the_column_glides_the_strip_to_the_page() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    let effects = reader.turn(1);
    assert_eq!(reader.viewer.page, 2, "the page turns as soon as it is asked");
    // The glide opens where the strip already is, and the surface keeps it
    // there until the next frame steps it.
    assert_eq!(
        effects.first(),
        Some(&at(0.0)),
        "a glide starts where the strip is"
    );
    assert!(reader.needs_tick(), "a glide owes frames");
    tick(&mut reader, 240.0);
    assert!(!reader.needs_tick(), "and lands");
    assert_eq!(reader.strip_view().unwrap().offset(), head_of(2));
}

#[test]
fn a_turn_is_reported_at_once_and_the_glide_does_not_hold_the_write_back() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    let effects = reader.turn(1);
    assert!(
        effects.iter().any(|effect| matches!(effect, Effect::Progress(_))),
        "a keypress is worth writing down at once"
    );
}

#[test]
fn a_long_jump_lands_at_once() {
    let mut reader = pages_of(400, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    let effects = reader.jump_to(360);
    // Two viewports is the longest a jump may be worth watching; across a book
    // it lands where it was aimed.
    assert_eq!(effects.first(), Some(&at(head_of(360))));
    assert!(
        matches!(effects.last(), Some(Effect::Progress(read)) if read.page == 360),
        "a page asked for is a page the app is told about at once"
    );
    assert!(reader.glide.is_none(), "a jump across a book is not eased");
    assert_eq!(reader.dominant_page(), 360);
}

#[test]
fn a_frozen_reader_lands_its_jumps_instead_of_easing_them() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    reader.motion = crate::reader::motion::Motion {
        glide: false,
        ..crate::reader::motion::Motion::default()
    };
    strip(&mut reader);
    // The mount's own aim is spent, so what is left is the turn's.
    while reader.anchor_effect().is_some() {}
    let effects = reader.turn(1);
    // One command, to the place the page is: nothing eases toward it, and the
    // aim stands until the surface answers it.
    assert_eq!(effects.first(), Some(&at(head_of(2))));
    assert!(reader.anchor_effect().is_some(), "the aim is re-posted, not forgotten");
    assert_eq!(reader.strip_view().unwrap().offset(), head_of(2));
}

#[test]
fn a_rescale_holds_the_point_under_the_middle_of_the_window() {
    let mut reader = pages_of(40, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    reader.scroll_by(400.0);
    // A zoom touching the display scale: the pages grow, the gap does not, and
    // the reader keeps the place they were looking at rather than a scaled
    // offset.
    reader.zoom.initialize(2.0);
    let effects = reader.reflow();
    let held = reader.strip_view().expect("a strip");
    assert_eq!(
        held.span(1),
        (1608.0, 1584.0),
        "the pages scaled, the gap did not"
    );
    assert_eq!(held.offset(), 1276.0);
    assert_eq!(effects, vec![at(1276.0)]);
}

#[test]
fn the_horizontal_strip_insets_its_ends_and_centres_the_page_it_lands_on() {
    // A window wider than the page, and a margin of the settings kind: two of
    // them are the spacing along the strip and one sits at each end, so a page
    // that fits is centred in what is left.
    let mut reader = pages_of(10, 5, ViewMode::ScrollHorizontal);
    // A window wider than one page, so a page that fits has room either side.
    reader.viewer.container = iced::Size::new(800.0, 1000.0);
    reader.viewer.margin = 20.0;
    strip(&mut reader);
    let held = reader.strip_view().expect("a strip");
    assert_eq!(held.axis(), Axis::Horizontal);
    assert_eq!(held.span(0), (20.0, 612.0), "the first page is inset by one");
    assert_eq!(held.span(1), (672.0, 612.0));
    // Two margins are the spacing: the pages are inset rather than gapped.
    assert_eq!(held.span(1).0 - (held.span(0).0 + held.span(0).1), 40.0);
    assert_eq!(held.total(), 10.0 * 612.0 + 9.0 * 40.0 + 2.0 * 20.0);
    // Centred: 612 wide in an 800 window, so 94 either side of it.
    let head = held.span(4).0;
    assert_eq!(held.offset(), head - 94.0);
    assert_eq!(reader.strip_viewport(), 800.0, "the window's width is the viewport");
    // The head of the book lands against the strip's own start rather than a
    // screenful before it: the inset is where the scroll stops.
    let mut first = pages_of(10, 1, ViewMode::ScrollHorizontal);
    first.viewer.container = iced::Size::new(800.0, 1000.0);
    first.viewer.margin = 20.0;
    strip(&mut first);
    assert_eq!(first.strip_view().unwrap().offset(), 0.0);
}

#[test]
fn a_wheel_notch_scrolls_the_horizontal_strip_and_leaves_the_column_alone() {
    // A wheel is a vertical gesture: iced hands it to the axis it points at, so
    // a horizontal strip hears nothing from it. The translation is the app's to
    // forward and the reader's to answer — and it is the horizontal mode's
    // alone, because a column already has an axis for the notch.
    let mut reader = pages_of(10, 1, ViewMode::ScrollHorizontal);
    reader.viewer.margin = 20.0;
    strip(&mut reader);
    while reader.anchor_effect().is_some() {}
    let before = reader.strip_view().unwrap().offset();
    let effects = reader.wheel(1.0);
    assert_eq!(effects, vec![across(before + 60.0)], "one notch is one step");
    assert_eq!(reader.viewer.page, 1, "a scroll is not a page turn until it settles");
    let mut column = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut column);
    assert!(column.wheel(1.0).is_empty(), "the column's own axis takes the notch");
}

#[test]
fn a_report_the_belief_already_agrees_with_moves_nothing() {
    let mut reader = pages_of(10, 4, ViewMode::ScrollVertical);
    strip(&mut reader);
    // The surface echoing a position it was just given: no page, no glide, no
    // work — which is what keeps a glide's own frames from cancelling it.
    let held = reader.strip_view().unwrap().offset();
    assert!(reader.scrolled(held).is_empty());
    assert_eq!(reader.viewer.page, 4);
}

#[test]
fn the_strip_is_let_go_when_nothing_scrolls_here() {
    let mut reader = pages_of(10, 4, ViewMode::ScrollVertical);
    strip(&mut reader);
    assert!(reader.strip_view().is_some());
    reader.viewer.mode = ViewMode::Single;
    assert!(strip(&mut reader).is_empty(), "a paginated mode has no strip to move");
    assert!(reader.strip_view().is_none());
    // And a scrolling mode with nothing open has nothing to window.
    reader.viewer.mode = ViewMode::ScrollVertical;
    reader.document.status = crate::reader::document::DocStatus::Idle;
    strip(&mut reader);
    assert!(reader.strip_view().is_none());
}

#[test]
fn a_page_step_moves_the_strip_by_a_step_and_stops_at_the_foot() {
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    assert_eq!(reader.scroll_by(720.0), vec![at(720.0)]);
    // The foot of the book is the last page's tail against the window's: a step
    // past it stops there rather than scrolling off the end of the book.
    let foot = 10.0 * 792.0 + 9.0 * PAGE_GAP - 1000.0;
    assert_eq!(reader.strip_view().unwrap().max_offset(1000.0), foot);
    assert_eq!(reader.scroll_by(10_000.0), vec![at(foot)]);
}

#[test]
fn the_page_keys_and_the_arrows_scroll_the_column_by_their_own_steps() {
    use crate::reader::keys::{line_step, page_step};
    let mut reader = pages_of(10, 1, ViewMode::ScrollVertical);
    strip(&mut reader);
    let down = Key::Named(key::Named::ArrowDown);
    let effects = reader.update(
        Message::Key {
            key: down,
            shift: false,
        },
        reader.last_tick,
    );
    assert_eq!(effects, vec![at(line_step(1000.0))], "an arrow is a reading nudge");
    assert_eq!(reader.viewer.page, 1, "and not a page turn");
    let space = Key::Named(key::Named::Space);
    let effects = reader.update(
        Message::Key {
            key: space,
            shift: false,
        },
        reader.last_tick,
    );
    // The step lands from where the arrow left the strip: until the surface
    // reports, the belief IS the command, so two steps in a row add up.
    assert_eq!(effects, vec![at(line_step(1000.0) + page_step(1000.0))]);
}

#[test]
fn flipping_back_to_a_paginated_mode_leaves_the_strip_behind() {
    let mut reader = pages_of(10, 4, ViewMode::ScrollVertical);
    strip(&mut reader);
    reader.update(Message::Mode(ViewMode::Single), reader.last_tick);
    assert!(reader.strip_view().is_none());
    assert_eq!(reader.viewer.page, 4, "the page they were on is the page they land on");
    assert_eq!(reader.viewer.fit, FitMode::Width, "and the paginated fit owns the scale");
    // And back: the column mounts on that same page.
    reader.update(Message::Mode(ViewMode::ScrollVertical), reader.last_tick);
    let held = reader.strip_view().expect("a strip again");
    assert_eq!(held.offset(), head_of(4));
}
