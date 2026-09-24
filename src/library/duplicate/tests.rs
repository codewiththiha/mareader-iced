//! The module's tests: the subjects beside this hold the code, and this
//! holds the cases they have to satisfy.

use super::*;

use std::collections::HashMap;

use library_core::book::{Fingerprint, Origin, Row};

use library_core::shelf::{self, Shelf};

use library_core::testkit;

/// One clock for the whole suite: the ids are a stamp plus a counter, so
/// a fixed stamp still mints distinct ids.
const NOW: u64 = 1_700_000_000_000;

fn nested_state() -> (Vec<Row>, Vec<Shelf>) {
    (
        vec![testkit::markdown_row("b1"), testkit::markdown_row("b2")],
        vec![
            testkit::shelf("s1", "Shelf", &["b1"], None),
            testkit::shelf("s2", "Inside", &["b2"], Some("s1")),
            testkit::shelf("s3", "Other", &[], None),
        ],
    )
}

fn copy_of(shelves: &[Shelf], name: &str) -> Shelf {
    shelves
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no shelf called {name}"))
        .clone()
}

/// The batch the store would have answered with, fabricated for the
/// members a plan wants to copy: every copy lands, wearing its own item
/// folder and its own measurement.
fn everything_landed(plan: &TreePlan) -> HashMap<String, (String, Option<Fingerprint>)> {
    plan.members
        .iter()
        .filter_map(|(_, what)| match what {
            Member::Copy { new_id, .. } => Some((
                new_id.clone(),
                (
                    format!("/app/Library/items/{new_id}/source.md"),
                    Some(testkit::fp_n(7)),
                ),
            )),
            _ => None,
        })
        .collect()
}

fn copies_of(plan: &TreePlan) -> Vec<&Member> {
    plan.members
        .iter()
        .map(|(_, what)| what)
        .filter(|what| matches!(what, Member::Copy { .. }))
        .collect()
}

#[test]
fn a_link_at_a_shelf_duplicates_as_a_link_beside_it() {
    let mut rows = vec![
        testkit::row_at("b1", "/books/dune.md"),
        testkit::link("l1", "Dune", "s1"),
    ];
    let mut s1 = testkit::plain_shelf("s1", &["b1", "l1"]);
    s1.name = "Shelf".into();
    let mut shelves = vec![s1];

    let name = land_shelf_link(&mut rows, &mut shelves, "s1", "l1", "Dune", "s1", NOW);
    assert_eq!(name, "Dune_1", "the level's counter, not a collision");
    assert_eq!(rows.len(), 3);
    let dup = rows.iter().find(|r| r.id() != "b1" && r.id() != "l1").unwrap();
    match dup {
        Row::Link { target, name, .. } => {
            assert_eq!(target, "s1", "the pointer points where the pointer pointed");
            assert_eq!(name, "Dune_1");
        }
        Row::Book(_) => panic!("a shelf's link duplicates as a link"),
    }
    assert_eq!(
        shelves[0].books,
        vec!["b1".to_string(), "l1".to_string(), dup.id().to_string()],
        "filed right behind the row the reader pointed at"
    );
}

#[test]
fn a_link_answers_with_what_it_points_at() {
    let rows = vec![
        testkit::row("b1"),
        testkit::link("l1", "Dune", "b1"),
        testkit::link("ls", "Shelf door", "s1"),
        testkit::link("dead", "Gone", "b9"),
    ];
    let mut missing = testkit::book("b2");
    missing.missing = true;
    let rows = [rows, vec![Row::Book(missing)]].concat();

    assert!(matches!(link_at(&rows, "s1"), LinkAt::Shelf));
    match link_at(&rows, "b1") {
        LinkAt::Book(book) => assert_eq!(book.id, "b1"),
        _ => panic!("a live book is a copy to make"),
    }
    assert!(
        matches!(link_at(&rows, "b9"), LinkAt::Dead),
        "a target no row answers for"
    );
    assert!(
        matches!(link_at(&rows, "b2"), LinkAt::Dead),
        "a target whose address died"
    );
    assert!(matches!(link_at(&rows, "l1"), LinkAt::Dead), "a link at a link");
}

