//! The fixtures the cases share: what more than one subject's cases
//! are written against.

use crate::book::{Book, Fingerprint, Origin, Row};
use crate::scan::FoundFile;
use crate::shelf::Shelf;
use super::arrival::Arrival;

pub(super) fn book(id: &str, path: &str) -> Row {
    Row::Book(Book {
        fp: Fingerprint {
            size: 10,
            mtime_ms: 5,
            head_hash: 9,
        },
        added_ms: 10,
        origin: Origin::Linked { src: path.to_string() },
        ..crate::testkit::book(id)
    })
}

pub(super) fn titled(id: &str, path: &str, title: &str) -> Row {
    let mut row = book(id, path);
    row.as_book_mut().unwrap().title = Some(title.to_string());
    row
}

pub(super) fn link(id: &str, name: &str, target: &str) -> Row {
    crate::testkit::link(id, name, target)
}

pub(super) fn shelf(id: &str, members: &[&str]) -> Shelf {
    crate::testkit::plain_shelf(id, members)
}

pub(super) fn plain_shelf(id: &str, name: &str) -> Shelf {
    Shelf {
        name: name.to_string(),
        ..shelf(id, &[])
    }
}

pub(super) fn file(name: &str) -> FoundFile {
    FoundFile {
        rel: name.to_string(),
        path: format!("/books/{name}"),
        ext: "pdf".to_string(),
        size: 10,
        fp: Fingerprint { size: 10, mtime_ms: 5, head_hash: 9 },
    }
}

pub(super) fn import(name: &str, shelf_id: &str) -> Arrival {
    Arrival::import(file(&format!("{name}.pdf")), shelf_id, None)
}

pub(super) fn drag(row_id: &str, name: &str, shelf_id: &str) -> Arrival {
    Arrival::moved(row_id, name, shelf_id, None)
}
