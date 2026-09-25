//! The read points' own cases.

use super::*;
use crate::book::book_rows;
use crate::book::check::apply_check;
use crate::book::kit::{at, check, fp, linked, private, rows};
use crate::book::read::ReadPoint;
use crate::book::read::record_read;
use crate::book::read::record_read_row;
use crate::book::read::rows_for_read;
use reader_core::format::Format;

#[test]
fn an_author_fills_a_gap_and_never_overwrites_one() {
    let mut books = rows([linked("a", "/books/one.pdf")]);
    record_read(
        &mut books,
        "/books/one.pdf",
        None,
        Some("Frank Herbert".into()),
        ReadPoint::fresh(),
        1,
    );
    assert_eq!(at(&books, 0).author.as_deref(), Some("Frank Herbert"));
    record_read(
        &mut books,
        "/books/one.pdf",
        None,
        Some("Somebody Else".into()),
        ReadPoint::fresh(),
        2,
    );
    assert_eq!(
        at(&books, 0).author.as_deref(),
        Some("Frank Herbert"),
        "the first author the document gave is the one the shelf keeps"
    );
    let mut books = rows([linked("b", "/books/two.pdf")]);
    record_read(
        &mut books,
        "/books/two.pdf",
        None,
        Some("   ".into()),
        ReadPoint::fresh(),
        1,
    );
    assert_eq!(at(&books, 0).author, None);
}

