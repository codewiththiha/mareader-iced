//! The departure: a read-at-place book leaving the ground that made it
//! becomes the library's own stored copy on the way out — its bytes its own,
//! the ORIGINAL fingerprint left free for the folder's log to keep — and the
//! copy is a cost the reader agrees to. One sheet for every door in, so no
//! path stores a file in silence.
//!
//! This is the pure half the web service ran against its signals: the screen
//! that says which rows a move takes into the store, the ask that names the
//! cost, the moved-out log a departure writes and a return binds. The store
//! batch between the ask and the landing is the app's, and so is the queue
//! of doors — the shelf's, the rung's and the removal's arrive with the
//! systems that own them; the move's is here.
//!
//! Two halves the native app does not carry yet, both documented where the
//! web does them: the covers a departure prunes and backfills wait on the
//! engines, and the highlights that follow the address wait on the reader's
//! marks store.

use library_core::book::{find_row, Book, Origin, Row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone, WatchedFolder};
use library_core::ledger::tombstone;
use library_core::paths::dir_label;
use library_core::shelf::{self, Shelf, ALL_SHELF};
use library_core::text::plural;

/// The gesture a copy question interrupted, so the sheet's own answer can
/// finish it: the same rows land, and the copies land marked — a departure
/// is not a return, so a copied book binds no folder's moved-out log, and a
/// book the move already took off its seat stays off it.
#[derive(Clone, PartialEq, Debug)]
pub enum RowMove {
    /// A drag or a bulk filing: the rows land on `to`, lifted off `from`
    /// where the two differ.
    Seat { from: Option<String>, to: String, index: Option<usize> },
    /// One row's own move, the form the conflict sheet rides as well as a
    /// drag of a single book. The single-row door arrives with the conflict
    /// sheet; the gesture's resume already answers it.
    #[allow(dead_code)]
    Row { to: String, index: Option<usize> },
    /// Out of every shelf the row was on, to the library's own top level.
    Unfile { shelf: String },
}

impl RowMove {
    /// Where the rows are going: the gate screens the gesture against it,
    /// and the answer screens again, because the sheet was up while the
    /// library went on living.
    pub fn to(&self) -> &str {
        match self {
            RowMove::Seat { to, .. } | RowMove::Row { to, .. } => to,
            RowMove::Unfile { .. } => ALL_SHELF,
        }
    }
}

/// The reader's answer: buy the copies, finish the gesture without them, or
/// leave everything as it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyAnswer {
    Copy,
    /// Finish the gesture but leave the books where they are: the shelf's
    /// door answers it by taking its read-at-place books home, the removal's
    /// by removing. The move's door has one way through, so these wait on
    /// the systems that own them — the sheet's handler already reads them.
    #[allow(dead_code)]
    WithoutCopies,
    #[allow(dead_code)]
    Cancel,
}

/// One answer's button. Built at the raise rather than at the click, because
/// the sheet's own wording is a fact about the gesture and not something the
/// view recomputes.
#[derive(Clone, PartialEq, Debug)]
pub struct CopyOption {
    pub label: String,
    pub title: String,
    pub answer: CopyAnswer,
    pub primary: bool,
}

/// What the reader is in the middle of, and everything the answer needs to
/// finish it. One enum rather than a flag per door: the sheet names the
/// action, and the answer resumes THAT gesture rather than a second one
/// built from the same facts. The shelf's, the rung's and the removal's
/// variants arrive with the systems that own them.
#[derive(Clone, PartialEq, Debug)]
pub enum CopyWork {
    /// Books on the move, and the way back into the drag or filing that held
    /// them.
    Rows { ids: Vec<String>, hand: RowMove },
}

/// The question, ready for the sheet: what it is about, what it costs, and
/// the answers.
#[derive(Clone, PartialEq, Debug)]
pub struct CopyAsk {
    /// The action the reader is in the middle of, in their own words.
    pub action: String,
    /// The shelf or the books it is about.
    pub subject: String,
    /// The cost, in one or two lines.
    pub lines: Vec<String>,
    pub options: Vec<CopyOption>,
    pub work: CopyWork,
}