#[test]
fn plan_one_answers_per_entry() {
    let (mut rows, shelves) = nested_state();
    rows.push(testkit::link("l1", "Dune", "b1"));
    rows.push(testkit::link("dead", "Gone", "b9"));

    match plan_one(&rows, &shelves, "b1", NOW) {
        DupPlan::Book(copy) => {
            assert_eq!(copy.beside, "b1");
            assert_eq!(copy.shown, "b1", "the book's own display name");
        }
        other => panic!("a book asks for a copy: {other:?}"),
    }
    match plan_one(&rows, &shelves, "l1", NOW) {
        DupPlan::Book(copy) => {
            assert_eq!(copy.beside, "l1", "beside the link, not the target");
            assert_eq!(copy.shown, "Dune", "the link's own name");
        }
        other => panic!("a link at a book asks for a copy: {other:?}"),
    }
    match plan_one(&rows, &shelves, "dead", NOW) {
        DupPlan::Dead(note) => assert!(note.contains("Gone"), "the toast names the link"),
        other => panic!("a dead link is the one loud refusal: {other:?}"),
    }
    match plan_one(&rows, &shelves, "s2", NOW) {
        DupPlan::Tree(plan) => assert_eq!(plan.name, "Inside_1"),
        other => panic!("a shelf asks for a tree: {other:?}"),
    }
    assert!(
        matches!(plan_one(&rows, &shelves, "gone", NOW), DupPlan::Skip),
        "an id no list answers for is nobody's to duplicate"
    );
}

#[test]
fn a_tree_plan_copies_every_member_once() {
    let (rows, shelves) = nested_state();
    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    assert_eq!(plan.name, "Shelf_1", "the level's counter, not a collision");
    assert_eq!(plan.label, "Shelf", "the card names what was duplicated");

    // One member per row, first-seen order: b1 off the root, b2 off the
    // rung inside.
    let order: Vec<&str> = plan.members.iter().map(|(old, _)| old.as_str()).collect();
    assert_eq!(order, vec!["b1", "b2"]);
    let copies = copies_of(&plan);
    assert_eq!(copies.len(), 2, "every member becomes a copy");
    let ids: Vec<&str> = plan
        .members
        .iter()
        .filter_map(|(_, what)| match what {
            Member::Copy { new_id, .. } => Some(new_id.as_str()),
            _ => None,
        })
        .collect();
    assert_ne!(ids[0], "b1", "the copy is a row of its own");
    assert_ne!(ids[1], "b2", "the copy is a row of its own");
    assert_ne!(ids[0], ids[1], "two members, two copies");
}

