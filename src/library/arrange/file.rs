//! Filing a book onto a shelf, lifting it off one, and the moves that carry a
//! whole set with them.

use super::order::{place_many, reorder_root};
use library_core::book::Row;
use library_core::shelf::{self, ALL_SHELF, Shelf};

/// Membership only, so the same rule covers a bulk filing and a drag's
/// second answer: nothing here touches a filesystem, and a book already on
/// the shelf is not moved to the end for being named twice.
pub fn file_many(shelves: &mut [Shelf], book_ids: &[String], shelf_id: &str) -> bool {
    if book_ids.is_empty() {
        return false;
    }
    let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
        return false;
    };
    let before = shelf.books.len();
    for book_id in book_ids {
        shelf::shelf_add(shelf, book_id);
    }
    shelf.books.len() != before
}

/// The books stay in the library — a shelf holds ids and never held a byte
/// — and the folder ledger is untouched: this is one shelf letting go.
pub fn unfile_books(shelves: &mut [Shelf], book_ids: &[String], shelf_id: &str) -> bool {
    if book_ids.is_empty() {
        return false;
    }
    let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
        return false;
    };
    let mut moved = false;
    for book_id in book_ids {
        moved |= shelf::forget(&mut shelf.books, book_id);
    }
    moved
}

/// A whole drag in one call, whatever it held. The blob is written once for
/// it — a reader who closes the window halfway through a move should find
/// all of it or none of it — so the caller persists on the answer.
///
/// The root has no member list, so a move there is a lift out of every
/// shelf plus a place in the library's own order; a move onto a shelf is a
/// lift off the source and a place at the slot the drop named.
pub fn move_many_to_shelf(
    shelves: &mut [Shelf],
    rows: &mut Vec<Row>,
    book_ids: &[String],
    from: Option<&str>,
    to: &str,
    index: Option<usize>,
) -> bool {
    if book_ids.is_empty() {
        return false;
    }
    if to == ALL_SHELF {
        let mut moved = false;
        if from.is_some() {
            for book_id in book_ids {
                let held = shelves.iter().any(|shelf| shelf.books.iter().any(|member| member == book_id));
                if held {
                    shelf::forget_everywhere(shelves, book_id);
                    moved = true;
                }
            }
        }
        moved |= reorder_root(rows, book_ids, index);
        return moved;
    }
    let mut moved = false;
    if let Some(from) = from.filter(|id| *id != to)
        && let Some(shelf) = shelf::find_mut(shelves, from)
    {
        for book_id in book_ids {
            moved |= shelf::forget(&mut shelf.books, book_id);
        }
    }
    if let Some(shelf) = shelf::find_mut(shelves, to) {
        let before = shelf.books.clone();
        place_many(&mut shelf.books, book_ids, index);
        moved |= shelf.books != before;
    }
    moved
}