/// The promise every copy's sheet makes: the bytes come IN, nothing goes
/// out.
pub const UNTOUCHED: &str = "The folder on disk is untouched.";

fn copying(label: &str) -> CopyOption {
    CopyOption {
        label: label.to_string(),
        title: "Copy the books read in place into the library, then finish".to_string(),
        answer: CopyAnswer::Copy,
        primary: true,
    }
}

/// The rows a move to `to` takes into the store, in the order the gesture
/// held them.
pub fn converting_rows(
    rows: &[Row],
    folders: &[WatchedFolder],
    ids: &[String],
    to: &str,
) -> Vec<String> {
    ids.iter().filter(|id| converts_on_move(rows, folders, id, to)).cloned().collect()
}

/// The three negatives are as load-bearing as the positive: a STORED book is
/// already the library's own and simply moves, and a book no in-place folder
/// placed is nobody's departure.
pub fn converts_on_move(rows: &[Row], folders: &[WatchedFolder], row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = find_row(rows, row_id)
        .and_then(|row| row.book())
        .filter(|book| matches!(book.origin, Origin::Linked { .. }))
        .map(|book| (book.fp, book.path().to_string()))
    else {
        return false;
    };
    // Only a folder that placed this content owes a departure; a deleted
    // rung answers `None` and so never matches the destination.
    let placing: Vec<&WatchedFolder> = folders
        .iter()
        .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
        .collect();
    if placing.is_empty() {
        return false;
    }
    to == ALL_SHELF || !placing.iter().any(|f| f.rungs_for(&path).0 == Some(to))
}

/// A book move's ask: the rows the gate screened read in place, and the
/// ground they are leaving. `None` when nothing converts — the gesture then
/// runs as it always did.
pub fn ask_of_rows(
    rows: &[Row],
    folders: &[WatchedFolder],
    ids: &[String],
    hand: RowMove,
) -> Option<CopyAsk> {
    let books = ids.len();
    let folder = ground_of(rows, folders, ids)?;
    Some(CopyAsk {
        action: "Move books".to_string(),
        subject: format!("{} from “{folder}”", plural(books, "book", "books")),
        lines: vec![
            plural(
                books,
                "It is read in place; moving it out stores a copy.",
                "They are read in place; moving them out stores copies.",
            ),
            UNTOUCHED.to_string(),
        ],
        options: vec![copying("Copy and move")],
        work: CopyWork::Rows { ids: ids.to_vec(), hand },
    })
}

