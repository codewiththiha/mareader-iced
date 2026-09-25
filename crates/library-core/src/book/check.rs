//! The path check: what a walk's measurement does to the rows it lands on.
//!
//! A file that moved inside a watched tree is the same book at a new address;
//! a row migrated from the previous schema carries a placeholder identity
//! nothing has measured yet.

use super::{Book, Row, book_rows, book_rows_mut};

/// Apply one path check to every book at that address, returning the ids it
/// touched.
///
/// Every row at the address shares its fate: a check that healed one row and
/// left a twin pending would hold every watched folder's rescan off forever.
/// Independent rows are no exception — this is the one place their opt-out
/// would strand a twin.
pub fn apply_check(rows: &mut [Row], check: &crate::wire::PathCheck) -> Vec<String> {
    let measured = check.fingerprint();
    let mut touched = Vec::new();
    for book in book_rows_mut(rows).filter(|b| b.path() == check.path) {
        let changed = match measured {
            Some(fp) => {
                let changed = book.fp != fp || book.missing || book.fp_pending;
                book.heal(fp);
                changed
            }
            None => {
                // Going missing is news; being told twice is not. Clearing a
                // pending mark is news too: it releases the folder's rescan.
                let changed = !book.missing || book.fp_pending;
                book.missing = true;
                book.fp_pending = false;
                changed
            }
        };
        if changed {
            touched.push(book.id.clone());
        }
    }
    touched
}

/// Add an imported book at the end of the library's order, or return the id
/// of the one already there.
///
/// Content identity decides, not the address: the same file reached through a
/// second watched folder is the same book (see [`crate::ledger`]). The first
/// row wins when duplicates the reader chose to keep share a fingerprint.
pub fn add_book(rows: &mut Vec<Row>, book: Book) -> String {
    // Never resolve to an independent row: that would file a private book on
    // a shelf the import never mentioned.
    if let Some(existing) = book_rows(rows).find(|b| b.fp == book.fp && !b.independent) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    rows.push(Row::Book(book));
    id
}

/// Remove a row by id, returning it — a book or a link, since a shelf holds
/// both. What else the removal implies (ledger tombstone, shelf membership,
/// store copy, dangling links) is the caller's decision.
pub fn remove_row(rows: &mut Vec<Row>, id: &str) -> Option<Row> {
    let at = rows.iter().position(|r| r.id() == id)?;
    Some(rows.remove(at))
}

/// Drop every link whose target is no longer a book in the list: a pointer at
/// nothing renders, is clicked, and does nothing. Runs after any removal and
/// on every load via [`sanitize`].
pub fn drop_dangling_links(rows: &mut Vec<Row>) {
    // Owned ids: the set must not borrow the list `retain` walks mutably.
    let books: std::collections::HashSet<String> =
        book_rows(rows).map(|b| b.id.clone()).collect();
    rows.retain(|r| {
        !matches!(r, Row::Link { target, .. }
            if !books.contains(target) && !crate::id::is_shelf(target))
    });
}

