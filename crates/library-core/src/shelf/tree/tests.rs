//! The nesting rules' own cases.

use super::*;
use crate::shelf::ALL_SHELF;
use crate::shelf::kit::{cut, ids_of, in_place_tree, nested, plain, shelf};
use crate::shelf::tree::ancestors;
use crate::shelf::tree::can_nest;
use crate::shelf::tree::children_of;
use crate::shelf::tree::lift_children;
use crate::shelf::tree::rehang_moves;
use crate::shelf::tree::reparent;
use crate::shelf::tree::subtree_ids;

#[test]
fn a_level_is_the_shelves_filed_directly_inside_it() {
    let shelves = vec![
        shelf("s1", "Fiction", &[]),
        nested("s2", "Sci-fi", "s1"),
        nested("s3", "Crime", "s1"),
        nested("s4", "Space", "s2"),
    ];
    assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2", "s3"]);
    assert_eq!(ids_of(&children_of(&shelves, Some("s2"))), vec!["s4"]);
    assert!(children_of(&shelves, Some("nope")).is_empty());
}

#[test]
fn the_way_out_of_a_shelf_is_the_chain_above_it() {
    let shelves = vec![
        shelf("s1", "Fiction", &[]),
        nested("s2", "Sci-fi", "s1"),
        nested("s3", "Space", "s2"),
    ];
    assert!(ancestors(&shelves, "s1").is_empty());
    assert_eq!(ids_of(&ancestors(&shelves, "s2")), vec!["s1"]);
    assert_eq!(ids_of(&ancestors(&shelves, "s3")), vec!["s1", "s2"]);
    assert!(ancestors(&shelves, ALL_SHELF).is_empty());
    assert!(ancestors(&shelves, "nope").is_empty());
}

#[test]
fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
    // s3 is inside s2 and both sit at the root, so s1 is the one shelf
    // that is nobody's ancestor.
    let shelves = vec![
        shelf("s1", "Fiction", &[]),
        shelf("s2", "Sci-fi", &[]),
        nested("s3", "Space", "s2"),
    ];
    assert!(can_nest(&shelves, "s1", "s3"), "s1 is above nothing, so it can go deepest");
    assert!(can_nest(&shelves, "s2", "s1"));
    assert!(can_nest(&shelves, "s3", "s1"), "and a shelf may be lifted out of its branch");
    assert!(!can_nest(&shelves, "s2", "s2"), "a shelf is not inside itself");
    assert!(!can_nest(&shelves, "s1", "s1"));
    assert!(!can_nest(&shelves, "s2", "s3"), "s3 is already inside s2");
    assert!(!can_nest(&shelves, "s2", "s2"));
    assert!(can_nest(&shelves, "s9", "s1"));
    let mut none: Vec<Shelf> = Vec::new();
    assert!(!reparent(&mut none, "s9", Some("s1")));
}

#[test]
fn a_nest_writes_one_parent_and_a_refusal_writes_nothing() {
    let mut shelves = vec![
        shelf("s1", "Fiction", &[]),
        shelf("s2", "Sci-fi", &[]),
        nested("s3", "Space", "s2"),
    ];
    assert!(reparent(&mut shelves, "s2", Some("s1")));
    assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
    assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2"]);
    assert!(!can_nest(&shelves, "s1", "s3"));
    assert!(!reparent(&mut shelves, "s1", Some("s3")));
    assert_eq!(shelves[0].parent, None);
    assert!(reparent(&mut shelves, "s2", None));
    assert_eq!(shelves[1].parent, None);
}

#[test]
fn taking_a_shelf_apart_lifts_the_shelves_inside_it() {
    let mut shelves = vec![
        shelf("s1", "Fiction", &[]),
        nested("s2", "Sci-fi", "s1"),
        nested("s3", "Space", "s2"),
        shelf("s4", "Unrelated", &[]),
    ];
    lift_children(&mut shelves, "s2");
    assert_eq!(
        shelves[2].parent.as_deref(),
        Some("s1"),
        "s3 inherits the level s2 was on, not the top of the library"
    );
    assert_eq!(shelves[3].parent, None, "a shelf elsewhere is not touched");
    lift_children(&mut shelves, "s1");
    assert_eq!(shelves[1].parent, None, "a root shelf's children become roots");
    assert_eq!(
        shelves[2].parent, None,
        "including the one that just moved up into it"
    );
}

