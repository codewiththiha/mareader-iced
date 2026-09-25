//! Row lookups: by id and by address.

use super::{Book, Row, book_rows, book_rows_mut};

/// The first row at the address — every shared row there agrees, so the first
/// is the answer. A caller that knows which row the reader means wants
/// [`find_by_id`] instead.
pub fn find_by_path<'a>(rows: &'a [Row], path: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.path() == path)
}

/// The lookup every row-addressed caller should make: an id survives a
/// relink, a rename and a move between shelves.
pub fn find_by_id<'a>(rows: &'a [Row], id: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.id == id)
}

/// Steps over links like [`book_rows_mut`], so a link can never be written
/// through as though it were a book.
pub fn find_book_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Book> {
    book_rows_mut(rows).find(|b| b.id == id)
}

/// Id → row in one pass, so resolving a level's members is a lookup rather
/// than a walk of the library per member. First wins if an id appears twice.
pub fn index_by_id(rows: &[Row]) -> std::collections::HashMap<&str, &Row> {
    let mut index = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        index.entry(row.id()).or_insert(row);
    }
    index
}

/// The resume page and fractional stream position (see [`Book::fraction`]),
/// clamped like [`ReadPoint::settled`] clamps a write.
///
/// `book_id` decides when the address holds more than one row; with no id — a
/// drop, an open-with, a dialog — the first row at the path answers.
pub fn resume_point(rows: &[Row], book_id: Option<&str>, path: &str) -> (u32, Option<f64>) {
    let book = book_id
        .and_then(|id| find_by_id(rows, id))
        .filter(|b| b.path() == path)
        .or_else(|| find_by_path(rows, path));
    match book {
        Some(b) => (
            b.page.max(1),
            b.fraction.filter(|f| (0.0..=1.0).contains(f)),
        ),
        None => (1, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::kit::{linked, private, rows};
    use crate::book::query::find_by_path;
    use crate::book::query::resume_point;

    #[test]
    fn the_resume_point_follows_the_row_the_reader_named() {
        let mut shared = linked("a", "/books/dune.pdf");
        shared.page = 12;
        let mut own = private("b", "/books/dune.pdf");
        own.page = 240;
        own.fraction = Some(0.5);
        let books = rows([shared, own]);
        assert_eq!(resume_point(&books, Some("b"), "/books/dune.pdf"), (240, Some(0.5)));
        assert_eq!(resume_point(&books, Some("a"), "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, None, "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, Some("zzz"), "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, None, "/books/nope.pdf"), (1, None));
    }

    #[test]
    fn the_resume_point_is_looked_up_by_address() {
        let books = rows([Book {
            page: 42,
            num_pages: 100,
            fraction: Some(0.5),
            ..linked("a", "/books/one.pdf")
        }]);
        assert_eq!(resume_point(&books, None, "/books/one.pdf"), (42, Some(0.5)));
        assert_eq!(resume_point(&books, None, "/books/zzz.pdf"), (1, None));
        assert_eq!(find_by_path(&books, "/books/one.pdf").map(|b| b.id.as_str()), Some("a"));
    }
}
