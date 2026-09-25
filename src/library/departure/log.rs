//! The moved-out log: the stones a departure writes for the folders that
//! placed a book, and the return that binds them again.

use library_core::book::{Book, find_row, Row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone, WatchedFolder};
use library_core::ledger::tombstone;
use library_core::shelf::{self, ALL_SHELF, Shelf};

/// The shelf of a folder that holds a book: one answer rather than every
/// answer, because a removed book comes back to ONE shelf.
pub fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|s| s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}

/// Every folder that placed a fingerprint records that the book left as the
/// library's own copy rather than died. `returned_row` names the row the
/// file is represented by, for the answers that dissolve a linked row into a
/// book the library already holds.
///
/// Persists nothing itself: every caller ends its own transaction with a
/// persist, and this log is one write inside it.
pub fn write_moved_stones(
    folders: &mut [WatchedFolder],
    shelves: &[Shelf],
    book: &Book,
    returned_row: Option<&str>,
    now: u64,
) {
    let home = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .and_then(|f| folder_shelf_of(shelves, &f.id, &book.id));
    // Spelling all eight fields here would be a second place a new
    // `Tombstone` field has to be remembered.
    let entry = Tombstone {
        moved: true,
        returned_row: returned_row.map(str::to_string),
        ..Tombstone::of(book, home, now)
    };
    tombstone(folders, &entry);
}

/// The bind is by ADDRESS, and the address is the one thing a copy cannot
/// change: the log's fp is the file's and the row's is its own copy's stamp,
/// so the fingerprints can never meet again — but the log remembers where the
/// file stood (`last_path`) and the row remembers where its bytes came from
/// (`origin.source()`), and two books called "Dune" in one folder left two
/// logs from two addresses. A row with no address to name — a legacy copy —
/// falls back to the name, and only while it is still wearing its pending
/// placeholder. True when the bind wrote.
pub fn bind_returned(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &mut [WatchedFolder],
    row_id: &str,
    shelf_id: &str,
) -> bool {
    if shelf_id == ALL_SHELF {
        return false;
    }
    let Some((name, src, measured)) = find_row(rows, row_id).and_then(|row| {
        let book = row.book().filter(|b| b.origin.is_stored())?;
        Some((row.display_name(), book.origin.source().map(str::to_string), !book.fp_pending))
    }) else {
        return false;
    };
    let Some(folder_id) = shelf::find(shelves, shelf_id).and_then(|s| s.kind.folder_id()) else {
        return false;
    };
    let Some(folder) = folder_ops::find_mut(folders, folder_id) else {
        return false;
    };
    let is_the_one = |entry: &Tombstone| {
        if !entry.moved {
            return false;
        }
        match src.as_deref() {
            Some(src) => src == entry.last_path,
            None => !measured && same_name(&entry.label(), &name),
        }
    };
    if let Some(entry) = folder.ignored.iter_mut().find(|entry| is_the_one(entry))
        && entry.returned_row.as_deref() != Some(row_id)
    {
        entry.returned_row = Some(row_id.to_string());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use crate::library::departure::kit::nested;
    use crate::library::departure::log::bind_returned;
    use crate::library::departure::log::write_moved_stones;
    use library_core::book::find_book_mut;
    use library_core::folder::{Tombstone, WatchedFolder};
    use library_core::testkit;
    use reader_core::format::Format;

    #[test]
    fn a_departure_writes_its_moved_log_on_the_placing_folder() {
        let rows = [testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
        let book = rows[0].book().expect("the row is a book");
        let mut held = testkit::folder_shelf("shelf3", "shelf3", "f1", Some("Fiction/SciFi"), &[], None);
        held.books = vec!["b1".to_string()];
        let shelves = vec![held];
        let mut folders = vec![nested(7)];

        write_moved_stones(&mut folders, &shelves, book, None, 42);

        let entry = folders[0].ignored.iter().find(|t| t.moved).expect("the moved log");
        assert_eq!(entry.last_path, "/books/Fiction/SciFi/dune.md", "the address the bind reads");
        assert_eq!(
            entry.shelf_id.as_deref(),
            Some("shelf3"),
            "the rung that held it is the restore's seat"
        );
        assert!(entry.returned_row.is_none(), "a departure names no return of its own");
    }

    #[test]
    fn a_return_binds_the_log_by_the_address_they_share() {
        let mut rows = vec![testkit::stored_row("b1", "/books/Fiction/SciFi/dune.md", "/store/b1.md", 9)];
        find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
        let shelves = vec![testkit::folder_shelf("shelf2", "shelf2", "f1", Some("Fiction"), &[], None)];
        let mut folders = vec![folder_with_moved_log()];

        // The shape of a return: a stored row whose source is the log's own
        // last address, landing on a shelf the folder names.
        assert!(bind_returned(&rows, &shelves, &mut folders, "b1", "shelf2"));
        assert_eq!(
            folders[0].ignored[0].returned_row.as_deref(),
            Some("b1"),
            "bound to the row by the address they share"
        );
        assert!(
            !bind_returned(&rows, &shelves, &mut folders, "b1", "shelf2"),
            "a log already bound to this row binds nothing again"
        );

        // A departure's own landing is no return: the resume carries the
        // copies and skips the bind, so the log stays free for the row that
        // actually comes home. That skip is the app's resume; the bind it
        // skips is the one above.
        let mut fresh = vec![folder_with_moved_log()];
        let linked = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(
            !bind_returned(&linked, &shelves, &mut fresh, "b1", "shelf2"),
            "a linked row is nobody's return — only the library's own copies bind"
        );
    }

    fn folder_with_moved_log() -> WatchedFolder {
        WatchedFolder {
            placed: HashSet::from([testkit::fp_n(7)]),
            ignored: vec![Tombstone {
                fp: testkit::fp_n(7),
                title: Some("Dune".to_string()),
                format: Format::Markdown,
                last_path: "/books/Fiction/SciFi/dune.md".to_string(),
                shelf_id: Some("shelf3".to_string()),
                removed_ms: 5,
                moved: true,
                returned_row: None,
            }],
            shelf_map: BTreeMap::from([
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            ..testkit::watched_folder("f1", "/books")
        }
    }
}
