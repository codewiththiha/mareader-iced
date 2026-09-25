//! Coverage's own cases.

use super::*;
use crate::folder::{FolderMode, FolderOpts};
use crate::tracking::TrackingTree;
use crate::testkit::{departed_shelf, folder_shelf};
use std::collections::{BTreeMap, HashSet};

fn folder(id: &str, root: &str, in_place: bool, map: &[(&str, &str)]) -> WatchedFolder {
    WatchedFolder {
        id: id.into(),
        root: root.into(),
        opts: FolderOpts {
            in_place,
            ..FolderOpts::default()
        },
        placed: HashSet::new(),
        ignored: Vec::new(),
        last_seen: Vec::new(),
        shelf_map: map
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
        shapes: crate::shape::ShapeTree::default(),
    }
}

fn tree() -> (Vec<WatchedFolder>, Vec<Shelf>) {
    let folders = vec![folder(
        "f1",
        "/books",
        true,
        &[("", "r"), ("Fiction", "fic"), ("Fiction/SciFi", "sf")],
    )];
    let shelves = vec![
        folder_shelf("r", "Books", "f1", None, &[], None),
        folder_shelf("fic", "Fiction", "f1", Some("Fiction"), &[], Some("r")),
        folder_shelf("sf", "SciFi", "f1", Some("Fiction/SciFi"), &[], Some("fic")),
        crate::testkit::plain_shelf("mine", &[]),
    ];
    (folders, shelves)
}

#[test]
fn a_tree_covers_its_own_root_and_a_rung_inside_it() {
    let (folders, shelves) = tree();
    let g = Governance::new(&folders, &shelves);
    let root = g.covering("/books").expect("the root shelf stands");
    assert_eq!(root.folder_id, "f1");
    assert_eq!(root.rel, "");
    assert_eq!(root.shelf_id, "r");
    let rung = g.covering("/books/Fiction/SciFi").expect("the rung stands");
    assert_eq!(rung.rel, "Fiction/SciFi");
    assert_eq!(rung.shelf_id, "sf");
    assert_eq!(g.covering("/books/Unmapped"), None);
    assert_eq!(g.covering("/other"), None);
    assert_eq!(g.covering("/bookshelf"), None, "a prefix is not a directory");
}

#[test]
fn the_empty_rung_outranks_a_deeper_one() {
    // Two in-place trees, one nested in the other: a pick of the outer
    // root is the outer tree's own door.
    let folders = vec![
        folder("outer", "/books", true, &[("", "r")]),
        folder("inner", "/books/Fiction", true, &[("", "fic")]),
    ];
    let shelves = vec![
        folder_shelf("r", "Books", "outer", None, &[], None),
        folder_shelf("fic", "Fiction", "inner", None, &[], None),
    ];
    let g = Governance::new(&folders, &shelves);
    assert_eq!(g.covering("/books").map(|c| c.folder_id).as_deref(), Some("outer"));
    assert_eq!(g.covering("/books/Fiction").map(|c| c.folder_id).as_deref(), Some("inner"));
}

#[test]
fn a_copying_tree_and_a_dead_rung_cover_nothing() {
    let copying = vec![folder("c1", "/books", false, &[("", "r")])];
    let shelves = vec![folder_shelf("r", "Books", "c1", None, &[], None)];
    assert_eq!(Governance::new(&copying, &shelves).covering("/books"), None);
    let (folders, mut dead) = tree();
    dead.retain(|s| s.id != "sf");
    assert_eq!(Governance::new(&folders, &dead).covering("/books/Fiction/SciFi"), None);
    assert!(Governance::new(&folders, &dead).covering("/books").is_some());
}

#[test]
fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
    let (folders, shelves) = tree();
    let g = Governance::new(&folders, &shelves);
    assert_eq!(
        g.family("/books/Fiction/Deleted"),
        Some(("f1".to_string(), "Fiction/Deleted".to_string()))
    );
    assert_eq!(g.family("/books/Fiction/SciFi"), None);
    assert_eq!(g.family("/books"), None);
    let mut dead = folders.clone();
    dead[0].shelf_map.insert("Fiction/SciFi".into(), "gone".into());
    assert_eq!(
        Governance::new(&dead, &shelves).family("/books/Fiction/SciFi"),
        Some(("f1".to_string(), "Fiction/SciFi".to_string()))
    );
}

#[test]
fn of_two_nested_trees_the_longest_relative_rung_answers() {
    // The tie-break is the longest `rel`: the tree whose root sits
    // highest. Also asserted in shelf/mod.rs's family test.
    let outer = folder("f1", "/books", true, &[("", "or"), ("Fiction", "fic")]);
    let inner = folder("f2", "/books/Fiction", true, &[("", "ir")]);
    let shelves = vec![
        folder_shelf("or", "Books", "f1", None, &[], None),
        folder_shelf("ir", "Fiction", "f2", None, &[], None),
    ];
    let both = vec![outer, inner];
    let g = Governance::new(&both, &shelves);
    assert_eq!(
        g.family("/books/Fiction/SciFi"),
        Some(("f1".to_string(), "Fiction/SciFi".to_string())),
        "the outermost tree's rel is the longest, so it answers"
    );
}

