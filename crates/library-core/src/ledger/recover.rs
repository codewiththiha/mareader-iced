//! What a folder can still give back: the books a removal took, gathered from
//! its stones, and the relink that puts one back on its shelf.

use std::collections::HashMap;

use crate::book::{book_rows_mut, Book, Fingerprint, Row};
use crate::folder::{Tombstone, WatchedFolder};
use crate::shelf::Shelf;

/// A book this folder could give the reader back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// Removed by the reader and still not in the library. The file may or
    /// may not still be on disk; a restore measures before it promises.
    Deleted(Tombstone),
    /// Still in the library and still inside this folder on disk, but no
    /// longer on any shelf this folder owns: the reader moved it elsewhere in
    /// the app. Nothing is re-imported — the offer is to show it here as well,
    /// or to go look at where it went.
    Moved {
        book_id: String,
        /// The name the shelf shows: the document's title when it had one,
        /// else the stem the reader knows — a stored book's source, never the
        /// store's own `source.pdf`.
        title: Option<String>,
        path: String,
        /// The first shelf the book is on, by name, for the row's "now in
        /// Fiction" half. `None` when the book is on no shelf.
        home_shelf: Option<String>,
    },
}

/// What this folder's import menu can offer to give back.
///
/// Pure and synchronous by design: the menu opens on a click and answers from
/// the last scan's `last_seen` plus the folder's tombstones — a walk over two
/// short lists, not over a directory tree. A restore re-measures the one file
/// it is about to import, which is where freshness actually matters.
///
/// Takes the shelf list rather than a membership callback: which shelves a
/// book is on, and which the folder owns, are rules [`crate::shelf`] owns.
/// "First membership" is shelf order, so the row's home is deterministic.
pub fn recoverables(
    folder: &WatchedFolder,
    books_by_fp: &HashMap<Fingerprint, &Book>,
    shelves: &[Shelf],
) -> Vec<Recovered> {
    let mut out = Vec::new();

    // Removed books first: an offer to give something back outranks an offer
    // to show you what you already have.
    for entry in &folder.ignored {
        if books_by_fp.contains_key(&entry.fp) {
            continue;
        }
        // A moved-out log is not a removal: the library still holds the book.
        if entry.moved {
            continue;
        }
        out.push(Recovered::Deleted(entry.clone()));
    }

    let owned_by_folder = |shelf: &Shelf| shelf.kind.folder_id() == Some(folder.id.as_str());
    for (fp, path) in &folder.last_seen {
        let Some(book) = books_by_fp.get(fp) else {
            continue;
        };
        // The card already offers a relink; a second door here would not know
        // the address is bad.
        if book.missing {
            continue;
        }
        let on = crate::shelf::containing(shelves, &book.id);
        if on.iter().any(|shelf| owned_by_folder(shelf)) {
            continue;
        }
        out.push(Recovered::Moved {
            book_id: book.id.clone(),
            // The display name: the menu's `path` fallback would read the
            // store's own `source.pdf` for a stored book.
            title: Some(book.title()),
            path: path.clone(),
            home_shelf: on.first().map(|shelf| shelf.name.clone()),
        });
    }
    out
}

