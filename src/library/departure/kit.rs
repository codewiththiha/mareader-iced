//! Fixtures the cases across this module share.

use std::collections::{BTreeMap, HashSet};
use library_core::book::Row;
use library_core::folder::WatchedFolder;
use library_core::shelf::Shelf;
use library_core::testkit;

pub(super) fn nested(n: u32) -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([testkit::fp_n(n)]),
        shelf_map: BTreeMap::from([
            (String::new(), "shelf1".to_string()),
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..testkit::watched_folder("f1", "/books")
    }
}

pub(super) fn reading_folder() -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([
            testkit::fp_n(7),
            testkit::fp_n(8),
            testkit::fp_n(9),
            testkit::fp_n(14),
        ]),
        shelf_map: BTreeMap::from([
            (String::new(), "r".to_string()),
            ("Fiction".to_string(), "fic".to_string()),
            ("Fiction/SciFi".to_string(), "sf".to_string()),
        ]),
        ..testkit::watched_folder("f1", "/books")
    }
}

pub(super) fn tree() -> Vec<Shelf> {
    vec![
        testkit::folder_shelf("r", "r", "f1", None, &["top", "shown2"], None),
        testkit::folder_shelf("fic", "fic", "f1", Some("Fiction"), &["mid"], Some("r")),
        testkit::folder_shelf("sf", "sf", "f1", Some("Fiction/SciFi"), &["deep", "shown2", "loose", "kept"], Some("fic")),
        testkit::shelf("mine", "Mine", &[], Some("fic")),
        testkit::shelf("elsewhere", "Elsewhere", &[], None),
    ]
}

pub(super) fn tree_rows() -> Vec<Row> {
    vec![
        testkit::row_at_n("top", "/books/top.md", 9),
        testkit::row_at_n("mid", "/books/Fiction/other.md", 8),
        testkit::row_at_n("deep", "/books/Fiction/SciFi/dune.md", 7),
        testkit::row_at_n("shown2", "/books/top2.md", 14),
        testkit::row_at_n("loose", "/loose/x.md", 12),
        testkit::stored_row("kept", "/books/Fiction/SciFi/old.md", "/store/kept.md", 13),
    ]
}

/// f1's tree with a displaced member: the shape a removed rung and a
/// subfolder imported on its own leave behind.
pub(super) fn family_state() -> (Vec<Shelf>, Vec<WatchedFolder>) {
    let mut tree = reading_folder();
    tree.shelf_map.remove("Fiction/SciFi");
    let mut member = reading_folder();
    member.id = "f3".into();
    member.root = "/books/Fiction/SciFi".into();
    member.shelf_map = BTreeMap::from([(String::new(), "s3".to_string())]);
    let shelves = vec![
        testkit::folder_shelf("r", "r", "f1", None, &[], None),
        testkit::folder_shelf("fic", "fic", "f1", Some("Fiction"), &[], Some("r")),
        testkit::folder_shelf("s3", "s3", "f3", None, &["deep"], None),
        testkit::shelf("mine", "Mine", &[], None),
    ];
    (shelves, vec![tree, member])
}
