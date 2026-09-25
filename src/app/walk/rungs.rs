//! The rungs and shelves a walk writes through: one spelling per minted rung,
//! the folded member's own rungs, and the flattening a re-import asks for.
use library_core::paths;
use library_core::folder::WatchedFolder;
use library_core::shelf::{self, Shelf};
use std::collections::HashSet;

/// One spelling for the shelf chain a folder walk mints through — the books
/// it adds and the rungs between them — so a rung cannot be minted twice
/// under two spellings of its own name. Rungs already in the map are
/// reused, which is what makes a rescan continue the tree instead of
/// growing a twin beside it.
pub(in crate::app) fn chain_for(
    folder: &mut WatchedFolder,
    key: &str,
    stamp: u64,
    planned_root: Option<&str>,
    root: &str,
    new_shelves: &mut Vec<shelf::Shelf>,
) -> String {
    let folder_id = folder.id.clone();
    let root = root.to_string();
    let planned = planned_root.map(str::to_string);
    folder.shelf_chain_for(
        key,
        |_| library_core::id::next_shelf_id(stamp),
        move |rung| {
            if rung.is_empty() {
                planned.clone().unwrap_or_else(|| paths::dir_label(&root))
            } else {
                rung.rsplit('/').next().unwrap_or(rung).to_string()
            }
        },
        |rung, id, name, parent| {
            let rel = (!rung.is_empty()).then(|| rung.to_string());
            new_shelves.push(shelf::Shelf::folder_shelf(id, name, &folder_id, rel, parent));
        },
    )
}

/// The rungs a folded member brings: every shelf the folded folder owns,
/// keyed the way the receiving tree keys its own — the rung the member's
/// directory names, and the ones below it.
pub(in crate::app) fn member_rungs(shelves: &[Shelf], gone_id: &str, rel: &str) -> Vec<(String, String)> {
    shelves
        .iter()
        .filter(|s| s.kind.folder_id() == Some(gone_id))
        .filter_map(|s| {
            let library_core::shelf::ShelfKind::Folder { rel: own, .. } = &s.kind else {
                return None;
            };
            let own = own.as_deref().unwrap_or("");
            let key =
                if own.is_empty() { rel.to_string() } else { format!("{rel}/{own}") };
            Some((key, s.id.clone()))
        })
        .collect()
}

/// The map's key as the shelf's own rel: the root's empty key is no rel.
pub(in crate::app) fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// The minted rungs join the list: one push per shelf no id already holds.
pub(in crate::app) fn page_into(shelves: &mut Vec<Shelf>, minted: Vec<Shelf>) {
    for made in minted {
        if !shelves.iter().any(|s| s.id == made.id) {
            shelves.push(made);
        }
    }
}

/// The whole of one tree's books onto `seat`, and the shelves the one-shelf
/// answer has no place for taken out: the flattening a re-import asks for,
/// and the one an adopting tree takes a member in by. Every rung of `from`
/// goes except the one that is `seat`. Books move, readers do not: a shelf
/// the reader made inside one comes up to `seat` with its books.
pub(in crate::app) fn flatten_rungs(shelves: &mut Vec<Shelf>, from: &str, seat: &str) {
    let own: Vec<String> =
        shelf::rungs_of(shelves, from).into_values().map(String::from).collect();
    let going: HashSet<String> = own.iter().filter(|id| id.as_str() != seat).cloned().collect();
    // A set rather than a growing list: the whole tree's books pass through
    // here, and membership was a scan per book.
    let mut held: HashSet<String> = HashSet::new();
    for rung in &own {
        let Some(one) = shelf::find(shelves, rung) else {
            continue;
        };
        held.extend(one.books.iter().cloned());
    }
    if let Some(one) = shelf::find_mut(shelves, seat) {
        for book_id in held {
            shelf::shelf_add(one, &book_id);
        }
    }
    for one in shelves.iter_mut() {
        if one.parent.as_deref().is_some_and(|parent| going.contains(parent)) {
            one.parent = Some(seat.to_string());
        }
    }
    shelves.retain(|one| !going.contains(&one.id));
}

/// Put the folder row back, by id. One place, because the ledger is the
/// part of the library that must never be written half-updated: a `placed`
/// set that lost an entry re-adds a book the reader already filed.
pub(in crate::app) fn write_folder_row(folders: &mut Vec<WatchedFolder>, folder: WatchedFolder) {
    match folders.iter().position(|each| each.id == folder.id) {
        Some(at) => folders[at] = folder,
        None => folders.push(folder),
    }
}

