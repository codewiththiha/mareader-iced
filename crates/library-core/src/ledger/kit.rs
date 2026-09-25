//! Fixtures the cases across this module share.

use std::collections::{BTreeMap, HashSet};
use crate::book::{Book, Fingerprint, Origin, Row};
use crate::folder::{FolderOpts, Tombstone, WatchedFolder};
use crate::ledger::registry::{KnownBook, Registry};
use crate::scan::FoundFile;
use crate::tracking::TrackingTree;

pub(super) fn fp(n: u32) -> Fingerprint {
    crate::testkit::fp_n(n)
}

pub(super) fn file(n: u32, path: &str) -> FoundFile {
    FoundFile {
        path: path.to_string(),
        rel: path.trim_start_matches("/books/").to_string(),
        ext: "pdf".into(),
        size: u64::from(n),
        fp: fp(n),
    }
}

pub(super) fn folder(placed: &[u32], ignored: &[u32]) -> WatchedFolder {
    WatchedFolder {
        id: "f1".into(),
        root: "/books".into(),
        opts: FolderOpts::default(),
        placed: placed.iter().copied().map(fp).collect::<HashSet<_>>(),
        ignored: ignored.iter().copied().map(stone).collect(),
        shelf_map: BTreeMap::new(),
        last_seen: Vec::new(),
        scanned_ms: 0,
        // The ledger's tables answer for a folder the walk is on: a tree
        // that tracks from its root.
        tracking: TrackingTree::tracking_root(),
        shapes: crate::shape::ShapeTree::default(),
    }
}

pub(super) fn stone(n: u32) -> Tombstone {
    Tombstone {
        fp: fp(n),
        title: Some(format!("Book {n}")),
        format: reader_core::format::Format::Pdf,
        last_path: format!("/books/{n}.pdf"),
        shelf_id: None,
        removed_ms: 5,
        moved: false,
        returned_row: None,
    }
}

pub(super) fn registry(rows: &[(u32, &str, &str, bool)]) -> Registry {
    rows.iter()
        .map(|(n, id, path, missing)| {
            (
                fp(*n),
                KnownBook {
                    id: (*id).to_string(),
                    path: (*path).to_string(),
                    missing: *missing,
                    source: None,
                },
            )
        })
        .collect()
}

pub(super) fn book(id: &str, origin: Origin, missing: bool) -> Row {
    Row::Book(book_value(id, origin, missing))
}

pub(super) fn book_value(id: &str, origin: Origin, missing: bool) -> Book {
    Book {
        fp: fp(1),
        title: Some("Dune".into()),
        origin,
        added_ms: 1,
        last_read_ms: 5,
        page: 42,
        num_pages: 400,
        missing,
        ..crate::testkit::book(id)
    }
}

pub(super) fn at(rows: &[Row], i: usize) -> &Book {
    rows[i].book().expect("a book row")
}