#[test]
fn a_missing_book_and_a_dead_link_are_skipped_not_shared() {
    let (mut rows, mut shelves) = nested_state();
    let mut missing = testkit::markdown_book("b3");
    missing.missing = true;
    rows.push(Row::Book(missing));
    rows.push(testkit::link("dead", "Gone", "b9"));
    shelves[0].books = vec!["b1".into(), "b3".into(), "dead".into()];

    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    let kinds: Vec<&str> = plan
        .members
        .iter()
        .map(|(_, what)| match what {
            Member::Copy { .. } => "copy",
            Member::Link(_) => "link",
            Member::Skip => "skip",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["copy", "skip", "skip", "copy"],
        "the live books copy; the dead rows are no one's to share"
    );
}

#[test]
fn a_link_at_a_shelf_inside_the_tree_points_at_the_copy() {
    let (mut rows, mut shelves) = nested_state();
    rows.push(testkit::link("l2", "Inside door", "s2"));
    rows.push(testkit::link("l3", "Other door", "s3"));
    shelves[0].books.push("l2".into());
    shelves[0].books.push("l3".into());

    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    let fresh_inside = plan.shelves[1].id.clone();
    let targets: Vec<(String, String)> = plan
        .members
        .iter()
        .filter_map(|(old, what)| match what {
            Member::Link(row) => Some((old.clone(), row.target().unwrap().to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(targets.len(), 2, "both links came along, each as its own row");
    assert_eq!(
        targets[0],
        ("l2".to_string(), fresh_inside),
        "a link at a shelf inside the tree points at the copy of that shelf"
    );
    assert_ne!(targets[0].1, "s2", "remapped, not shared");
    assert_eq!(
        targets[1],
        ("l3".to_string(), "s3".to_string()),
        "a link at a shelf outside the tree keeps pointing where it did"
    );
}

#[test]
fn the_landing_remaps_the_tree_onto_fresh_copies() {
    let (mut rows, mut shelves) = nested_state();
    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    let landed = everything_landed(&plan);
    let name = land_tree(&mut rows, &mut shelves, plan, &landed, NOW);

    assert_eq!(name, "Shelf_1");
    assert_eq!(shelves.len(), 5, "the shelf and the one inside it, copied");
    let root = copy_of(&shelves, "Shelf_1");
    assert_eq!(root.books.len(), 1, "one member, and it is not the original's");
    let fresh_id = root.books[0].clone();
    assert_ne!(fresh_id, "b1", "the tree holds its own copy, not the row itself");

    let dup = library_core::book::find_by_id(&rows, &fresh_id).expect("the copy's row");
    match &dup.origin {
        Origin::Stored { src, store } => {
            assert_eq!(
                src.as_deref(),
                Some("/books/b1.md"),
                "read at its place, and the copy is the library's own anyway"
            );
            assert_eq!(
                store,
                &format!("/app/Library/items/{fresh_id}/source.md"),
                "the copy's item folder is its own"
            );
        }
        other => panic!("the duplicate is stored, whatever the original was: {other:?}"),
    }
    assert_eq!(dup.title.as_deref(), Some("b1"), "the name the original showed");
    assert!(dup.title_locked, "a name the reader asked for is not debris");
    assert_eq!(dup.fp, testkit::fp_n(7), "known by its own measurement");
    assert!(!dup.fp_pending, "the measurement the copy rode home with landed");

    let inside = copy_of(&shelves, "Inside");
    assert_eq!(inside.books.len(), 1);
    assert_ne!(inside.books[0], "b2", "the rung's copy is its own row too");
    assert_eq!(
        inside.parent.as_deref(),
        Some(root.id.as_str()),
        "still inside the copy, not the original"
    );
    let level: Vec<&str> = shelf::children_of(&shelves, None)
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(level, vec!["Shelf", "Shelf_1", "Other"], "spliced in behind the original");
}

#[test]
fn a_member_the_store_refused_is_dropped_not_shared() {
    let (mut rows, mut shelves) = nested_state();
    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    let before = rows.len();
    let name = land_tree(&mut rows, &mut shelves, plan, &HashMap::new(), NOW);

    assert_eq!(name, "Shelf_1", "the shelf itself still landed");
    assert_eq!(shelves.len(), 5, "the tree keeps its shape");
    let root = copy_of(&shelves, "Shelf_1");
    assert!(
        root.books.is_empty(),
        "a copy that did not come home is not filed as the original"
    );
    assert_eq!(rows.len(), before, "no row landed for a copy that did not");
}

#[test]
fn a_folder_shelf_duplicates_as_a_shelf_of_the_readers_own() {
    // One directory is one linked shelf: a second folder shelf of one
    // rung would be two doors to one directory with only one of them on
    // the ledger — and a rescan would re-hang a shelf the reader made.
    // The copy is the reader's own second tree over fresh copies of the
    // books.
    let (mut rows, mut shelves) = nested_state();
    shelves[0] = testkit::folder_shelf("s1", "Books", "f1", None, &["b1"], None);
    let plan = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    assert_eq!(plan.name, "Books_1");
    assert!(
        plan.shelves[0].kind.folder_id().is_none(),
        "the copy is not a second shelf of the folder"
    );
    let landed = everything_landed(&plan);
    land_tree(&mut rows, &mut shelves, plan, &landed, NOW);

    let root = copy_of(&shelves, "Books_1");
    assert_ne!(root.books[0], "b1", "the folder's copy holds a copy, not the row");
    // The original is untouched: the copy is not a rung of the folder, so
    // a walk that mints the tree again mints the tree it already had.
    let original = shelf::find(&shelves, "s1").expect("the original stands");
    assert_eq!(original.kind.folder_id(), Some("f1"));
    assert_eq!(original.books, vec!["b1".to_string()]);
}

#[test]
fn a_duplicate_of_a_duplicate_steps_the_shelf_counter() {
    let (mut rows, mut shelves) = nested_state();
    let first = plan_tree(&rows, &shelves, "s1", NOW).expect("the shelf is there");
    assert_eq!(first.name, "Shelf_1");
    let first_id = first.shelves[0].id.clone();
    land_tree(&mut rows, &mut shelves, first, &HashMap::new(), NOW);
    // Duplicating the copy steps rather than stacks, the reading a file
    // manager gives: the counter is not part of the name.
    assert_eq!(
        plan_tree(&rows, &shelves, &first_id, NOW).map(|plan| plan.name),
        Some("Shelf_2".to_string())
    );
}

#[test]
fn a_shelf_that_is_not_there_duplicates_into_nothing() {
    let (rows, shelves) = nested_state();
    assert!(plan_tree(&rows, &shelves, "gone", NOW).is_none());
    // "All" is the book list and not a shelf, so it has no second
    // instance to make either — the pseudo-shelf's own answer everywhere
    // else.
    assert!(plan_tree(&rows, &shelves, library_core::shelf::ALL_SHELF, NOW).is_none());
    assert_eq!(shelves.len(), 3);
}

#[test]
fn a_report_counts_the_kinds_it_landed() {
    let one = |name: &str, shelf| Duplicated { name: name.to_string(), shelf };
    assert_eq!(report(&[one("Dune_1", false)]), "Duplicated as “Dune_1”.");
    assert_eq!(report(&[one("Shelf_1", true)]), "Duplicated as “Shelf_1”.");
    assert_eq!(report(&[one("a", false), one("b", false)]), "Duplicated 2 books.");
    assert_eq!(report(&[one("a", true), one("b", true)]), "Duplicated 2 shelves.");
    assert_eq!(
        report(&[one("a", true), one("b", false), one("c", false)]),
        "Duplicated 3 shelves and books."
    );
}