#[test]
fn a_flat_subfolder_shelf_rehangs_under_the_rung_it_was_cut_from() {
    // An older build minted "2/deep" as a sibling of the root; the disk's
    // tree says it hangs on "2".
    let shelves = vec![
        cut("r", "f1", None, None),
        cut("two", "f1", Some("2"), None),
        cut("deep", "f1", Some("2/deep"), None),
    ];
    let moves = rehang_moves(&shelves, "f1");
    assert_eq!(
        moves,
        vec![
            ("two".to_string(), Some("r".to_string())),
            ("deep".to_string(), Some("two".to_string())),
        ]
    );
}

#[test]
fn a_hand_moved_shelf_keeps_its_place_and_still_routes_its_subtree() {
    let mut two = cut("two", "f1", Some("2"), Some("r"));
    two.manual_parent = true;
    let shelves = vec![
        cut("r", "f1", None, None),
        two,
        // The subtree the reader carried off re-hangs together.
        cut("deep", "f1", Some("2/deep"), None),
    ];
    let moves = rehang_moves(&shelves, "f1");
    assert_eq!(moves, vec![("deep".to_string(), Some("two".to_string()))]);
}

#[test]
fn a_rehang_never_touches_a_virtual_shelf_or_another_folders() {
    let shelves = vec![
        cut("r", "f1", None, None),
        plain("mine", &[]),
        cut("other", "f2", None, None),
    ];
    assert!(rehang_moves(&shelves, "f1").is_empty());
}

#[test]
fn a_stale_rung_that_names_the_shelf_itself_rehangs_on_the_rung_above_it() {
    // The shelf's own key is never its seat: the rung above it answers,
    // and with no root rung in the list that is the library's top level.
    let shelves = vec![cut("loop", "f1", Some("loop"), Some("r"))];
    let moves = rehang_moves(&shelves, "f1");
    assert_eq!(moves, vec![("loop".to_string(), None)]);
}

#[test]
fn a_rung_under_a_level_that_was_taken_apart_hangs_inside_the_tree() {
    // `2nd` stood inside `1st` and `1st` is gone: its books come up to
    // the rung the tree still stands on, never out of the tree.
    let shelves = vec![
        cut("r", "f1", None, None),
        cut("second", "f1", Some("1st/2nd"), None),
    ];
    assert_eq!(
        rehang_moves(&shelves, "f1"),
        vec![("second".to_string(), Some("r".to_string()))]
    );
}

#[test]
fn a_hand_that_puts_a_shelf_back_on_its_seat_gives_the_scan_its_place_back() {
    let (mut shelves, _) = in_place_tree();
    assert!(reparent(&mut shelves, "sf", Some("mine")));
    assert!(marked(&shelves, "sf"));
    // Back on the seat the disk names, the mark comes off.
    assert!(reparent(&mut shelves, "sf", Some("fic")));
    assert!(!marked(&shelves, "sf"));
    // A re-order on the shelf's current seat writes the same answer it had.
    assert!(reparent(&mut shelves, "sf", Some("fic")));
    assert!(!marked(&shelves, "sf"));
}

#[test]
fn the_subtree_is_everything_below_and_never_the_root_itself() {
    let tree = vec![
        shelf("a", "A", &[]),
        nested("b", "B", "a"),
        nested("c", "C", "b"),
        nested("d", "D", "a"),
    ];
    let mut under_a = subtree_ids(&tree, &["a".to_string()]);
    under_a.sort();
    assert_eq!(
        under_a,
        vec!["b".to_string(), "c".to_string(), "d".to_string()]
    );
    assert!(
        subtree_ids(&tree, &["d".to_string()]).is_empty(),
        "an empty leaf has no subtree"
    );
    let mut under_both = subtree_ids(&tree, &["a".to_string(), "b".to_string()]);
    under_both.sort();
    assert_eq!(under_both, vec!["c".to_string(), "d".to_string()]);
    // A loop a hand-edited blob can carry still terminates: the seen-set
    // refuses the second visit of a root.
    let looped = vec![nested("x", "X", "y"), nested("y", "Y", "x")];
    assert_eq!(
        subtree_ids(&looped, &["x".to_string()]),
        vec!["y".to_string()]
    );
}

fn marked(shelves: &[Shelf], id: &str) -> bool {
    shelves
        .iter()
        .find(|s| s.id == id)
        .is_some_and(|s| s.manual_parent)
}