#[test]
fn a_watch_dot_is_the_rung_s_answer_not_the_tree_s() {
    let (folders, shelves) = tree();
    let g = Governance::new(&folders, &shelves);
    assert!(!g.shelf_tracked("r"));
    assert!(!g.shelf_tracked("sf"));

    let mut all = folders.clone();
    all[0].set_tracking("", true);
    let g = Governance::new(&all, &shelves);
    assert!(g.shelf_tracked("r"));
    assert!(g.shelf_tracked("fic"));
    assert!(g.shelf_tracked("sf"), "a rung inherits the root");

    // Turning one rung off is the case the flag could not express.
    let mut partly = all.clone();
    partly[0].set_tracking("Fiction", false);
    let g = Governance::new(&partly, &shelves);
    assert!(g.shelf_tracked("r"));
    assert!(!g.shelf_tracked("fic"), "the rung turned off");
    assert!(!g.shelf_tracked("sf"), "and everything below it");
    assert!(partly[0].tracked());
    assert!(partly[0].opts.watch);
}

#[test]
fn a_shelf_the_reader_made_answers_for_the_tree_it_stands_inside() {
    let (folders, mut shelves) = tree();
    // A shelf inside the Fiction rung is not a rung the disk names, but
    // the tree's answer for that rung is its answer.
    shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("fic")));
    let mut tracked = folders.clone();
    tracked[0].set_tracking("", true);
    let g = Governance::new(&tracked, &shelves);
    assert!(g.shelf_tracked("mine2"));
    let mut off = tracked.clone();
    off[0].set_tracking("Fiction", false);
    assert!(!Governance::new(&off, &shelves).shelf_tracked("mine2"));
}

#[test]
fn a_shelf_a_move_took_off_its_tree_answers_no_tree() {
    let (folders, mut shelves) = tree();
    // SciFi was dragged out of Fiction, the copy paid, and filed inside
    // the rung it left: nothing answers for it any more.
    shelves.push(departed_shelf("moved", &[], Some("fic")));
    let mut tracked = folders.clone();
    tracked[0].set_tracking("", true);
    let g = Governance::new(&tracked, &shelves);
    assert_eq!(g.tree_of("moved"), None);
    assert_eq!(g.seat_of("moved"), None);
    assert!(!g.shelf_tracked("moved"), "no ground, no dot");
    assert_eq!(g.mode_of("moved"), Some(FolderMode::Copy));
    assert_eq!(g.tree_of("fic").map(|seat| seat.rung), Some("Fiction".to_string()));
}

#[test]
fn a_shelf_inside_a_departed_one_stands_on_the_reader_s_own() {
    let (folders, mut shelves) = tree();
    shelves.push(departed_shelf("moved", &[], Some("fic")));
    shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("moved")));
    let g = Governance::new(&folders, &shelves);
    assert_eq!(g.tree_of("mine2"), None, "the ground ends with the shelf that left it");
}

#[test]
fn a_copying_tree_gives_the_badge_but_no_seat() {
    let copying = vec![folder("c1", "/books", false, &[("", "r")])];
    let shelves = vec![folder_shelf("r", "Books", "c1", None, &[], None)];
    let g = Governance::new(&copying, &shelves);
    assert_eq!(g.mode_of("r"), Some(FolderMode::Copy), "the library keeps its own");
    assert_eq!(g.seat_of("r"), None, "and there is no tracking to promise");
    assert_eq!(g.tree_of("r").map(|seat| seat.rung), Some(String::new()));
}

#[test]
fn a_shelf_nothing_reads_in_place_has_no_dot_to_draw() {
    let (folders, shelves) = tree();
    let g = Governance::new(&folders, &shelves);
    assert!(!g.shelf_tracked("mine"));
    assert!(!g.shelf_tracked("gone"));
    assert!(!g.shelf_tracked(crate::shelf::ALL_SHELF));
    // A copying folder's shelf has no watch either way: the import sheet
    // does not offer one beside a copy.
    let copying = vec![folder("c1", "/dvds", false, &[("", "x")])];
    let copy_shelves = vec![folder_shelf("x", "DVDs", "c1", None, &[], None)];
    assert!(!Governance::new(&copying, &copy_shelves).shelf_tracked("x"));
}

#[test]
fn the_seat_a_shelf_stands_on_is_the_rung_a_toggle_writes() {
    let (folders, mut shelves) = tree();
    shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("fic")));
    let g = Governance::new(&folders, &shelves);
    assert_eq!(
        g.seat_of("r"),
        Some(Seat { folder_id: "f1".into(), rung: "".into() })
    );
    assert_eq!(
        g.seat_of("sf"),
        Some(Seat { folder_id: "f1".into(), rung: "Fiction/SciFi".into() })
    );
    assert_eq!(
        g.seat_of("mine2"),
        Some(Seat { folder_id: "f1".into(), rung: "Fiction".into() })
    );
    assert_eq!(g.seat_of("mine"), None);
    assert_eq!(g.seat_of("gone"), None);
    let copying = vec![folder("c1", "/dvds", false, &[("", "x")])];
    let copy_shelves = vec![folder_shelf("x", "DVDs", "c1", None, &[], None)];
    assert_eq!(Governance::new(&copying, &copy_shelves).seat_of("x"), None);
}
