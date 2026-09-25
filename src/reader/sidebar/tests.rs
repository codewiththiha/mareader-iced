//! The rail's own cases: the motion it opens through, the panel it keeps up
//! while it closes, and the two doors the reader has into it.

use std::time::Instant;

use iced::Size;
use reader_core::outline::OutlineNode;
use reader_core::settings::Settings;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use super::outline::{centre, reveal};
use super::slide::{HOVER_GRACE_MS, SLIDE_MS};
use super::{Action, Panel, RAIL_W, Sidebar};
use crate::reader::kit::{reader_of, seeded, tick};
use crate::reader::{Effect, Message, Reader};

/// The window these cases are measured in, and a clock long enough to settle any
/// motion the rail starts.
const SETTLED_MS: f64 = 400.0;

fn page() -> Size {
    Size::new(1000.0, 800.0)
}

/// A reader on a book of A4 pages, in the paginated mode, with a measured
/// window: the fixture the strip's own cases start from, without a strip.
fn book(floating: bool) -> Reader {
    let mut settings = Settings::default();
    settings.layout.default_fit = FitMode::Width;
    settings.layout.page_margin = 0.0;
    settings.layout.sidebar_overlay = floating;
    let mut reader = reader_of(&settings);
    reader.viewer.mode = ViewMode::Single;
    seeded(&mut reader, 40, 1);
    reader.update(Message::Resized(page()), Instant::now());
    reader
}

/// A chapter tree of `count` entries, one page apart.
fn chapters(count: usize) -> Vec<OutlineNode> {
    (0..count)
        .map(|index| OutlineNode::new(format!("Chapter {}", index + 1), index as u32 + 1, 1))
        .collect()
}

/// A rail opened on its panel and settled: what most of these cases start from.
fn opened(settings: &Settings) -> Sidebar {
    let mut rail = Sidebar::new(settings);
    rail.toggle();
    rail.step(SLIDE_MS);
    rail
}

#[test]
fn the_open_and_the_close_keep_the_panel_painted_to_the_end() {
    let mut rail = Sidebar::new(&Settings::default());
    assert!(!rail.present(), "a fresh reader has no rail on screen");
    assert_eq!(rail.slot(), 0.0);

    rail.toggle();
    assert_eq!(
        rail.panel(),
        Some(Panel::Outline),
        "the toggle opens the panel a close left behind"
    );
    assert!(rail.present());
    assert!(rail.ticking(), "a motion owes frames");
    assert_eq!(rail.factor(), 0.0, "the rail starts at the window's edge");

    rail.step(SLIDE_MS / 2.0);
    let half = rail.factor();
    assert!(half > 0.0 && half < 1.0, "half the clock is part of the way");
    assert!(
        (rail.slot() - RAIL_W * half).abs() < 1e-3,
        "the page gives up exactly what the rail has taken"
    );

    rail.step(SLIDE_MS / 2.0);
    assert_eq!(rail.factor(), 1.0);
    assert!(!rail.ticking());

    // The close: the mode is gone, the panel is not — it leaves with the rail.
    rail.toggle();
    assert!(rail.present(), "a close is still a rail on screen");
    assert_eq!(rail.panel(), Some(Panel::Outline), "painted to the end");
    rail.step(SLIDE_MS);
    assert!(!rail.present());
    assert_eq!(rail.panel(), None);
    assert_eq!(rail.slot(), 0.0);
}

#[test]
fn a_frozen_rail_lands_in_the_frame_it_was_asked_in() {
    let mut settings = Settings::default();
    settings.animations.sidebar_slide = false;
    let mut rail = Sidebar::new(&settings);
    rail.toggle();
    assert_eq!(rail.factor(), 1.0);
    assert!(!rail.ticking(), "no motion, no frames");
    rail.toggle();
    assert!(!rail.present(), "and no slide to paint through");
}

#[test]
fn a_floating_rail_opens_the_same_way_and_takes_nothing_from_the_page() {
    let mut settings = Settings::default();
    settings.layout.sidebar_overlay = true;
    let mut rail = Sidebar::new(&settings);
    assert!(!rail.docks());
    rail.toggle();
    rail.step(SLIDE_MS);
    assert_eq!(rail.factor(), 1.0, "the same window, the same motion");
    assert_eq!(rail.slot(), 0.0, "the page keeps its width");
}