/// Apply a `Relink`: rewrite the address, clear `missing`, keep everything
/// else. A stored book does not take the new address — its bytes are the app's
/// own copy — so only its recorded source moves.
pub fn relink(rows: &mut [Row], book_id: &str, to: &str) -> bool {
    // A link has no address to move, so `book_rows_mut` is the whole guard.
    let Some(book) = book_rows_mut(rows).find(|b| b.id == book_id) else {
        return false;
    };
    match &mut book.origin {
        crate::book::Origin::Linked { src } => {
            *src = to.to_string();
            book.missing = false;
        }
        crate::book::Origin::Stored { src, .. } => {
            *src = Some(to.to_string());
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Origin, Row};
    use crate::folder::Tombstone;
    use crate::ledger::kit::{at, book, book_value, file, folder, fp, stone};
    use crate::ledger::registry::index_by_fp;
    use crate::shelf::Shelf;

    #[test]
    fn a_relink_moves_the_address_and_keeps_everything_else() {
        let mut books = vec![book("b1", Origin::Linked { src: "/gone/a.pdf".into() }, true)];
        assert!(relink(&mut books, "b1", "/books/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/books/a.pdf");
        assert!(!at(&books, 0).missing);
        assert_eq!(at(&books, 0).page, 42, "the resume point is the reader's, not the scan's");
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(books[0].id(), "b1");
        assert!(!relink(&mut books, "zzz", "/x"));
    }

    #[test]
    fn a_relink_never_repoints_a_stored_book_at_the_source() {
        // The store copy is what the reader opens; the source moving is
        // provenance.
        let mut books = vec![book(
            "b1",
            Origin::Stored {
                src: Some("/gone/a.pdf".into()),
                store: "/app/store/pdf/a_b1.pdf".into(),
            },
            false,
        )];
        assert!(relink(&mut books, "b1", "/downloads/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/app/store/pdf/a_b1.pdf");
        assert_eq!(at(&books, 0).origin.source(), Some("/downloads/a.pdf"));
    }

    #[test]
    fn a_removed_book_is_offered_back_with_enough_to_recognise_it() {
        let f = folder(&[1], &[2]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[fshelf("s1", "Books", &[])]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Recovered::Deleted(entry) => {
                assert_eq!(entry.fp, fp(2));
                assert_eq!(entry.label(), "Book 2");
                assert_eq!(entry.last_path, "/books/2.pdf");
            }
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_moved_out_log_is_not_offered_back_as_a_removal() {
        // The book a moved-out log belongs to is still in the library as its
        // own stored copy; the way back is an import, which spends the log.
        let mut f = folder(&[1], &[2]);
        f.ignored.push(Tombstone {
            moved: true,
            returned_row: Some("b9".into()),
            ..stone(3)
        });
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[]);
        assert_eq!(out.len(), 1, "only the real removal is offered back");
        match &out[0] {
            Recovered::Deleted(entry) => assert_eq!(entry.fp, fp(2)),
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_book_that_came_back_by_another_route_is_not_a_recovery() {
        let f = folder(&[1], &[1]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &[]).is_empty());
    }

    #[test]
    fn a_book_moved_off_every_folder_shelf_is_offered_as_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        assert_eq!(
            recoverables(&f, &index, &shelves),
            vec![Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/1.pdf".into(),
                home_shelf: Some("Fiction".into()),
            }]
        );
    }

    #[test]
    fn a_book_still_on_one_of_the_folders_shelves_is_not_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let here = [fshelf("s1", "Books", &["b1"])];
        assert!(recoverables(&f, &index, &here).is_empty());
    }

    #[test]
    fn a_book_on_a_shelf_the_folder_does_not_own_is_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            fshelf("s2", "Sci-fi", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_missing_book_is_a_relink_and_not_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", true)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        assert!(recoverables(&f, &index, &shelves).is_empty());
    }

    #[test]
    fn a_file_no_longer_in_the_tree_is_neither_a_move_nor_a_removal() {
        let mut f = folder(&[1, 2], &[]);
        f.last_seen = vec![(fp(2), "/books/2.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &[fshelf("s1", "Books", &[])]).is_empty());
    }

    #[test]
    fn removals_are_listed_before_moves() {
        let mut f = folder(&[1, 2], &[2]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], Recovered::Deleted(_)));
        assert!(matches!(out[1], Recovered::Moved { .. }));
    }

    #[test]
    fn a_home_shelf_is_the_first_one_in_shelf_order() {
        // Deterministic shelf order, because the row's label is a sentence.
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s8", "Fiction", &["b1"]),
            vshelf("s3", "Classics", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => {
                assert_eq!(home_shelf.as_deref(), Some("Fiction"))
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_book_on_no_shelf_at_all_has_no_home_to_name() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[fshelf("s1", "Books", &[])]);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => assert_eq!(home_shelf, &None),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_tombstone_is_the_record_a_restore_row_needs() {
        let b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/dune.pdf".into(),
            },
            false,
        );
        let entry = Tombstone::of(&b, Some("s2".into()), 999);
        assert_eq!(entry.fp, b.fp);
        assert_eq!(entry.title.as_deref(), Some("Dune"));
        assert_eq!(entry.format, reader_core::format::Format::Pdf);
        assert_eq!(entry.last_path, "/books/dune.pdf");
        assert_eq!(entry.shelf_id.as_deref(), Some("s2"));
        assert_eq!(entry.removed_ms, 999);
        assert_eq!(entry.label(), "Dune");
    }

    #[test]
    fn a_tombstone_of_a_stored_book_names_the_file_it_came_from() {
        // The log labels itself with the name the shelf showed, never the
        // store's own "source.pdf".
        let mut b = book_value(
            "b1",
            Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            false,
        );
        b.title = None;
        assert_eq!(Tombstone::of(&b, None, 1).label(), "dune");
    }

    #[test]
    fn a_tombstone_of_a_book_never_opened_labels_itself_from_the_file() {
        let mut b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/rust-book.pdf".into(),
            },
            false,
        );
        b.title = None;
        assert_eq!(Tombstone::of(&b, None, 1).label(), "rust-book");
    }

    #[test]
    fn a_tombstone_crosses_the_wire_with_its_camel_case_names() {
        let entry = stone(3);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"lastPath\""), "{json}");
        assert!(json.contains("\"removedMs\""), "{json}");
        assert!(json.contains("\"shelfId\""), "{json}");
        assert!(!json.contains('_'), "{json}");
        let back: Tombstone = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
        let older: Tombstone = serde_json::from_str(
            r#"{"fp":{"size":1,"mtimeMs":1,"headHash":1},"format":"pdf",
                "lastPath":"/a.pdf","removedMs":2}"#,
        )
        .unwrap();
        assert_eq!(older.shelf_id, None);
        assert_eq!(older.title, None);
        assert!(!older.moved);
        assert_eq!(older.returned_row, None);
        let moved_stone = Tombstone {
            moved: true,
            returned_row: Some("b7".into()),
            ..stone(3)
        };
        let json = serde_json::to_string(&moved_stone).unwrap();
        assert!(json.contains("\"moved\":true"), "{json}");
        assert!(json.contains("\"returnedRow\":\"b7\""), "{json}");
        let back: Tombstone = serde_json::from_str(&json).unwrap();
        assert_eq!(back, moved_stone);
    }

    #[test]
    fn the_last_scan_is_remembered_only_for_books_this_folder_placed() {
        let mut f = folder(&[1], &[]);
        let found = vec![file(1, "/books/1.pdf"), file(2, "/books/2.pdf")];
        f.record_seen(&found);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/1.pdf".to_string())]);
        // A second scan replaces the first: the menu answers "where is it
        // now", not "where has it ever been".
        f.record_seen(&[]);
        assert!(f.last_seen.is_empty());
    }

    #[test]
    fn a_scan_that_changed_nothing_still_refreshes_what_was_seen() {
        // `record_seen` is not behind the "did anything change" check: a book
        // moved out between two quiet scans is what the menu has to see.
        let mut f = folder(&[1], &[]);
        f.record_seen(&[file(1, "/books/1.pdf")]);
        f.record_seen(&[file(1, "/books/moved/1.pdf")]);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/moved/1.pdf".to_string())]);
    }

    fn sized_book(id: &str, n: u32, path: &str, missing: bool) -> Row {
        Row::Book(Book {
            fp: fp(n),
            origin: Origin::Linked {
                src: path.to_string(),
            },
            missing,
            ..book_value(id, Origin::Linked { src: path.to_string() }, false)
        })
    }

    fn vshelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: crate::shelf::ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    fn fshelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            kind: crate::shelf::ShelfKind::Folder {
                folder_id: "f1".to_string(),
                rel: None,
            },
            ..vshelf(id, name, books)
        }
    }
}
