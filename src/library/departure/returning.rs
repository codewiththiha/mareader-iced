//! The way back: where a returning copy lands, and the rungs a shelf keeps
//! once its ground goes.

use std::collections::HashSet;


use library_core::book::Row;
use library_core::folder::{self as folder_ops, WatchedFolder};
use library_core::shelf::{self, Shelf};

use super::leaving::{departing_book_ids, departing_sets};
use super::{ReturnPath, ShelfSeam};

/// The seam's anchor answers with the level that holds IT, a filing answers
/// with its target, and the root is the level that is not a shelf.
pub fn landing_level(
    shelves: &[Shelf],
    target: &Option<String>,
    seam: Option<&ShelfSeam>,
) -> Option<String> {
    match seam {
        Some(seam) => shelf::find(shelves, &seam.anchor_id).and_then(|s| s.parent.clone()),
        None => target.clone(),
    }
}

/// A rung of an in-place tree whose root covers the mover's ground directory
/// — the mover's own tree included, whose rungs are its first family. A
/// read-at-place shelf lives on the seat its directory stands on, so a move
/// inside the tree it belongs to can answer with the seat instead of a copy.
pub fn target_is_family(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    target: Option<&str>,
    ground: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Some(one) = shelf::find(shelves, target) else {
        return false;
    };
    let Some(folder_id) = one.kind.folder_id() else {
        return false;
    };
    folders.iter().any(|f| {
        f.id == folder_id
            && f.mode().reads_in_place()
            && folder_ops::rel_under(ground, &f.root).is_some()
    })
}

/// Two shapes, and which one a shelf owes is a fact about where its folder
/// stands. The folder's ROOT shelf whose ground a family tree covers at a
/// free rung goes home by the fold: `reclaim_rung`, which hangs the shelf on
/// the rung its directory names and folds the folder that was reading it
/// into the tree's ledger.
pub fn return_path(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Option<ReturnPath> {
    let one = shelf::find(shelves, shelf_id)?;
    let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
        return None;
    };
    let folder = folders
        .iter()
        .find(|f| &f.id == folder_id && f.mode().reads_in_place())?;
    let key = rel.clone().unwrap_or_default();
    if key.is_empty()
        && let Some((tree, tree_rel)) = shelf::family_for(folders, shelves, &folder.root)
    {
        return Some(ReturnPath::Reclaim {
            tree,
            gone: folder.id.clone(),
            rel: tree_rel,
        });
    }
    let seat = folder_ops::parent_key(&key)
        .and_then(|rung| folder.shelf_map.get(rung))
        .cloned();
    (one.parent != seat).then_some(ReturnPath::Reseat { seat })
}

/// The books read in place that a rung takes with it when its ground goes:
/// the folder placed them, their own file answers to this rung, and they
/// stand inside it. A book answering to a level that stays — a seat the tree
/// still names — is not one of them, and neither is a book the library
/// already stores.
pub fn books_the_rung_takes(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Vec<String> {
    let Some(rung) = shelf::find(shelves, shelf_id) else {
        return Vec::new();
    };
    let Some(folder) = rung.kind.folder_id().and_then(|id| folders.iter().find(|f| f.id == id))
    else {
        return Vec::new();
    };
    if !folder.mode().reads_in_place() {
        return Vec::new();
    }
    let (subtree, _) = departing_sets(shelves, &folder.id, shelf_id);
    let going: HashSet<String> = std::iter::once(shelf_id.to_string()).collect();
    departing_book_ids(rows, shelves, folder, &going, &subtree)
}

/// Every book the named shelves take with them, and none the removal is
/// taking anyway: a book going out of the library is not a book to copy
/// first.
pub fn shelf_books(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    going: &[String],
    purge: &[String],
) -> Vec<String> {
    let mut taking: Vec<String> = Vec::new();
    for id in going {
        for book in books_the_rung_takes(rows, shelves, folders, id) {
            if !purge.contains(&book) && !taking.contains(&book) {
                taking.push(book);
            }
        }
    }
    taking
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::departure::kit::{family_state, reading_folder, tree, tree_rows};
    use crate::library::departure::returning::books_the_rung_takes;
    use crate::library::departure::shelves::ask_of_rung;

    #[test]
    fn the_rung_question_counts_only_the_level_s_own_books() {
        let (shelves, rows) = (tree(), tree_rows());
        let folders = vec![reading_folder()];
        assert_eq!(
            books_the_rung_takes(&rows, &shelves, &folders, "fic"),
            vec!["mid".to_string()],
            "asked and answered through one walk, so the sheet and the copies cannot drift"
        );
        // The lowest rung: its own book goes; the guest on the root's ground,
        // the file no folder placed, and the library's own copy all stay out.
        assert_eq!(books_the_rung_takes(&rows, &shelves, &folders, "sf"), vec!["deep".to_string()]);
        // A reader's own shelf is nobody's ground, and an empty rung owes
        // nothing: neither is a question.
        assert!(ask_of_rung(&rows, &shelves, &folders, "mine").is_none());
        assert!(ask_of_rung(&rows, &shelves, &folders, "elsewhere").is_none());
    }

    #[test]
    fn a_displaced_folder_s_root_shelf_goes_home_by_the_fold() {
        let (shelves, folders) = family_state();
        match return_path(&shelves, &folders, "s3") {
            Some(ReturnPath::Reclaim { tree, gone, rel }) => {
                assert_eq!(tree, "f1", "the family the ground belongs to");
                assert_eq!(gone, "f3", "the folder that was reading it on its own");
                assert_eq!(rel, "Fiction/SciFi", "the rung its directory names");
            }
            path => panic!("the fold is a displaced root shelf's way home: {path:?}"),
        }
        assert!(target_is_family(&shelves, &folders, Some("fic"), "/books/Fiction/SciFi"));
        assert!(target_is_family(&shelves, &folders, Some("r"), "/books/Fiction/SciFi"));
        assert!(!target_is_family(&shelves, &folders, Some("mine"), "/books/Fiction/SciFi"));
        assert!(!target_is_family(&shelves, &folders, None, "/books/Fiction/SciFi"));
        assert!(!target_is_family(&shelves, &folders, Some("gone"), "/books/Fiction/SciFi"));
    }
}
