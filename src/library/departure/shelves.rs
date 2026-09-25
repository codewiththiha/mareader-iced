//! A whole level leaving: the rungs its books stand on, and one spelling of
//! the sheet for a shelf's move, its rung and its removal.

use std::collections::HashSet;


use library_core::book::{duplicate_title, Row};
use library_core::folder::{self as folder_ops, WatchedFolder};
use library_core::paths::dir_label;
use library_core::shelf::{self, Shelf};
use library_core::text::plural;

use super::leaving::{departing_book_ids, departing_sets, ground_label_of};
use super::returning::{books_the_rung_takes, landing_level, return_path, shelf_books, target_is_family};
use super::{CopyAnswer, CopyAsk, CopyOption, CopyWork, ReturnPath, ShelfSeam, UNTOUCHED, copying, doing};

/// One departing shelf's whole fact, read the moment the answer lands: the
/// rung it is, the books it owes, the rungs and the subtree that go free of
/// the folder with it.
#[derive(Clone, Debug)]
pub struct ShelfDeparture {
    pub id: String,
    pub name: String,
    pub folder_id: String,
    pub rel: String,
    pub rungs: HashSet<String>,
    pub subtree: HashSet<String>,
    pub books: Vec<String>,
}

/// The departures a shelf move owes, in the gesture's own order. The rule is
/// asked again per shelf, because the sheet was up while the library went on
/// living: a shelf that no longer owes a departure — or that cannot nest
/// where the gesture was going — is skipped silently.
pub fn shelf_departures(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    departing: &[String],
    level: Option<&str>,
) -> Vec<ShelfDeparture> {
    let mut out: Vec<ShelfDeparture> = Vec::new();
    for id in departing {
        let Some(one) = shelf::find(shelves, id) else {
            continue;
        };
        let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
            continue;
        };
        let Some(folder) = folders.iter().find(|f| &f.id == folder_id) else {
            continue;
        };
        if !shelf::departs_on_move(shelves, folders, id, level) {
            continue;
        }
        if let Some(parent) = level
            && !shelf::can_nest(shelves, id, parent)
        {
            continue;
        }
        let (subtree, rungs) = departing_sets(shelves, folder_id, id);
        let books = departing_book_ids(rows, shelves, folder, &rungs, &subtree);
        out.push(ShelfDeparture {
            id: id.clone(),
            name: one.name.clone(),
            folder_id: folder_id.clone(),
            rel: rel.clone().unwrap_or_default(),
            rungs,
            subtree,
            books,
        });
    }
    out
}

/// A copy takes the level's next free name, so the folder's own name stays
/// free for the original.
pub fn free_name(name: &str, promised: &mut HashSet<String>) -> String {
    let free = duplicate_title(name, promised);
    promised.insert(free.clone());
    free
}

/// The copy names as the sheet lists them.
fn listed(names: &[String]) -> String {
    names.iter().map(|name| format!("“{name}”")).collect::<Vec<_>>().join(", ")
}

/// A shelf move's ask: one spelling for the three hand-moves, because a
/// drag, a bulk filing and a sibling reorder are one rule and one question.
/// The copies wear the level's next free names, and a drop inside the
/// mover's own family offers the way home instead of a copy. `None` when no
/// departing shelf is a folder's rung at all — the gesture then is nobody's
/// departure and lands clean.
pub fn ask_of_shelf(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) -> Option<CopyAsk> {
    let level = landing_level(shelves, &target, seam.as_ref());
    let mut promised: HashSet<String> = shelf::children_of(shelves, level.as_deref())
        .into_iter()
        .map(|s| s.name.clone())
        .collect();
    let mut names: Vec<String> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    let mut returns: Vec<(String, ReturnPath)> = Vec::new();
    // The first folder that placed one of the books names the sheet.
    let mut folder = String::new();
    let mut count = 0usize;
    for id in &departing {
        let Some(one) = shelf::find(shelves, id) else {
            continue;
        };
        let Some(folder_id) = one.kind.folder_id() else {
            continue;
        };
        let Some(placing) = folders.iter().find(|f| f.id == folder_id) else {
            continue;
        };
        let (subtree, rungs) = departing_sets(shelves, folder_id, id);
        count += departing_book_ids(rows, shelves, placing, &rungs, &subtree).len();
        if folder.is_empty() {
            folder = dir_label(&placing.root);
        }
        // Offered only for a drop inside the mover's FAMILY: anywhere else
        // the copy is the only honest answer, because there is no tree to
        // put the shelf back into.
        let ground = folder_ops::dir_of_rung(&placing.root, one.kind.rung());
        if target_is_family(shelves, folders, level.as_deref(), &ground)
            && let Some(path) = return_path(shelves, folders, id)
        {
            returns.push((id.clone(), path));
        }
        copies.push(free_name(&one.name, &mut promised));
        names.push(one.name.clone());
    }
    if names.is_empty() {
        return None;
    }
    let one = names.len() == 1;
    let cost = plural(count, "book", "books");
    let subject = match (one, count) {
        (true, 0) => format!("“{}” — nothing read in place", names[0]),
        (true, _) => format!("“{}” — {cost} from “{folder}”", names[0]),
        (false, 0) => plural(names.len(), "shelf", "shelves"),
        (false, _) => format!(
            "{} — {cost} read in place",
            plural(names.len(), "shelf", "shelves")
        ),
    };
    let lines = match (one, count) {
        (true, 0) => vec![
            "Nothing on it is read in place, so no file is copied.".to_string(),
            UNTOUCHED.to_string(),
        ],
        (true, _) => vec![
            format!("Moving it out stores its {cost}; the copy lands as “{}”.", copies[0]),
            UNTOUCHED.to_string(),
        ],
        (false, 0) => vec![
            "Nothing on them is read in place, so no file is copied.".to_string(),
            UNTOUCHED.to_string(),
        ],
        (false, _) => vec![
            format!(
                "Moving them out stores their {cost}; the copies land as {}.",
                listed(&copies)
            ),
            UNTOUCHED.to_string(),
        ],
    };
    let mut options = vec![copying("Copy and move")];
    if !returns.is_empty() {
        options.push(CopyOption {
            label: if returns.len() == 1 {
                "Put it back in its place".to_string()
            } else {
                "Put them back in their places".to_string()
            },
            title: "Return each shelf to the place its folder names; nothing is copied"
                .to_string(),
            answer: CopyAnswer::WithoutCopies,
            primary: false,
        });
    }
    Some(CopyAsk {
        action: doing(names.len(), "Move shelf", "Move shelves"),
        subject,
        lines,
        options,
        work: CopyWork::Shelf { ids: departing, target, seam, returns },
    })
}