#[test]
fn a_floating_rail_closes_a_grace_after_the_pointer_leaves_it() {
    let mut settings = Settings::default();
    settings.layout.sidebar_overlay = true;
    let mut rail = opened(&settings);
    rail.hold(false);
    assert!(rail.ticking(), "the grace rides the rail's own clock");
    rail.step(HOVER_GRACE_MS / 2.0);
    assert!(rail.present(), "half the grace is not the end of it");
    rail.hold(true);
    rail.step(HOVER_GRACE_MS);
    assert!(rail.present(), "a pointer that came back keeps the rail");
    rail.hold(false);
    rail.step(HOVER_GRACE_MS);
    assert_eq!(
        rail.panel(),
        Some(Panel::Outline),
        "the grace closed it onto the way out"
    );
    rail.step(SLIDE_MS);
    assert!(!rail.present(), "and a pointer that left leaves no rail behind");

    // A docked rail is opened and closed by hand only.
    let mut docked = opened(&Settings::default());
    docked.hold(false);
    docked.step(HOVER_GRACE_MS * 2.0);
    assert!(docked.present());
}

#[test]
fn the_panel_follows_the_reader_and_centres_when_it_is_asked() {
    let mut rail = opened(&Settings::default());
    rail.measured(500.0);
    rail.scrolled(0.0);

    // The reader moves into a chapter below the panel's window.
    rail.watch(Some(30), 31);
    assert_eq!(rail.aim_effect(), None, "the panel is not built yet");
    let owed = rail.aim_effect().expect("the reveal is owed");
    let expected = reveal(30, 500.0, 0.0).expect("a row below the window");
    assert!((owed - expected).abs() < 1e-6);

    // The same page again: the panel is not scrolled at twice.
    rail.watch(Some(30), 31);
    assert_eq!(rail.aim_effect(), None);

    // The deliberate gesture moves whether or not the row is on screen.
    rail.centre(Some(2));
    assert_eq!(rail.aim_effect(), None);
    let asked = rail.aim_effect().expect("the gesture moves");
    assert!((asked - centre(2, 500.0)).abs() < 1e-6);

    // A panel nobody has measured cannot be told where to scroll.
    let mut unbuilt = opened(&Settings::default());
    unbuilt.watch(Some(30), 31);
    assert_eq!(unbuilt.aim_effect(), None);
}

#[test]
fn a_chapter_jumps_the_book_and_reports_the_move() {
    let mut reader = book(false);
    assert!(reader.document.outline.is_empty());
    reader.document.outline = chapters(20);

    let effects = reader.update(Message::Sidebar(Action::Chapter(3)), Instant::now());
    assert_eq!(reader.viewer.page, 4, "the chapter's own page");
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Progress(_))),
        "a jump is the reader's own report, and the app writes it"
    );

    // A row that is not there moves nothing.
    let effects = reader.update(Message::Sidebar(Action::Chapter(99)), Instant::now());
    assert!(effects.is_empty());
    assert_eq!(reader.viewer.page, 4);
}

#[test]
fn the_reading_area_follows_a_docked_rail_and_not_a_floating_one() {
    let mut docked = book(false);
    assert_eq!(docked.viewer.container, page(), "the area is the window");
    docked.update(Message::Sidebar(Action::Toggle), Instant::now());
    tick(&mut docked, SETTLED_MS);
    assert_eq!(docked.viewer.container.width, page().width - RAIL_W);
    assert_eq!(docked.viewer.container.height, page().height, "width only");

    let mut floating = book(true);
    floating.update(Message::Sidebar(Action::Toggle), Instant::now());
    tick(&mut floating, SETTLED_MS);
    assert_eq!(floating.viewer.container, page(), "the rail lies over the page");

    // And the page gets its width back the moment the rail goes away.
    docked.update(Message::Sidebar(Action::Toggle), Instant::now());
    tick(&mut docked, SETTLED_MS);
    assert_eq!(docked.viewer.container, page());
}
