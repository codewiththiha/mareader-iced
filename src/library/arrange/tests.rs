//! The arrangement rules' own cases.

use super::*;
use super::order::{insert_many, place_many};
use library_core::book::Row;
use library_core::folder::WatchedFolder;
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::testkit::{self, markdown_row};

fn row(id: &str) -> Row {
    markdown_row(id)
}

fn list() -> Vec<Row> {
    vec![row("a"), row("b"), row("c"), row("d")]
}

fn ids(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(Row::id).collect()
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

/// `home/root/1st/2nd`: a read-at-place tree with a rung per folder, its
/// books on the lowest one.
fn deep_tree() -> (Vec<Shelf>, Vec<Row>, WatchedFolder) {
    let folder = WatchedFolder {
        placed: std::collections::HashSet::from([
            library_core::testkit::fp_n(7),
            library_core::testkit::fp_n(8),
            library_core::testkit::fp_n(9),
        ]),
        shelf_map: std::collections::BTreeMap::from([
            (String::new(), "root".to_string()),
            ("1st".to_string(), "one".to_string()),
            ("1st/2nd".to_string(), "two".to_string()),
        ]),
        ..library_core::testkit::watched_folder("f1", "/books")
    };
    let shelves = vec![
        testkit::folder_shelf("root", "root", "f1", None, &["b0"], None),
        testkit::folder_shelf("one", "one", "f1", Some("1st"), &[], Some("root")),
        testkit::folder_shelf("two", "two", "f1", Some("1st/2nd"), &["b1", "b2"], Some("one")),
    ];
    let books = vec![
        testkit::row_at_n("b0", "/books/notes.md", 8),
        testkit::row_at_n("b1", "/books/1st/2nd/a.md", 7),
        testkit::row_at_n("b2", "/books/1st/2nd/b.md", 9),
    ];
    (shelves, books, folder)
}

#[test]
fn taking_a_rung_apart_brings_its_books_up_one_level_inside_the_tree() {
    let (mut shelves, mut rows, folder) = deep_tree();
    let mut folders = vec![folder];
    let stepped = dismantle(&mut shelves, &mut folders, &mut rows, "two").expect("the rung");
    assert_eq!(stepped, "one", "a reader standing on it steps out to the level it hung from");
    assert!(shelves.iter().all(|s| s.id != "two"), "one level, and only that one");
    let one = shelves.iter().find(|s| s.id == "one").expect("the level above it stands");
    assert_eq!(
        one.books,
        vec!["b1".to_string(), "b2".to_string()],
        "the books come up exactly one level"
    );
    assert_eq!(
        folders[0].shelf_map.get("1st/2nd"),
        None,
        "and the folder's map lets the rung go"
    );
}

#[test]
fn the_level_below_a_hole_still_hangs_inside_the_tree() {
    let (mut shelves, mut rows, folder) = deep_tree();
    let mut folders = vec![folder];
    assert_eq!(dismantle(&mut shelves, &mut folders, &mut rows, "one"), Some("root".to_string()));
    assert_eq!(dismantle(&mut shelves, &mut folders, &mut rows, "two"), Some("root".to_string()));
    let root = shelves.iter().find(|s| s.id == "root").expect("the tree's own rung");
    assert_eq!(
        root.books,
        vec!["b0".to_string(), "b1".to_string(), "b2".to_string()],
        "the repro: `2nd` comes up to `1st`, and once `1st` is a hole, to the root"
    );
    assert_eq!(
        shelves.iter().filter(|s| s.is_folder()).count(),
        1,
        "one level left, and no shelf standing beside the tree"
    );
}

#[test]
fn the_root_rung_has_nothing_above_it_to_come_up_to() {
    let (mut shelves, mut rows, folder) = deep_tree();
    let mut folders = vec![folder];
    let stepped = dismantle(&mut shelves, &mut folders, &mut rows, "root").expect("the root");
    assert_eq!(stepped, ALL_SHELF, "the root hangs from the library's own level");
    // The tree's own root going leaves no rung of the folder's above its
    // books: they stay in the library at the top level, and the levels
    // below keep the shape the tree gave them.
    assert!(rows.iter().any(|r| r.id() == "b0"), "no book goes with the shelf");
    assert!(
        shelves.iter().all(|s| !s.books.contains(&"b0".to_string())),
        "and no shelf seats it: the library's own floor does"
    );
    let one = shelves.iter().find(|s| s.id == "one").expect("the rung below stands");
    assert_eq!(one.parent, None, "and it hangs from the top now");
    assert!(shelves.iter().any(|s| s.id == "two"));
}

#[test]
fn a_drop_on_the_root_puts_one_row_where_the_reader_pointed() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["d"]), Some(1));
    assert_eq!(ids(&rows), vec!["a", "d", "b", "c"]);
}