/// The shelf-target half of [`drop_dangling_links`], which needs the shelf
/// list to answer.
pub fn drop_dead_shelf_links(rows: &mut Vec<Row>, shelves: &[crate::shelf::Shelf]) {
    rows.retain(|r| match r {
        Row::Link { target, .. } if crate::id::is_shelf(target) => {
            shelves.iter().any(|s| &s.id == target)
        }
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::check::add_book;
    use crate::book::check::apply_check;
    use crate::book::check::drop_dead_shelf_links;
    use crate::book::check::remove_row;
    use crate::book::kit::{at, check, fp, linked, private, rows};

    #[test]
    fn a_linked_book_is_the_path_it_was_opened_from() {
        let b = linked("a", "/books/dune.pdf");
        assert_eq!(b.path(), "/books/dune.pdf");
        assert_eq!(b.origin.source(), Some("/books/dune.pdf"));
        assert!(!b.origin.is_stored());
    }

    #[test]
    fn progress_is_a_page_fraction_or_the_stream_fraction() {
        let mut b = linked("a", "/books/dune.pdf");
        assert_eq!(b.progress(), None, "an unknown page count is no progress");
        b.num_pages = 200;
        b.page = 50;
        assert_eq!(b.progress(), Some(0.25));
        b.page = 900;
        assert_eq!(b.progress(), Some(1.0), "past the end clamps");
        let mut s = linked("s", "/notes.md");
        s.fraction = Some(0.4);
        assert_eq!(s.progress(), Some(0.4));
        s.fraction = Some(1.7);
        assert_eq!(s.progress(), None, "an out-of-range fraction is dropped");
    }

    #[test]
    fn a_path_check_is_what_measures_a_book() {
        let mut books = rows([Book {
            fp_pending: true,
            ..linked("a", "/books/one.pdf")
        }]);
        let touched = apply_check(
            &mut books,
            &check("/books/one.pdf", true, 20, 2, 8),
        );
        assert_eq!(touched, vec!["a".to_string()]);
        assert_eq!(at(&books, 0).fp, fp(20, 2, 8));
        assert!(!at(&books, 0).fp_pending, "the measurement replaces the placeholder");
        assert!(apply_check(&mut books, &check("/books/one.pdf", true, 20, 2, 8)).is_empty());
    }

    #[test]
    fn a_path_that_does_not_resolve_marks_the_book_missing_and_keeps_it() {
        let mut books = rows([Book {
            page: 42,
            num_pages: 100,
            ..linked("a", "/books/one.pdf")
        }]);
        assert_eq!(
            apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)),
            vec!["a".to_string()]
        );
        assert!(at(&books, 0).missing);
        assert!(!at(&books, 0).fp_pending, "a check that ran is not a check still owed");
        assert_eq!(books.len(), 1, "a missing book is not a removed one");
        assert_eq!(at(&books, 0).page, 42, "the resume point survives the address dying");
        assert_eq!(at(&books, 0).fp, fp(10, 1, 7), "and so does the last known identity");
        assert!(apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)).is_empty());
    }

    #[test]
    fn a_check_for_an_address_the_library_does_not_hold_does_nothing() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        assert!(apply_check(&mut books, &check("/books/other.pdf", true, 1, 1, 1)).is_empty());
        assert!(!at(&books, 0).missing);
    }

    #[test]
    fn two_rows_of_one_file_are_two_books_with_two_ids() {
        // Marks are keyed by row id; the crate owes that the two rows are two
        // ids and never resolve to each other.
        let shared = linked("a", "/books/dune.pdf");
        let own = private("b", "/books/dune.pdf");
        assert_eq!(shared.id, "a");
        assert_eq!(own.id, "b");
        assert_ne!(shared.id, own.id, "two rows, two keys, two mark lists");
        assert!(own.independent && !shared.independent);
        // An import resolving this address must never land on the private row.
        let mut list = rows([own.clone()]);
        assert_eq!(add_book(&mut list, shared.clone()), "a", "a private row holds nothing back");
        assert_eq!(list.len(), 2, "so the arrival joins as a book of its own");
        for order in [
            rows([shared.clone(), own.clone()]),
            rows([own.clone(), shared.clone()]),
        ] {
            let mut both = order;
            assert_eq!(
                add_book(&mut both, Book { fp: fp(10, 1, 7), ..linked("new", "/books/dune.pdf") }),
                "a",
                "a shared row at the address is the one an import resolves to"
            );
        }
    }

    #[test]
    fn a_scan_names_the_shared_row_and_only_falls_back_to_a_private_one() {
        let books = rows([private("a", "/one/dune.pdf"), linked("b", "/two/dune.pdf")]);
        let registry = crate::ledger::registry_of(&books);
        assert_eq!(
            registry.get(&fp(10, 1, 7)).map(|k| k.id.as_str()),
            Some("b"),
            "the file moved, so the shared row is the one that follows it"
        );
        let only = rows([private("a", "/one/dune.pdf")]);
        assert_eq!(
            crate::ledger::registry_of(&only).get(&fp(10, 1, 7)).map(|k| k.id.as_str()),
            Some("a")
        );
    }

    #[test]
    fn different_content_is_a_different_book() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        let other = Book {
            fp: fp(11, 1, 7),
            ..linked("b", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, other), "b");
        assert_eq!(books.len(), 2);
    }

    #[test]
    fn removing_returns_the_book_so_the_caller_can_finish_the_job() {
        let mut books = rows([linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")]);
        let gone = remove_row(&mut books, "a").expect("present");
        assert_eq!(gone.book().expect("a book row").path(), "/books/one.pdf");
        assert_eq!(books.len(), 1);
        assert!(remove_row(&mut books, "zzz").is_none());
    }

    #[test]
    fn a_link_to_a_shelf_goes_with_the_shelf() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        books.push(Row::link("l1".into(), "Books".into(), "s1".into(), 1));
        books.push(Row::link("l2".into(), "Comics".into(), "s2".into(), 1));
        let shelves = vec![crate::shelf::Shelf {
            id: "s1".into(),
            name: "Books".into(),
            kind: Default::default(),
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }];
        drop_dead_shelf_links(&mut books, &shelves);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "l1"], "only the pointer at a shelf that is gone goes");
    }

    #[test]
    fn adopting_a_measurement_makes_the_copy_the_identity() {
        let mut b = linked("b1", "/src/a.pdf");
        b.fp_pending = true;
        b.adopt_measurement(Some(fp(99, 5, 3)));
        assert_eq!(b.fp, fp(99, 5, 3));
        assert!(!b.fp_pending);
        // Adoption answers "what is this instance", not "is the address there";
        // callers that made the bytes their own clear `missing` themselves.
        assert!(!b.missing);
    }

    #[test]
    fn a_copy_that_could_not_be_weighed_stays_pending() {
        // A fingerprint nobody measured would be a guess every later rescan
        // trusts; a guess that matched nothing is a book added twice.
        let mut b = linked("b1", "/src/a.pdf");
        b.adopt_measurement(None);
        assert!(b.fp_pending);
        assert_eq!(b.fp, fp(10, 1, 7), "the identity it had is left alone");
    }

    #[test]
    fn a_title_the_document_gave_survives_a_departure() {
        let mut b = linked("b1", "/src/a.pdf");
        b.title = Some("Dune".to_string());
        b.become_stored("/src/a.pdf", "/store/b1.pdf".into(), None);
        assert_eq!(b.title.as_deref(), Some("Dune"), "a name is not a gap");
        assert!(b.fp_pending, "a copy nobody weighed is still owed its first check");
    }

    #[test]
    fn healing_an_address_brings_a_missing_book_back() {
        // The first walk that finds the file replaces the migrated row's
        // placeholder; until then every watched folder's rescan is held off.
        let mut b = linked("b1", "/src/a.pdf");
        b.missing = true;
        b.fp_pending = true;
        b.heal(fp(40, 9, 2));
        assert_eq!(b.fp, fp(40, 9, 2));
        assert!(!b.missing);
        assert!(!b.fp_pending);
    }
}
