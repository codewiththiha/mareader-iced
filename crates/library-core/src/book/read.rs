//! Where the reader left off, and how a read is written down.
//!
//! A read updates a row rather than inserting into a second list; the old
//! separate "recent books" record drifted from the library it described.

use super::{Book, Fingerprint, Origin, Row, find_by_id};

/// Where the reader is in a book: resume page, page count, and — for a
/// reflowable document read as one stream — the fraction along it. One value
/// because the three always travel together.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadPoint {
    pub page: u32,
    pub num_pages: u32,
    pub fraction: Option<f64>,
}

impl ReadPoint {
    pub fn fresh() -> Self {
        Self {
            page: 1,
            num_pages: 0,
            fraction: None,
        }
    }

    /// The point with an impossible fraction dropped and the page clamped to
    /// the first, so no writer can hand the library a resume point it has to
    /// second-guess later.
    pub fn settled(self) -> Self {
        Self {
            page: self.page.max(1),
            num_pages: self.num_pages,
            fraction: self.fraction.filter(|f| (0.0..=1.0).contains(f)),
        }
    }
}

/// Record a read: every book at `path` moves to `now_ms` with the reader's
/// resume point, or the library gains a linked book when the reader opened
/// something it did not know.
///
/// Every row at the path, because a shelf can hold two rows of one file (a
/// duplicate the reader kept) and the reading position is a fact about the
/// file, not the row: both copies resume together.
pub fn record_read(
    rows: &mut Vec<Row>,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, None, path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref()).map(str::to_string);
    let author = crate::text::non_blank(author.as_deref()).map(str::to_string);
    let book = Book {
        title,
        author,
        last_read_ms: now_ms,
        page: point.page,
        num_pages: point.num_pages,
        fraction: point.fraction,
        fp_pending: true,
        ..Book::new(
            crate::id::next_id(now_ms),
            Fingerprint::placeholder(path),
            reader_core::format::format_of(path),
            Origin::Linked {
                src: path.to_string(),
            },
            now_ms,
        )
    };
    rows.insert(0, Row::Book(book.clone()));
    Some(book)
}

/// Write one read to every row [`rows_for_read`] named, answering whether it
/// wrote anything: a read that found no rows owes the library a new book.
fn write_read(
    rows: &mut [Row],
    at: &[usize],
    title: &Option<String>,
    author: &Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> bool {
    if at.is_empty() {
        return false;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref());
    let author = crate::text::non_blank(author.as_deref());
    for i in at {
        // `rows_for_read` never names a link; the guard keeps that a property
        // of this function rather than of its caller.
        let Some(book) = rows.get_mut(*i).and_then(Row::as_book_mut) else {
            continue;
        };
        book.page = point.page;
        book.num_pages = point.num_pages;
        book.fraction = point.fraction;
        book.last_read_ms = now_ms;
        book.missing = false;
        if crate::text::non_blank(book.title.as_deref()).is_none()
            && let Some(t) = title
        {
            book.title = Some(t.to_string());
        }
        if book.author.is_none() {
            book.author = author.map(str::to_string);
        }
    }
    true
}

/// The rows a reading position belongs to: the named row when it is
/// independent (its resume point is that book's alone), otherwise every shared
/// row at the address.
pub fn rows_for_read(rows: &[Row], book_id: Option<&str>, path: &str) -> Vec<usize> {
    let named = book_id
        .and_then(|id| find_by_id(rows, id))
        .filter(|b| b.path() == path);
    if let Some(book) = named
        && book.independent
    {
        let id = book.id.clone();
        return rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id() == id)
            .map(|(i, _)| i)
            .collect();
    }
    let shared: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path && !b.independent))
        .map(|(i, _)| i)
        .collect();
    if !shared.is_empty() {
        return shared;
    }
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path))
        .map(|(i, _)| i)
        .collect()
}

/// [`record_read`] for an open that knows which row the reader meant — an id
/// is the only thing that tells two rows of one address apart. A shared row
/// answers as [`record_read`] does; an independent one moves alone.
pub fn record_read_row(
    rows: &mut Vec<Row>,
    book_id: &str,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, Some(book_id), path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    record_read(rows, path, title, author, point, now_ms)
}

#[cfg(test)]
mod tests {
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
}