#[test]
fn the_index_counts_the_list_as_it_was_before_the_lift() {
    // The reader pointed at the slot "d" occupied while they were
    // holding the two, not two further down the list the lift just
    // shortened.
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["a", "b"]), Some(3));
    assert_eq!(ids(&rows), vec!["c", "a", "b", "d"]);
}

#[test]
fn a_drop_past_the_end_appends() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["a"]), Some(99));
    assert_eq!(ids(&rows), vec!["b", "c", "d", "a"]);
}

#[test]
fn an_append_keeps_the_payload_s_order_not_the_list_s() {
    // A set has no order, so the payload is sorted into the level's own
    // order — by position in `row_ids`, because putting them back in
    // the list's order would be a drop that quietly shuffled the hand.
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["c", "a"]), None);
    assert_eq!(ids(&rows), vec!["b", "d", "c", "a"]);
}

#[test]
fn a_row_the_list_does_not_hold_is_not_invented() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["gone", "b"]), Some(0));
    assert_eq!(ids(&rows), vec!["b", "a", "c", "d"]);
}

#[test]
fn a_link_is_reordered_by_its_own_id_like_any_other_row() {
    let mut rows = vec![
        row("a"),
        Row::link("l1".into(), "Dune".into(), "a".into(), 1),
        row("b"),
    ];
    reorder_root(&mut rows, &owned(&["l1"]), Some(0));
    assert_eq!(ids(&rows), vec!["l1", "a", "b"]);
}

#[test]
fn an_empty_set_leaves_the_list_alone() {
    let mut rows = list();
    assert!(!reorder_root(&mut rows, &[], Some(0)));
    assert_eq!(ids(&rows), vec!["a", "b", "c", "d"]);
}

#[test]
fn a_bulk_move_to_the_root_removes_each_row_from_every_shelf() {
    let mut rows = list();
    let mut shelves = vec![
        testkit::shelf("from", "From", &["a", "b"], None),
        testkit::shelf("also", "Also", &["a", "c"], None),
    ];
    assert!(move_many_to_shelf(
        &mut shelves,
        &mut rows,
        &owned(&["a", "b"]),
        Some("from"),
        ALL_SHELF,
        Some(4),
    ));
    assert!(
        shelves
            .iter()
            .all(|shelf| !shelf.books.iter().any(|id| id == "a" || id == "b")),
        "a move to All uses the same everywhere-removal as a single-row move"
    );
    assert_eq!(ids(&rows), vec!["c", "d", "a", "b"]);
}

#[test]
fn a_move_onto_a_shelf_lifts_off_the_source_and_lands_at_the_slot() {
    let mut rows = list();
    let mut shelves = vec![
        testkit::shelf("from", "From", &["a", "b"], None),
        testkit::shelf("to", "To", &["c"], None),
    ];
    assert!(move_many_to_shelf(
        &mut shelves,
        &mut rows,
        &owned(&["a", "b"]),
        Some("from"),
        "to",
        Some(1),
    ));
    let to = shelf::find(&shelves, "to").unwrap();
    assert_eq!(to.books, owned(&["c", "a", "b"]));
    let from = shelf::find(&shelves, "from").unwrap();
    assert!(from.books.is_empty());
}

#[test]
fn a_book_already_on_the_shelf_is_moved_not_duplicated() {
    let mut members = owned(&["a", "b", "c"]);
    place_many(&mut members, &owned(&["a"]), Some(2));
    assert_eq!(members, vec!["b", "a", "c"], "one membership, in the slot the drop named");
}

