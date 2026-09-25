//! What the folder's last run knew: the books it recorded, indexed by the
//! fingerprint a scan compares against, and the lookups the diff asks of it.

use std::collections::HashMap;

use crate::book::{book_rows, Book, Fingerprint, Origin, Row};

/// What the ledger needs to know about a book already in the library: id,
/// address, whether the address is known dead, and — for a stored copy — the
/// address it was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBook {
    pub id: String,
    pub path: String,
    pub missing: bool,
    /// `None` for a linked row: its source is its own address, and the rule
    /// this feeds is about a copy and its original.
    pub source: Option<String>,
}

/// Derived from the book list on every scan rather than persisted; a derived
/// index cannot go stale.
pub type Registry = HashMap<Fingerprint, KnownBook>;

/// The [`Registry`] for a row list, built from its book rows: a link has no
/// fingerprint, so a scan never sees or places one. Two rows can share a
/// fingerprint (duplicates the reader kept); shared rows are registered
/// before independent ones, so a scan always resolves to the shared copy.
pub fn registry_of(rows: &[Row]) -> Registry {
    let mut out = Registry::with_capacity(rows.len());
    let books: Vec<&Book> = book_rows(rows).collect();
    for book in books.iter().filter(|b| !b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    for book in books.iter().filter(|b| b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    out
}

/// The row the library already holds for this content, if any: "are these
/// bytes already a book somewhere", not "is this name on the level I am
/// dropping onto" (that is [`crate::conflict::collide`]).
pub fn existing_for(rows: &[Row], fp: Fingerprint) -> Option<ExistingContent> {
    if fp.mtime_ms == 0 {
        return None;
    }
    // Bound rather than chained: the registry is a temporary, and borrowing
    // out of one in the same expression drops it.
    let registry = registry_of(rows);
    let known = registry.get(&fp)?;
    if known.path.is_empty() {
        return None;
    }
    Some(ExistingContent {
        row_id: known.id.clone(),
        missing: known.missing,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingContent {
    pub row_id: String,
    pub missing: bool,
}

fn known_of(book: &crate::book::Book) -> KnownBook {
    KnownBook {
        id: book.id.clone(),
        path: book.path().to_string(),
        missing: book.missing,
        // A stored row's provenance only: a link's source IS its address, so
        // carrying it would make every linked book look like a copy of itself.
        source: match &book.origin {
            Origin::Stored { src, .. } => src.clone(),
            Origin::Linked { .. } => None,
        },
    }
}

/// Borrowed rather than cloned: the menu asking this opens on a click, and
/// copying a whole library to answer one question is a frame drop. Duplicates
/// resolve to the first row.
pub fn index_by_fp(rows: &[Row]) -> HashMap<Fingerprint, &Book> {
    let mut out = HashMap::with_capacity(rows.len());
    for book in book_rows(rows) {
        out.entry(book.fp).or_insert(book);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::kit::{at, book, book_value, file, fp};
    use crate::book::{Book, Fingerprint, Origin, Row};
    use crate::ledger::recover::relink;
    use crate::ledger::scan::{copy_over_paths, unbound_copies};
    use reader_core::format::Format;

    #[test]
    fn content_the_library_holds_is_a_question_before_it_is_a_second_copy() {
        // A folder walk has always asked through the registry; a loose file
        // dropped on the library never did.
        let rows = vec![crate::testkit::row_at("b1", "/books/dune.pdf")];
        let held = existing_for(&rows, fp(1)).expect("the library holds this content");
        assert_eq!(held.row_id, "b1");
        assert!(!held.missing);
        assert_eq!(existing_for(&rows, fp(2)), None);
        // A link has no fingerprint, so it is never an answer.
        let with_link = vec![
            crate::testkit::row_at("b1", "/books/dune.pdf"),
            crate::testkit::link("l1", "Dune", "b1"),
        ];
        assert_eq!(
            existing_for(&with_link, fp(1)).map(|e| e.row_id).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn an_unmeasured_fingerprint_matches_nothing() {
        // A placeholder carries `mtime_ms == 0`: matching a real measurement
        // against one would either miss every book or claim one it does not.
        let pending = vec![Row::Book(Book {
            fp: Fingerprint::placeholder("/books/dune.pdf"),
            fp_pending: true,
            ..crate::testkit::book("b1")
        })];
        assert_eq!(existing_for(&pending, Fingerprint::placeholder("/books/dune.pdf")), None);
        assert_eq!(existing_for(&pending, fp(1)), None);
        // A measured row answers: the guard is about the fingerprint, not the row.
        let measured = vec![crate::testkit::row_at("b1", "/books/dune.pdf")];
        assert!(existing_for(&measured, fp(1)).is_some());
    }

    #[test]
    fn a_book_whose_address_died_is_still_the_book_the_library_holds() {
        // `missing` rides the answer because it changes what the answer
        // means: an offer to open, or an offer to find the file again.
        let gone = vec![Row::Book(Book {
            missing: true,
            ..crate::testkit::book_at("b1", "/books/dune.pdf")
        })];
        let held = existing_for(&gone, fp(1)).expect("the content is still held");
        assert_eq!(held.row_id, "b1");
        assert!(held.missing);
    }

    #[test]
    fn a_duplicate_the_reader_kept_is_still_one_content() {
        // Two honest rows of one file share a fingerprint; the answer names a
        // real row.
        let twins = vec![
            crate::testkit::row_at("b1", "/books/dune.pdf"),
            Row::Book(Book {
                independent: true,
                ..crate::testkit::book_at("b2", "/books/dune.pdf")
            }),
        ];
        let held = existing_for(&twins, fp(1)).expect("one content");
        assert_eq!(held.row_id, "b1", "the shared row answers, not the private one");
    }

    #[test]
    fn the_registry_carries_a_copy_provenance() {
        let rows = vec![Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Stored {
                src: Some("/books/a.pdf".into()),
                store: "/store/b1.pdf".into(),
            },
            0,
        ))];
        let known = registry_of(&rows).get(&fp(1)).cloned().expect("a row");
        assert_eq!(known.path, "/store/b1.pdf", "the address is the store's");
        assert_eq!(known.source.as_deref(), Some("/books/a.pdf"), "and the source is the file's");
        let rows = vec![Row::Book(Book::new(
            "b2".into(),
            fp(2),
            Format::Markdown,
            Origin::Stored {
                src: None,
                store: "/store/b2.pdf".into(),
            },
            0,
        ))];
        assert_eq!(registry_of(&rows).get(&fp(2)).and_then(|k| k.source.clone()), None);
    }

    #[test]
    fn a_copies_run_copies_over_every_file_the_library_reads_in_place() {
        let linked = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Linked { src: path.into() },
                0,
            ))
        };
        let stored = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Stored {
                    src: Some(path.into()),
                    store: format!("/store/{id}.md"),
                },
                0,
            ))
        };
        let rows = vec![
            linked("b1", "/one/a.md", 1),   // read in place
            stored("b2", "/one/b.md", 2),   // already the library's own copy
            linked("b3", "/one/c.md", 3),   // read in place by ANOTHER folder's tree
        ];
        let found = vec![
            file(1, "/one/a.md"),
            file(2, "/one/b.md"),
            file(3, "/one/c.md"),
            file(4, "/one/d.md"), // content the library does not hold
        ];
        let registry = registry_of(&rows);
        let paths = copy_over_paths(&found, &registry, &rows);
        let mut paths = paths.into_iter().collect::<Vec<_>>();
        paths.sort();
        assert_eq!(
            paths,
            vec!["/one/a.md".to_string(), "/one/c.md".to_string()],
            "which folder placed the linked row is nobody's question: a copies run              owes a book of its own for every file the library reads in place. A              stored row is already a copy, and a file the library does not hold is              an ordinary add."
        );
    }

    /// The unbound copies run's table: what the bound explicit run owes,
    /// minus everything a ledger would have answered.
    #[test]
    fn an_unbound_copies_run_owes_every_file_but_the_copy_the_library_made() {
        let linked = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Linked { src: path.into() },
                0,
            ))
        };
        let stored = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Stored {
                    src: Some(path.into()),
                    store: format!("/store/{id}.md"),
                },
                0,
            ))
        };
        let rows = vec![
            linked("b1", "/one/a.md", 1), // a tree reads in place: owed a copy
            stored("b2", "/one/b.md", 2), // the library's own copy of this very file
            linked("b3", "/one/c.md", 3), // read in place: owed a copy
            Row::Book(Book {
                missing: true,
                ..Book::new(
                    "b9".into(),
                    fp(9),
                    Format::Markdown,
                    Origin::Linked { src: "/gone/x.md".into() },
                    0,
                )
            }),
        ];
        let found = vec![
            file(1, "/one/a.md"),
            file(2, "/one/b.md"),
            file(3, "/one/c.md"),
            file(4, "/one/d.md"), // content nobody holds: an ordinary add
            file(1, "/one/a-copy.md"), // a second file of the first one's bytes
            file(9, "/one/x.md"), // a missing book's content, at a new address
        ];
        let registry = registry_of(&rows);
        let owed = unbound_copies(&found, &registry, &rows);
        let paths: Vec<&str> = owed.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["/one/a.md", "/one/c.md", "/one/d.md"],
            "the copy the library already made is not owed a second, one book \
             per fingerprint inside the walk, and a missing book's heal is the \
             walk that owns the row's rather than this run's"
        );
    }

    #[test]
    fn a_namesake_at_another_address_is_not_a_second_instance() {
        let rows = vec![Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Linked {
                src: "/elsewhere/a.md".into(),
            },
            0,
        ))];
        let found = vec![file(1, "/one/a.md")];
        assert!(
            copy_over_paths(&found, &registry_of(&rows), &rows).is_empty(),
            "the row the ledger named is not the row at this address"
        );
    }

    #[test]
    fn a_link_is_invisible_to_a_scan() {
        // A pointer is not a copy of a file: no fingerprint to match, no
        // address to relink, nothing a folder could place.
        let rows = vec![
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
        ];
        let r = registry_of(&rows);
        assert_eq!(r.len(), 1, "the link is not in it");
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(index_by_fp(&rows).len(), 1);
        let mut rows = rows;
        assert!(!relink(&mut rows, "l1", "/somewhere/a.pdf"));
        assert!(relink(&mut rows, "b1", "/moved/a.pdf"));
        assert_eq!(at(&rows, 1).path(), "/moved/a.pdf");
    }

    #[test]
    fn the_registry_is_built_from_the_books_and_first_row_wins() {
        let books = vec![
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
            Row::Book(Book {
                id: "dup".into(),
                ..book_value("b1", Origin::Linked { src: "/books/a.pdf".into() }, false)
            }),
        ];
        let r = registry_of(&books);
        assert_eq!(r.len(), 1);
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(r[&fp(1)].path, "/books/a.pdf");
        assert!(!r[&fp(1)].missing);
    }
}
