//! The module's tests: the subjects beside this hold the code, and this
//! holds the cases they have to satisfy.

use super::*;

use library_core::view::{COLUMNS_MAX, COLUMNS_MIN};

use library_core::testkit;

#[test]
fn auto_shows_a_dash_and_leaves_both_steps_live() {
    let step = columns_step(None, 5);
    assert_eq!(step.face, "–", "the count is the flow's, not the reader's");
    assert!(step.down && step.up, "the first press pins what the flow was showing");
}

#[test]
fn a_pinned_count_offers_only_the_step_it_can_take() {
    let floor = columns_step(Some(COLUMNS_MIN), 5);
    assert_eq!(floor.face, COLUMNS_MIN.to_string());
    assert!(!floor.down && floor.up, "nothing below the floor to offer");

    let ceiling = columns_step(Some(COLUMNS_MAX), 5);
    assert!(!ceiling.up && ceiling.down, "nothing above the ceiling to offer");

    let between = columns_step(Some(COLUMNS_MIN + 1), 5);
    assert!(between.down && between.up);
}
#[test]
fn the_removal_row_says_what_leaves() {
    let link = testkit::link("l1", "Dune", "s1");
    assert_eq!(removal_row(&link).0, "Remove link", "a pointer, not a book");
    assert_eq!(removal_row(&testkit::row_at("b1", "Dune")).0, "Remove from library");
}

#[test]
fn the_selection_row_offers_the_other_end() {
    assert_eq!(selection_row(true).label, "Clear selection");
    assert_eq!(selection_row(false).label, "Select all");
}
