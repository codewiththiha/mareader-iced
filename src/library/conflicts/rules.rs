//! The questions asked of a level and its shelves: what collides, what the
//! sheet may offer, and the small writes an answer makes.

use library_core::book::{Book, Row, find_by_id, find_row};
use library_core::conflict::{Arrival, Placement, collide, next_name};
use library_core::folder::WatchedFolder;
use library_core::shelf::{self, Shelf};
use super::ask::{AskKind, ConflictAsk};
use crate::library::departure;

/// Every placing surface hands its placements through here before writing
/// anything, and applies the clean half at once: what collides waits on the
/// sheet, what does not lands now.
pub fn screen(
    rows: &[Row],
    shelves: &[Shelf],
    arrivals: Vec<Arrival>,
) -> (Vec<Arrival>, Vec<ConflictAsk>) {
    let mut clean = Vec::with_capacity(arrivals.len());
    let mut asks = Vec::new();
    for arrival in arrivals {
        match collide(rows, shelves, &arrival) {
            Some(existing_id) => {
                let existing_name = existing_name_of(rows, &existing_id, &arrival);
                asks.push(ConflictAsk::name_collision(arrival, existing_id, existing_name));
            }
            None => clean.push(arrival),
        }
    }
    (clean, asks)
}

/// One spelling, because the sheet prints this name in its heading and in
/// every button's sentence: a site that derived its own would eventually
/// disagree with the others about which book the question is about.
pub fn existing_name_of(rows: &[Row], existing_id: &str, arrival: &Arrival) -> String {
    find_row(rows, existing_id)
        .map(|row| row.display_name())
        .unwrap_or_else(|| arrival.name.clone())
}

/// A drag of four books is four arrivals, and a row that went between the
/// lift and the drop is not one of them. The name is read here rather than
/// by the rule, because the rule is pure and holds no rows.
pub fn moved_arrivals(
    rows: &[Row],
    row_ids: &[String],
    to: &str,
    index: Option<usize>,
    from: Option<&str>,
) -> Vec<Arrival> {
    row_ids
        .iter()
        .filter_map(|row_id| {
            let row = find_row(rows, row_id)?;
            let arrival = Arrival::moved(row_id.clone(), row.display_name(), to, index);
            Some(match from {
                Some(from) => arrival.leaving(from),
                None => arrival,
            })
        })
        .collect()
}

/// The import half of a screen is the import's own landing; nothing a move
/// raises carries a file, so the clean half of a move's screen is ids.
pub fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// The row being dragged is a read-at-place book an in-place folder placed,
/// and the row already on the level is one of the library's own stored
/// copies: neither side is the reader's to destroy, so the sheet offers
/// *make link* in place of *replace*.
pub fn link_shape(rows: &[Row], folders: &[WatchedFolder], ask: &ConflictAsk) -> bool {
    let Some(moved_id) = ask.arrival.moving.as_deref() else {
        return false;
    };
    let existing_is_a_copy = find_row(rows, &ask.existing_id)
        .and_then(|row| row.book())
        .is_some_and(|book| book.origin.is_stored());
    existing_is_a_copy
        && departure::converts_on_move(rows, folders, moved_id, &ask.arrival.shelf_id)
}

/// One function rather than a branch in the sheet and a second in the
/// answer, so the two cannot drift about which buttons a given arrival gets.
/// The kind decides first — the two-answer and the folder-merge sheets carry
/// their own fixed lists — and only the name question derives its offers
/// from the arrival and the shapes on the level.
pub fn offers_for(rows: &[Row], folders: &[WatchedFolder], ask: &ConflictAsk) -> &'static [Placement] {
    match &ask.kind {
        AskKind::Covered { .. } | AskKind::AlreadyHave => Placement::COVERED,
        AskKind::FolderMerge { .. } => Placement::FOLDER_MERGE,
        AskKind::NameCollision => {
            if ask.arrival.is_import() {
                Placement::FILE
            } else if link_shape(rows, folders, ask) {
                Placement::MOVE_KEEPING_BOTH
            } else {
                Placement::MOVE
            }
        }
    }
}

/// Read at the click rather than at the raise, in one place: the two answers
/// that mint a name must mint the same one for the same arrival.
pub fn minted_name(rows: &[Row], shelves: &[Shelf], ask: &ConflictAsk) -> String {
    next_name(rows, shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

/// The slot the displaced row holds on the level the arrival is landing on:
/// a replace seats the arrival where the reader pointed at, not at the end.
pub fn member_slot(shelves: &[Shelf], shelf_id: &str, row_id: &str) -> Option<usize> {
    shelf::find(shelves, shelf_id).and_then(|s| s.books.iter().position(|m| m == row_id))
}

/// The shelves a row is filed on, in shelf order: the memberships a merge's
/// survivor takes over and a replace's arrival inherits.
pub fn memberships(shelves: &[Shelf], book_id: &str) -> Vec<(String, String)> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect()
}

/// Whether the row that survives is the library's own copy OF the row that
/// dissolves. One question in one place, because the two answers that
/// dissolve a row — a merge into the copy and a link at it — both write the
/// folder's moved-out log on this condition and nothing else.
pub fn survivor_is_the_copy_of(rows: &[Row], survivor: &str, gone: &Book) -> bool {
    find_by_id(rows, survivor).is_some_and(|keep| keep.origin.is_store_copy_of(gone.path()))
}

/// File a row onto every shelf named, in one write: the shelves a dissolved
/// row held are the shelves its survivor takes over.
pub fn file_on_all(shelves: &mut [Shelf], row_id: &str, shelves_named: &[String]) {
    for one in shelves.iter_mut() {
        if shelves_named.contains(&one.id) {
            shelf::shelf_add(one, row_id);
        }
    }
}

/// The name a rename gives a row, whichever shape the row is: a book wears
/// it as the reader's own title, a link as the name it was minted with.
pub fn rename_row(rows: &mut [Row], row_id: &str, name: &str) -> bool {
    let Some(row) = library_core::book::find_row_mut(rows, row_id) else {
        return false;
    };
    match row {
        Row::Book(b) => {
            b.title = Some(name.to_string());
            b.title_locked = true;
        }
        Row::Link { name: own, .. } => *own = name.to_string(),
    }
    true
}
