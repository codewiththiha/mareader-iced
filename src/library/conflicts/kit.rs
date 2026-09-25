//! Fixtures the cases across this module share.

use crate::library::conflicts::{AskKind, ConflictAsk};
use library_core::book::Book;
use library_core::book::Origin;
use library_core::book::Row;
use library_core::conflict::Arrival;
use library_core::folder::WatchedFolder;
use library_core::scan::FoundFile;
use library_core::shelf::Shelf;
use library_core::{paths, testkit};
use reader_core::format::Format;

pub(super) fn linked_row(id: &str, name: &str, path: &str, n: u32) -> Row {
    let mut book = Book::new(
        id.to_string(),
        testkit::fp_n(n),
        Format::Markdown,
        Origin::Linked { src: path.to_string() },
        0,
    );
    book.title = Some(name.to_string());
    book.title_locked = true;
    Row::Book(book)
}

pub(super) fn stored_row(id: &str, name: &str, src: &str, store: &str, n: u32) -> Row {
    let mut book = Book::new(
        id.to_string(),
        testkit::fp_n(n),
        Format::Markdown,
        Origin::Stored { src: Some(src.to_string()), store: store.to_string() },
        0,
    );
    book.title = Some(name.to_string());
    book.title_locked = true;
    Row::Book(book)
}

pub(super) fn plain_shelf(id: &str, books: &[&str]) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: id.to_string(),
        kind: library_core::shelf::ShelfKind::Virtual,
        books: books.iter().map(|b| b.to_string()).collect(),
        parent: None,
        manual_parent: false,
    }
}

pub(super) fn placing_folder(n: u32) -> WatchedFolder {
    WatchedFolder {
        placed: std::collections::HashSet::from([testkit::fp_n(n)]),
        ..testkit::watched_folder("f1", "/books")
    }
}

pub(super) fn ask(moving: Option<&str>, name: &str, to: &str) -> ConflictAsk {
    let arrival = match moving {
        Some(id) => Arrival::moved(id, name, to, None),
        None => Arrival::folder(name, to),
    };
    ConflictAsk {
        arrival,
        existing_id: "e1".to_string(),
        existing_name: name.to_string(),
        kind: AskKind::NameCollision,
    }
}

pub(super) fn found(path: &str, n: u32) -> FoundFile {
    FoundFile {
        path: path.to_string(),
        rel: paths::file_name(path),
        ext: paths::extension(path),
        size: 10,
        fp: testkit::fp_n(n),
    }
}