#[test]
fn reading_a_known_book_updates_it_in_place() {
    let mut books = rows([linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")]);
    let created = record_read(
        &mut books,
        "/books/two.pdf",
        Some("Two".into()),
        None,
        ReadPoint { page: 42, num_pages: 100, fraction: None },
        500,
    );
    assert!(created.is_none(), "an existing book is not created again");
    assert_eq!(books.len(), 2);
    assert_eq!(at(&books, 0).id, "a");
    assert_eq!(at(&books, 1).page, 42);
    assert_eq!(at(&books, 1).last_read_ms, 500);
    assert_eq!(at(&books, 1).title.as_deref(), Some("Two"));
    assert_eq!(at(&books, 1).fp, fp(10, 1, 7));
    assert!(!at(&books, 1).fp_pending);
}

#[test]
fn reading_an_unknown_book_creates_a_linked_one_at_the_front() {
    let mut books = rows([linked("a", "/books/one.pdf")]);
    let created = record_read(
        &mut books,
        "/books/new.md",
        None,
        None,
        ReadPoint::fresh(),
        600,
    )
    .expect("a new book");
    assert_eq!(books.len(), 2);
    assert_eq!(at(&books, 0).id, created.id);
    assert_eq!(created.format, Format::Markdown);
    assert_eq!(created.path(), "/books/new.md");
    assert!(!created.origin.is_stored());
    // Opening proved the file exists and nothing more; the next path check measures it.
    assert!(created.fp_pending);
    assert_eq!(created.fp, Fingerprint::placeholder("/books/new.md"));
}

#[test]
fn a_title_only_ever_fills_a_gap() {
    let mut books = rows([Book {
        title: Some("Named by the document".into()),
        ..linked("a", "/books/one.pdf")
    }]);
    record_read(
        &mut books,
        "/books/one.pdf",
        Some("one".into()),
        None,
        ReadPoint { page: 3, num_pages: 10, fraction: None },
        10,
    );
    assert_eq!(at(&books, 0).title.as_deref(), Some("Named by the document"));
}

#[test]
fn a_resume_point_is_settled_before_it_is_written() {
    // The reader hands over whatever the document said; the library keeps it valid.
    let mut books = rows([linked("a", "/books/one.pdf")]);
    record_read(
        &mut books,
        "/books/one.pdf",
        None,
        None,
        ReadPoint { page: 0, num_pages: 10, fraction: Some(1.4) },
        1,
    );
    assert_eq!(at(&books, 0).page, 1);
    assert_eq!(at(&books, 0).fraction, None);
    assert_eq!(ReadPoint::fresh(), ReadPoint { page: 1, num_pages: 0, fraction: None });
}

#[test]
fn a_shared_read_leaves_a_private_book_where_it_was() {
    // Two rows of one file share their position — unless one is independent.
    let mut books = rows([linked("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
    at_mut(&mut books, 1).page = 240;
    record_read(
        &mut books,
        "/books/dune.pdf",
        Some("Dune".into()),
        None,
        ReadPoint { page: 90, num_pages: 400, fraction: None },
        700,
    );
    assert_eq!(at(&books, 0).page, 90, "the shared row moves");
    assert_eq!(at(&books, 1).page, 240, "and the private one does not");
    assert_eq!(at(&books, 0).last_read_ms, 700);
    assert_eq!(at(&books, 1).last_read_ms, 1, "its stamp is its own too");
    // A path check is the address's fate, so it writes every row.
    assert_eq!(
        apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
        2
    );
    assert!(book_rows(&books).all(|b| !b.missing && b.fp == fp(20, 2, 8)));
}

#[test]
fn an_open_that_names_no_row_treats_the_address_as_one_book() {
    // An open that cannot ask which row was meant falls back to the address.
    let mut books = rows([private("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
    assert_eq!(rows_for_read(&books, None, "/books/dune.pdf"), vec![0, 1]);
    assert!(record_read(
        &mut books,
        "/books/dune.pdf",
        Some("Dune".into()),
        None,
        ReadPoint { page: 12, num_pages: 400, fraction: None },
        5,
    )
    .is_none());
    assert_eq!(books.len(), 2, "nothing was minted for a file already held");
    assert!(book_rows(&books).all(|b| b.page == 12));
    assert_eq!(rows_for_read(&books, Some("b"), "/books/dune.pdf"), vec![1]);
}

#[test]
fn the_rows_a_read_belongs_to_are_the_rows_it_writes() {
    let books = rows([
        linked("a", "/books/dune.pdf"),
        private("b", "/books/dune.pdf"),
        linked("c", "/books/dune.pdf"),
        linked("d", "/books/other.pdf"),
    ]);
    assert_eq!(rows_for_read(&books, None, "/books/dune.pdf"), vec![0, 2]);
    assert_eq!(rows_for_read(&books, Some("c"), "/books/dune.pdf"), vec![0, 2]);
    assert_eq!(rows_for_read(&books, Some("b"), "/books/dune.pdf"), vec![1]);
    assert_eq!(rows_for_read(&books, Some("d"), "/books/dune.pdf"), vec![0, 2]);
    assert_eq!(rows_for_read(&books, Some("zzz"), "/books/dune.pdf"), vec![0, 2]);
    assert!(rows_for_read(&books, None, "/books/nothing.pdf").is_empty());
}

#[test]
fn a_read_that_names_its_row_writes_that_row() {
    let mut books = rows([linked("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
    let point = ReadPoint { page: 240, num_pages: 400, fraction: None };
    assert!(record_read_row(&mut books, "a", "/books/dune.pdf", None, None, point, 9).is_none());
    assert_eq!(at(&books, 0).page, 240);
    assert_eq!(at(&books, 1).page, 1, "the private row is not a twin of it");
    let further = ReadPoint { page: 380, num_pages: 400, fraction: None };
    assert!(record_read_row(&mut books, "b", "/books/dune.pdf", None, None, further, 11).is_none());
    assert_eq!(at(&books, 1).page, 380);
    assert_eq!(at(&books, 1).last_read_ms, 11);
    assert_eq!(at(&books, 0).page, 240, "the shared row keeps the read it was given");
    assert!(record_read_row(&mut books, "zzz", "/books/dune.pdf", None, None, point, 12).is_none());
    assert_eq!(at(&books, 0).page, 240);
    assert_eq!(books.len(), 2);
    let created = record_read_row(
        &mut books,
        "zzz",
        "/books/other.pdf",
        Some("Other".into()),
        None,
        ReadPoint::fresh(),
        13,
    );
    assert_eq!(created.and_then(|b| b.title).as_deref(), Some("Other"));
    assert_eq!(books.len(), 3);
}

fn at_mut(rows: &mut [Row], i: usize) -> &mut Book {
    rows[i].as_book_mut().expect("a book row")
}
