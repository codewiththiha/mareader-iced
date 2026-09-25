//! Taking a shelf apart: its rung dissolves and its books come up one level
//! inside the tree.

use library_core::book::{self as book_ops, Row};
use library_core::folder::{self as folder_ops, WatchedFolder};
use library_core::shelf::{self, ALL_SHELF, Shelf};

/// A shelf coming off the list, whole: the children re-hang on its parent,
/// the folder's own rungs re-hang the way its next scan would hang them — a
/// rung whose level is gone takes the nearest one still standing — the
/// folder lets the rung go in its map, and the books standing on the level
/// come up exactly one level too, onto the nearest rung the folder's tree
/// still stands on: a shelf a level was taken out from under is a shelf the
/// folder's next scan cannot see, and the reader never put its books on the
/// library's own top level. Returns the level the shelf hung from, for the
/// reader standing on it to step out to.
pub fn dismantle(
    shelves: &mut Vec<Shelf>,
    folders: &mut [WatchedFolder],
    rows: &mut Vec<Row>,
    shelf_id: &str,
) -> Option<String> {
    // One read of the shelf that is going answers every fact about it: the
    // level to step out to, which watched folder filed onto it, the rung it
    // stood on, and the books that come up with it.
    let gone = shelf::find(shelves, shelf_id)?;
    let stepped_out = gone.parent.clone().unwrap_or_else(|| ALL_SHELF.to_string());
    let detached = gone.kind.folder_id().map(str::to_string);
    let rung = gone.is_folder().then(|| gone.kind.rung().to_string());
    let stood_on = gone.books.clone();
    shelf::lift_children(shelves, shelf_id);
    shelves.retain(|one| one.id != shelf_id);
    if let Some(folder_id) = &detached {
        for (id, want) in shelf::rehang_moves(shelves, folder_id) {
            if let Some(moved) = shelf::find_mut(shelves, &id) {
                moved.parent = want;
            }
        }
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            folder.shelf_map.retain(|_, sid| sid != shelf_id);
        }
    }
    // Read after the removal, so the rung that went cannot answer for
    // itself.
    let home = match (&detached, &rung) {
        (Some(folder_id), Some(rung)) => shelf::rung_above(shelves, folder_id, rung),
        _ => None,
    };
    if let Some(home) = &home {
        for id in &stood_on {
            if let Some(seat) = shelf::find_mut(shelves, home) {
                shelf::shelf_add(seat, id);
            }
        }
    }
    book_ops::drop_dead_shelf_links(rows, shelves);
    Some(stepped_out)
}
