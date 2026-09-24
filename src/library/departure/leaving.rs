//! What a move takes into the store and the ground it leaves: the rows a
//! gesture converts, and the sheet that names the copy they cost.

use std::collections::HashSet;


use library_core::book::{book_rows, find_row, Origin, Row};
use library_core::folder::WatchedFolder;
use library_core::paths::dir_label;
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::text::plural;

use super::{CopyAsk, CopyWork, RowMove, UNTOUCHED, copying};

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
    let folder = ground_label_of(rows, folders, ids)?;
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
pub(super) fn ground_label_of(rows: &[Row], folders: &[WatchedFolder], ids: &[String]) -> Option<String> {
    ids.iter().find_map(|id| {
        let fp = find_row(rows, id).and_then(|row| row.book()).map(|b| b.fp)?;
        let folder =
            folders.iter().find(|f| f.mode().reads_in_place() && f.placed.contains(&fp))?;
        Some(dir_label(&folder.root))
    })
}

/// The subtree is every shelf below the one the hand named — it rides with
/// the copy the way a directory's tree rides with the directory — and the
/// rungs are the folder's OWN shelves inside that subtree, which go free of
/// the folder's map.
pub fn departing_sets(
    shelves: &[Shelf],
    folder_id: &str,
    top_id: &str,
) -> (HashSet<String>, HashSet<String>) {
    let root = [top_id.to_string()];
    let subtree: HashSet<String> =
        std::iter::once(top_id.to_string()).chain(shelf::subtree_ids(shelves, &root)).collect();
    let rungs: HashSet<String> = subtree
        .iter()
        .filter(|id| {
            shelf::find(shelves, id).is_some_and(|s| s.kind.folder_id() == Some(folder_id))
        })
        .cloned()
        .collect();
    (subtree, rungs)
}

/// The linked books the folder placed whose own rung is one of the departing
/// ones, and who are members of the departing subtree. The two conditions
/// each rule out a real shape: a book whose rung stands OUTSIDE the subtree,
/// and a book the folder placed but no longer holds.
pub fn departing_book_ids(
    rows: &[Row],
    shelves: &[Shelf],
    folder: &WatchedFolder,
    rungs: &HashSet<String>,
    subtree: &HashSet<String>,
) -> Vec<String> {
    book_rows(rows)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && folder.placed.contains(&b.fp))
        .filter(|b| folder.rungs_for(b.path()).0.is_some_and(|rung| rungs.contains(rung)))
        .filter(|b| shelf::containing(shelves, &b.id).iter().any(|s| subtree.contains(&s.id)))
        .map(|b| b.id.clone())
        .collect()
}
