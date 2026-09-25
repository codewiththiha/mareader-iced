//! Fixtures the cases across this module share.

use std::collections::{BTreeMap, HashSet};
use crate::folder::{FolderOpts, WatchedFolder};
use crate::shelf::{Shelf, ShelfKind};
use crate::tracking::TrackingTree;

pub(super) fn plain(id: &str, members: &[&str]) -> Shelf {
    crate::testkit::plain_shelf(id, members)
}

pub(super) fn shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
    crate::testkit::shelf(id, name, books, None)
}

pub(super) fn nested(id: &str, name: &str, parent: &str) -> Shelf {
    Shelf {
        parent: Some(parent.to_string()),
        ..shelf(id, name, &[])
    }
}

pub(super) fn ids_of<'a>(shelves: &[&'a Shelf]) -> Vec<&'a str> {
    shelves.iter().map(|s| s.id.as_str()).collect()
}

pub(super) fn ids(members: &[String]) -> Vec<&str> {
    members.iter().map(String::as_str).collect()
}

/// `rel` is the rung it serves, `None` for the folder's root shelf.
pub(super) fn cut(id: &str, folder_id: &str, rel: Option<&str>, parent: Option<&str>) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: id.to_string(),
        kind: ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: rel.map(str::to_string),
        },
        books: Vec::new(),
        parent: parent.map(str::to_string),
        manual_parent: false,
    }
}

/// `/books` read in place, cut into three rungs — root ("r"), "Fiction"
/// ("fic"), "Fiction/SciFi" ("sf") — plus one virtual shelf.
pub(super) fn in_place_tree() -> (Vec<Shelf>, Vec<WatchedFolder>) {
    let shelves = vec![
        cut("r", "f1", None, None),
        cut("fic", "f1", Some("Fiction"), Some("r")),
        cut("sf", "f1", Some("Fiction/SciFi"), Some("fic")),
        plain("mine", &[]),
    ];
    let folders = vec![WatchedFolder {
        id: "f1".into(),
        root: "/books".into(),
        opts: FolderOpts::default(),
        placed: HashSet::new(),
        ignored: Vec::new(),
        last_seen: Vec::new(),
        shelf_map: BTreeMap::from([
            (String::new(), "r".to_string()),
            ("Fiction".to_string(), "fic".to_string()),
            ("Fiction/SciFi".to_string(), "sf".to_string()),
        ]),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
        shapes: crate::shape::ShapeTree::default(),
    }];
    (shelves, folders)
}