/// The folder whose ground these books leave: the first that placed one of
/// them, and the one the sheet names.
fn ground_of(rows: &[Row], folders: &[WatchedFolder], ids: &[String]) -> Option<String> {
    ids.iter().find_map(|id| {
        let fp = find_row(rows, id).and_then(|row| row.book()).map(|b| b.fp)?;
        let folder =
            folders.iter().find(|f| f.mode().reads_in_place() && f.placed.contains(&fp))?;
        Some(dir_label(&folder.root))
    })
}

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
    use super::*;
    use library_core::book::find_book_mut;
    use library_core::testkit;
    use reader_core::format::Format;
    use std::collections::{BTreeMap, HashSet};

    fn linked_at(id: &str, path: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            testkit::fp_n(n),
            Format::Markdown,
            Origin::Linked { src: path.to_string() },
            0,
        ))
    }

    fn stored_at(id: &str, src: &str, store: &str, n: u32) -> Row {
        Row::Book(Book::new(
            id.to_string(),
            testkit::fp_n(n),
            Format::Markdown,
            Origin::Stored { src: Some(src.to_string()), store: store.to_string() },
            0,
        ))
    }

    fn nested(n: u32) -> WatchedFolder {
        WatchedFolder {
            placed: HashSet::from([testkit::fp_n(n)]),
            shelf_map: BTreeMap::from([
                (String::new(), "shelf1".to_string()),
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            ..testkit::watched_folder("f1", "/books")
        }
    }

    fn folder_shelf_at(id: &str, folder_id: &str, rel: &str) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: library_core::shelf::ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: Some(rel.to_string()),
            },
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }
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

    #[test]
    fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
        let folders = vec![nested(7)];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        // Reading the tie as the folder's shelf tree instead — "any shelf
        // this folder owns" — left the row linked at an address it had been
        // dragged off.
        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
        assert!(converts_on_move(&rows, &folders, "b1", "shelf1"));
        assert!(converts_on_move(&rows, &folders, "b1", "elsewhere"));
    }

    #[test]
    fn a_reorder_on_the_book_s_own_rung_copies_nothing() {
        let folders = vec![nested(7)];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        // Re-ordering the books a folder placed, on the rung it placed them
        // on, is the folder's own business.
        assert!(!converts_on_move(&rows, &folders, "b1", "shelf3"));
    }

    #[test]
    fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
        let folders = vec![nested(7)];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(converts_on_move(&rows, &folders, "b1", ALL_SHELF));
        assert!(converts_on_move(&rows, &folders, "b1", "mine"));
    }

    #[test]
    fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
        let mut folder = nested(7);
        folder.shelf_map.remove("Fiction/SciFi");
        let folders = vec![folder];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
        assert!(converts_on_move(&rows, &folders, "b1", "shelf3"));
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
        let mut folder = nested(7);
        folder.opts.groups = false;
        folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
        let folders = vec![folder];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(!converts_on_move(&rows, &folders, "b1", "flat"));
        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
    }

    #[test]
    fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
        let mut copying = nested(7);
        copying.id = "f2".into();
        copying.opts.in_place = false;
        let folders = vec![nested(7), copying];
        let rows = vec![
            linked_at("b1", "/books/Fiction/SciFi/dune.md", 7),
            stored_at("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
            linked_at("b3", "/elsewhere/loose.md", 11),
        ];

        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "b2", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "b3", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "gone", "shelf2"));
    }

    #[test]
    fn the_ask_names_the_ground_and_the_cost() {
        let folders = vec![nested(7)];
        let rows = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        let ids = vec!["b1".to_string()];
        let converting = converting_rows(&rows, &folders, &ids, "mine");
        assert_eq!(converting, ids, "the screen keeps the gesture's own order");

        let hand = RowMove::Seat { from: Some("shelf3".into()), to: "mine".into(), index: None };
        let ask = ask_of_rows(&rows, &folders, &converting, hand).expect("one book owes a copy");
        assert_eq!(ask.action, "Move books");
        assert_eq!(ask.subject, "1 book from “books”", "the folder names the ground");
        assert!(ask.lines[0].contains("read in place"), "the cost, in the sheet's own sentence");
        assert_eq!(ask.lines[1], UNTOUCHED);
        assert_eq!(ask.options.len(), 1, "the move's door has one way through");
        assert_eq!(ask.options[0].answer, CopyAnswer::Copy);

        // A gesture nothing converts raises no sheet.
        let hand = RowMove::Seat { from: None, to: "mine".into(), index: None };
        assert!(ask_of_rows(&rows, &folders, &[], hand).is_none());
    }

    #[test]
    fn a_departure_writes_its_moved_log_on_the_placing_folder() {
        let rows = [linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        let book = rows[0].book().expect("the row is a book");
        let mut held = folder_shelf_at("shelf3", "f1", "Fiction/SciFi");
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
        let mut rows = vec![stored_at("b1", "/books/Fiction/SciFi/dune.md", "/store/b1.md", 9)];
        find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
        let shelves = vec![folder_shelf_at("shelf2", "f1", "Fiction")];
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
        let linked = vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(
            !bind_returned(&linked, &shelves, &mut fresh, "b1", "shelf2"),
            "a linked row is nobody's return — only the library's own copies bind"
        );
    }
}
