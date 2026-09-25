//! Fixtures the cases across this module share.

use crate::book::{Book, Fingerprint, Origin, Row};
use reader_core::format::Format;

pub(super) fn rows(books: impl IntoIterator<Item = Book>) -> Vec<Row> {
    books.into_iter().map(Row::Book).collect()
}

pub(super) fn at(rows: &[Row], i: usize) -> &Book {
    rows[i].book().expect("a book row")
}

pub(super) fn fp(size: u64, mtime: u64, head: u32) -> Fingerprint {
    Fingerprint {
        size,
        mtime_ms: mtime,
        head_hash: head,
    }
}

pub(super) fn linked(id: &str, path: &str) -> Book {
    Book {
        id: id.to_string(),
        fp: fp(10, 1, 7),
        title: None,
        author: None,
        format: Format::Pdf,
        origin: Origin::Linked {
            src: path.to_string(),
        },
        added_ms: 1,
        last_read_ms: 1,
        page: 1,
        num_pages: 0,
        fraction: None,
        missing: false,
        fp_pending: false,
        independent: false,
        title_locked: false,
    }
}

pub(super) fn private(id: &str, path: &str) -> Book {
    Book {
        independent: true,
        ..linked(id, path)
    }
}

pub(super) fn check(path: &str, exists: bool, size: u64, mtime: u64, head: u32) -> crate::wire::PathCheck {
    crate::wire::PathCheck {
        path: path.to_string(),
        exists,
        size,
        mtime_ms: mtime,
        head_hash: head,
    }
}
