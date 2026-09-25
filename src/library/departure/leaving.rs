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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use crate::library::departure::CopyAnswer;
    use crate::library::departure::kit::{nested, reading_folder, tree, tree_rows};
    use crate::library::departure::leaving::ask_of_rows;
    use crate::library::departure::leaving::converting_rows;
    use crate::library::departure::leaving::converts_on_move;
    use library_core::shelf::ALL_SHELF;
    use library_core::testkit;

    #[test]
    fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
        let folders = vec![nested(7)];
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
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
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
        // Re-ordering the books a folder placed, on the rung it placed them
        // on, is the folder's own business.
        assert!(!converts_on_move(&rows, &folders, "b1", "shelf3"));
    }

    #[test]
    fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
        let folders = vec![nested(7)];
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(converts_on_move(&rows, &folders, "b1", ALL_SHELF));
        assert!(converts_on_move(&rows, &folders, "b1", "mine"));
    }

    #[test]
    fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
        let mut folder = nested(7);
        folder.shelf_map.remove("Fiction/SciFi");
        let folders = vec![folder];
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
        assert!(converts_on_move(&rows, &folders, "b1", "shelf3"));
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
        let mut folder = nested(7);
        folder.opts.groups = false;
        folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
        let folders = vec![folder];
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
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
            testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7),
            testkit::stored_row("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
            testkit::row_at_n("b3", "/elsewhere/loose.md", 11),
        ];

        assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "b2", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "b3", "shelf2"));
        assert!(!converts_on_move(&rows, &folders, "gone", "shelf2"));
    }

    #[test]
    fn the_ask_names_the_ground_and_the_cost() {
        let folders = vec![nested(7)];
        let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
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
    fn a_departing_rung_carries_the_books_standing_on_the_rungs_it_takes() {
        let shelves = tree();
        let folder = reading_folder();
        let (subtree, rungs) = departing_sets(&shelves, "f1", "sf");
        assert!(subtree.contains("sf"));
        assert!(rungs.contains("sf"));
        let ids = departing_book_ids(&tree_rows(), &shelves, &folder, &rungs, &subtree);
        assert_eq!(ids, vec!["deep".to_string()], "only the book whose OWN rung is the one leaving");
    }

    #[test]
    fn a_book_shown_on_a_departing_rung_keeps_its_link_when_its_ground_stays() {
        let shelves = tree();
        let folder = reading_folder();
        // "shown2" is a member of "sf" below it, but its address stands on
        // the ROOT rung, which is not departing: the tree keeps answering
        // for it, and a copy of a book whose rung stays is a copy the reader
        // never asked for.
        let (subtree, rungs) = departing_sets(&shelves, "f1", "fic");
        let ids = departing_book_ids(&tree_rows(), &shelves, &folder, &rungs, &subtree);
        assert!(ids.contains(&"mid".to_string()), "the book of the level itself goes");
        assert!(!ids.contains(&"shown2".to_string()), "the guest of a departing rung stays linked");
        assert!(
            ids.contains(&"deep".to_string()),
            "a MOVE takes the whole subtree the directory rides with; the take-APART below \
             pays for the level's own book alone"
        );
    }
}