/// A rung of a read-at-place tree coming apart: the level's own books leave
/// the ground that made them, so they become the library's own before it
/// goes. `None` when nothing on it is a question — an empty rung, a reader's
/// own shelf, a level the library already stores.
pub fn ask_of_rung(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Option<CopyAsk> {
    let books = books_the_rung_takes(rows, shelves, folders, shelf_id);
    if books.is_empty() {
        return None;
    }
    let folder = ground_label_of(rows, folders, &books)?;
    let rung = shelf::find(shelves, shelf_id)?;
    let home = match rung
        .kind
        .folder_id()
        .and_then(|folder_id| shelf::rung_above(shelves, folder_id, rung.kind.rung()))
        .and_then(|seat| shelf::find(shelves, &seat))
    {
        Some(seat) => format!("“{}”", seat.name),
        None => "the library's top level".to_string(),
    };
    let cost = plural(books.len(), "book", "books");
    let line = if books.len() == 1 {
        format!("The book read in place here becomes a copy and comes up to {home}.")
    } else {
        format!(
            "The {} books read in place here become copies and come up to {home}.",
            books.len()
        )
    };
    Some(CopyAsk {
        action: "Take shelf apart".to_string(),
        subject: format!("“{}” — {cost} from “{folder}”", rung.name),
        lines: vec![line, UNTOUCHED.to_string()],
        options: vec![copying("Copy and take apart")],
        work: CopyWork::Rung { id: shelf_id.to_string() },
    })
}

/// A shelf coming off the list with books read in place on it: the same
/// copy, and the one removal that changes nothing — the folder's next
/// import makes the level again.
pub fn ask_of_removal(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    purge: &[String],
    going: &[String],
) -> Option<CopyAsk> {
    let taking = shelf_books(rows, shelves, folders, going, purge);
    if taking.is_empty() {
        return None;
    }
    let folder = ground_label_of(rows, folders, &taking)?;
    let names: Vec<String> =
        going.iter().filter_map(|id| shelf::find(shelves, id).map(|s| s.name.clone())).collect();
    let one = names.len() == 1;
    let again = if one {
        format!("Or let “{folder}” make it again.")
    } else {
        "Or let the folders make them again.".to_string()
    };
    let label = if one {
        format!("Let “{folder}” make it again")
    } else {
        "Let the folders make them again".to_string()
    };
    let kept = plural(
        taking.len(),
        "Read in place, so removing it stores a copy.",
        "Read in place, so removing them stores copies.",
    );
    Some(CopyAsk {
        action: doing(names.len(), "Remove shelf", "Remove shelves"),
        subject: plural(names.len(), "shelf", "shelves"),
        lines: vec![kept, again],
        options: vec![
            copying("Copy and remove"),
            CopyOption {
                label,
                title: "Take the shelf off the list; its books read in place where they are"
                    .to_string(),
                answer: CopyAnswer::WithoutCopies,
                primary: false,
            },
        ],
        work: CopyWork::Removal { purge: purge.to_vec(), shelves: going.to_vec() },
    })
}

#[cfg(test)]
mod tests {
    use crate::library::departure::kit::{reading_folder, tree, tree_rows};
    use crate::library::departure::shelves::ask_of_rung;

    #[test]
    fn a_level_the_library_already_stores_comes_apart_without_a_question() {
        let mut shelves = tree();
        shelves
            .iter_mut()
            .find(|s| s.id == "sf")
            .expect("the lowest rung")
            .books = vec!["loose".to_string(), "kept".to_string()];
        let rows = tree_rows();
        let folders = vec![reading_folder()];
        // `kept` is the library's own copy already and `loose` is a file no
        // folder placed here: neither is a book to make a copy of.
        assert!(ask_of_rung(&rows, &shelves, &folders, "sf").is_none());
    }
}