#[test]
fn the_shift_is_counted_per_book_rather_than_for_the_batch() {
    // Counting the batch instead of the members would have landed the
    // three one slot early.
    let mut members = owned(&["a", "x", "c", "y"]);
    place_many(&mut members, &owned(&["a", "b", "c"]), Some(3));
    assert_eq!(members, vec!["x", "a", "b", "c", "y"]);
}

#[test]
fn filing_with_no_index_appends_in_order() {
    let mut members = owned(&["a"]);
    place_many(&mut members, &owned(&["b", "c"]), None);
    assert_eq!(members, vec!["a", "b", "c"]);
}

#[test]
fn each_item_lands_after_the_last_rather_than_all_at_one_place() {
    let mut list: Vec<&str> = vec!["x", "y"];
    insert_many(&mut list, ["a", "b", "c"].into_iter(), Some(1), 0);
    assert_eq!(list, vec!["x", "a", "b", "c", "y"], "not reversed");
}

#[test]
fn an_index_past_the_end_clamps_per_item() {
    let mut list: Vec<&str> = vec!["x"];
    insert_many(&mut list, ["a", "b"].into_iter(), Some(99), 0);
    assert_eq!(list, vec!["x", "a", "b"]);
}

#[test]
fn a_shift_larger_than_the_index_lands_at_the_front() {
    let mut list: Vec<&str> = vec!["x", "y"];
    insert_many(&mut list, ["a"].into_iter(), Some(1), 4);
    assert_eq!(list, vec!["a", "x", "y"]);
}

#[test]
fn a_filing_names_a_book_already_a_member_without_moving_it() {
    let mut shelves = vec![testkit::shelf("to", "To", &["a", "b"], None)];
    assert!(!file_many(&mut shelves, &owned(&["a"]), "to"));
    assert_eq!(shelves[0].books, owned(&["a", "b"]));
    assert!(file_many(&mut shelves, &owned(&["c"]), "to"));
    assert_eq!(shelves[0].books, owned(&["a", "b", "c"]));
}

#[test]
fn an_unfile_lets_go_of_one_shelf_alone() {
    let mut shelves = vec![
        testkit::shelf("here", "Here", &["a", "b"], None),
        testkit::shelf("there", "There", &["a"], None),
    ];
    assert!(unfile_books(&mut shelves, &owned(&["a"]), "here"));
    assert_eq!(shelves[0].books, owned(&["b"]));
    assert_eq!(shelves[1].books, owned(&["a"]), "the other membership stands");
}

#[test]
fn a_sibling_seam_lands_the_moved_shelf_beside_its_anchor() {
    let mut shelves = vec![
        testkit::shelf("x", "X", &[], None),
        testkit::shelf("y", "Y", &[], None),
        testkit::shelf("z", "Z", &[], None),
    ];
    assert!(reorder_shelves_to_anchor(&mut shelves, &owned(&["x"]), "z", false));
    let order: Vec<&str> = shelves.iter().map(|shelf| shelf.id.as_str()).collect();
    assert_eq!(order, vec!["y", "x", "z"]);
}

#[test]
fn a_folder_cannot_nest_inside_itself() {
    let mut shelves = vec![testkit::shelf("outer", "Outer", &[], None), testkit::shelf("inner", "Inner", &[], Some("outer"))];
    assert!(!nest_shelf(&mut shelves, "outer", Some("inner")));
    assert!(nest_shelf(&mut shelves, "inner", None));
    assert!(shelf::find(&shelves, "inner").unwrap().parent.is_none());
}

#[test]
fn a_filing_answers_false_for_a_shelf_that_is_not_there() {
    let mut shelves = vec![testkit::shelf("to", "To", &[], None)];
    assert!(!file_many(&mut shelves, &owned(&["a"]), "gone"));
    assert!(shelves[0].books.is_empty());
}

#[test]
fn a_nesting_moves_the_folder_and_reports_the_move() {
    let mut shelves =
        vec![testkit::shelf("a", "A", &[], None), testkit::shelf("b", "B", &[], None), testkit::shelf("home", "Home", &[], None)];
    assert!(nest_many(&mut shelves, &owned(&["a", "b"]), "home"));
    assert_eq!(shelves[0].parent.as_deref(), Some("home"));
    assert_eq!(shelves[1].parent.as_deref(), Some("home"));
}
