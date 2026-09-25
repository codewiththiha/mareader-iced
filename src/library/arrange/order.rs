//! The level's order: nesting a shelf, reordering to an anchor, placing a set
//! where the reader pointed, and the insert they all share.

use library_core::book::Row;
use library_core::shelf::{self, Shelf};

/// The cycle check is `library_core::shelf::reparent`'s, not the caller's:
/// a folder filed inside itself renders on no level and can never be
/// opened again, so the rule has to hold for every caller.
pub fn nest_shelf(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    shelf::reparent(shelves, folder_id, parent)
}

/// Books as memberships, folders as nestings, one persist for the batch.
/// No name question is asked: a nesting writes no membership, so nothing
/// arrives on the parent's level. A folder that cannot be nested there (it
/// would end up inside itself) is left where it is rather than failing the
/// batch.
pub fn nest_many(shelves: &mut [Shelf], folder_ids: &[String], parent: &str) -> bool {
    let mut moved = false;
    for folder_id in folder_ids {
        moved |= shelf::reparent(shelves, folder_id, Some(parent));
    }
    moved
}

/// What a drag onto a shelf row's edge commits — the sibling seam the list
/// layout draws — a reorder rather than a filing wherever the two shelves
/// already share a level, which is the common case.
pub fn reorder_shelves_to_anchor(
    shelves: &mut Vec<Shelf>,
    ids: &[String],
    anchor: &str,
    after: bool,
) -> bool {
    if ids.is_empty() {
        return false;
    }
    let parent = shelf::find(shelves, anchor).and_then(|shelf| shelf.parent.clone());
    let mut moved = false;
    for id in ids {
        if id == anchor || !shelf::reparent(shelves, id, parent.as_deref()) {
            continue;
        }
        let (Some(at), Some(mut ai)) = (
            shelves.iter().position(|shelf| shelf.id == *id),
            shelves.iter().position(|shelf| shelf.id == anchor),
        ) else {
            continue;
        };
        let item = shelves.remove(at);
        if at < ai {
            ai -= 1;
        }
        let at = if after { ai + 1 } else { ai };
        shelves.insert(at, item);
        moved = true;
    }
    moved
}

/// Lifted out and put back together rather than one at a time: each removal
/// shifts the tail left, so moving four in sequence would have the second
/// one's index mean something the first already changed.
pub fn reorder_root(rows: &mut Vec<Row>, row_ids: &[String], index: Option<usize>) -> bool {
    let mut lifted: Vec<(usize, Row)> = row_ids
        .iter()
        .filter_map(|row_id| {
            let was = rows.iter().position(|row| row.id() == row_id.as_str())?;
            Some((was, rows[was].clone()))
        })
        .collect();
    if lifted.is_empty() {
        return false;
    }
    let mut positions: Vec<usize> = lifted.iter().map(|(was, _)| *was).collect();
    positions.sort_unstable_by_key(|was| std::cmp::Reverse(*was));
    for was in positions {
        rows.remove(was);
    }
    let shift = index.map_or(0, |at| lifted.iter().filter(|(was, _)| *was < at).count());
    lifted.sort_by_key(|(_, row)| {
        row_ids
            .iter()
            .position(|row_id| row_id.as_str() == row.id())
            .unwrap_or(usize::MAX)
    });
    insert_many(rows, lifted.into_iter().map(|(_, row)| row), index, shift);
    true
}

/// [`shelf::place`] for one book, this for a drag: the same two steps, but
/// per-book placement would leave each index counting a list the last one
/// already changed.
pub(super) fn place_many(members: &mut Vec<String>, book_ids: &[String], index: Option<usize>) {
    let shift = index.map_or(0, |at| {
        book_ids
            .iter()
            .filter(|book_id| {
                members
                    .iter()
                    .position(|member| member.as_str() == book_id.as_str())
                    .is_some_and(|was| was < at)
            })
            .count()
    });
    for book_id in book_ids {
        shelf::forget(members, book_id);
    }
    insert_many(members, book_ids.iter().cloned(), index, shift);
}

/// Each one after the last rather than each one at the same place, which
/// would put them back reversed.
pub(super) fn insert_many<T>(
    list: &mut Vec<T>,
    items: impl Iterator<Item = T>,
    index: Option<usize>,
    shift: usize,
) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
}
