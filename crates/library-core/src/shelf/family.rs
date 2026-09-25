//! The shelf-and-folder family rules: which in-place tree a directory belongs
//! to, and which shelf moves are departures from their folder's ground rather
//! than plain re-hangs.

use super::{ancestors, find, Shelf, ShelfKind};

/// The family a ground directory belongs to but is not standing in —
/// [`crate::governance::Governance::family`] with the folder and shelf lists
/// passed in.
pub fn family_for(
    folders: &[crate::folder::WatchedFolder],
    shelves: &[Shelf],
    ground: &str,
) -> Option<(String, String)> {
    crate::governance::Governance::new(folders, shelves).family(ground)
}

/// Whether moving this shelf under `parent` is a departure that owes a copy:
/// a shelf cut from a read-at-place folder, leaving the seat the folder's
/// ledger names for its rung.
///
/// Each negative is the book departure's rule one level up: a shelf that is
/// nobody's rung is the reader's own, a copying folder's shelf is the
/// library's own already, and a re-order on the current seat moves nothing.
pub fn departs_on_move(
    shelves: &[Shelf],
    folders: &[crate::folder::WatchedFolder],
    shelf_id: &str,
    parent: Option<&str>,
) -> bool {
    let Some(shelf) = find(shelves, shelf_id) else {
        return false;
    };
    let ShelfKind::Folder { folder_id, .. } = &shelf.kind else {
        return false;
    };
    let Some(folder) = folders
        .iter()
        .find(|f| &f.id == folder_id && f.mode().reads_in_place())
    else {
        return false;
    };
    // A re-order on the current seat is the folder's own business.
    if shelf.parent.as_deref() == parent {
        return false;
    }
    let key = shelf.kind.rung();
    let seat = crate::folder::parent_key(key).and_then(|rung| folder.shelf_map.get(rung));
    seat.map(String::as_str) != parent
}

/// Split a batch of requested shelf moves into the half that lands as it is
/// and the half that owes the departure's ask, in the order the gesture gave.
/// A departing shelf nested inside another departing shelf rides along rather
/// than asking twice.
pub fn departing_moves(
    shelves: &[Shelf],
    folders: &[crate::folder::WatchedFolder],
    ids: &[String],
    parent: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let mut clean: Vec<String> = Vec::new();
    let mut departing: Vec<String> = Vec::new();
    for id in ids {
        if departs_on_move(shelves, folders, id, parent) {
            departing.push(id.clone());
        } else {
            clean.push(id.clone());
        }
    }
    // Riders collected before the retain: filtering against `departing`
    // while the retain holds it would be two borrows of one list.
    let riders: Vec<String> = departing
        .iter()
        .filter(|id| {
            ancestors(shelves, id.as_str())
                .iter()
                .any(|each| {
                    each.id != id.as_str() && departing.iter().any(|outer| outer == &each.id)
                })
        })
        .cloned()
        .collect();
    departing.retain(|id| !riders.contains(id));
    (clean, departing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::kit::{cut, in_place_tree};
    use std::collections::{BTreeMap, HashSet};
    use crate::folder::{FolderOpts, WatchedFolder};
    use crate::shelf::family::departing_moves;
    use crate::shelf::family::departs_on_move;
    use crate::shelf::family::family_for;
    use crate::tracking::TrackingTree;

    #[test]
    fn a_rung_of_a_reading_folder_departs_whatever_it_leaves_for() {
        let (shelves, folders) = in_place_tree();
        // Another rung of the same tree is still a departure: what ties a
        // rung to its folder is the seat its directory stands on, not
        // membership of the folder's shelf tree.
        assert!(departs_on_move(&shelves, &folders, "sf", Some("r")));
        assert!(departs_on_move(&shelves, &folders, "sf", Some("mine")));
        assert!(departs_on_move(&shelves, &folders, "sf", None));
        assert!(departs_on_move(&shelves, &folders, "r", Some("mine")));
    }

    #[test]
    fn a_reorder_on_the_seat_and_a_return_to_it_copy_nothing() {
        let (shelves, folders) = in_place_tree();
        // A re-order among siblings is the same ground: the cheapest drag in
        // the library must stay the cheapest.
        assert!(!departs_on_move(&shelves, &folders, "sf", Some("fic")));
        assert!(!departs_on_move(&shelves, &folders, "r", None));
        // A rung an older blob carries off its seat comes back as a return:
        // no copy is owed.
        let mut off_seat = shelves.clone();
        off_seat
            .iter_mut()
            .find(|s| s.id == "sf")
            .unwrap()
            .parent = Some("mine".to_string());
        assert!(!departs_on_move(&off_seat, &folders, "sf", Some("fic")));
    }

    #[test]
    fn a_virtual_shelf_and_a_copying_folder_s_move_freely() {
        let (mut shelves, mut folders) = in_place_tree();
        shelves.push(cut("stored", "f2", None, None));
        folders.push(WatchedFolder {
            id: "f2".into(),
            root: "/dvds".into(),
            opts: FolderOpts {
                in_place: false,
                ..FolderOpts::default()
            },
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([(String::new(), "stored".to_string())]),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
            shapes: crate::shape::ShapeTree::default(),
        });
        assert!(!departs_on_move(&shelves, &folders, "mine", Some("r")));
        // A copying folder's shelf is the library's own: no ledger waits on
        // its rung.
        assert!(!departs_on_move(&shelves, &folders, "stored", Some("mine")));
        assert!(!departs_on_move(&shelves, &folders, "gone", Some("r")));
        let orphan = vec![cut("orphan", "f9", None, None)];
        assert!(!departs_on_move(&orphan, &folders, "orphan", Some("mine")));
    }

    #[test]
    fn a_departing_shelf_inside_another_departing_one_rides_with_it() {
        let (shelves, folders) = in_place_tree();
        let ids: Vec<String> = ["fic", "sf", "mine"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let (clean, departing) = departing_moves(&shelves, &folders, &ids, None);
        assert_eq!(
            departing,
            vec!["fic".to_string()],
            "sf rides inside fic's copy and asks nothing of its own"
        );
        assert_eq!(clean, vec!["mine".to_string()]);
    }

    #[test]
    fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
        let (shelves, folders) = in_place_tree();
        // Ground deep in f1's tree whose rung the map does not name: the
        // family is f1.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/SciFi"),
            None
        );
        assert_eq!(family_for(&folders, &shelves, "/books"), None);
        let mut dead_slot = folders.clone();
        dead_slot[0]
            .shelf_map
            .insert("Fiction/SciFi".to_string(), "gone".to_string());
        assert_eq!(
            family_for(&dead_slot, &shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
        let mut deeper = folders[0].clone();
        deeper.id = "f2".into();
        deeper.root = "/books/Fiction".into();
        deeper.shelf_map.clear();
        deeper
            .shelf_map
            .insert(String::new(), "deeper_root".to_string());
        let mut both_shelves = shelves.clone();
        both_shelves.push(cut("deeper_root", "f2", None, None));
        let mut outer = folders[0].clone();
        outer.shelf_map.remove("Fiction/SciFi");
        let both = vec![outer, deeper];
        assert_eq!(
            family_for(&both, &both_shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
    }

    #[test]
    fn a_taken_out_root_leaves_no_family_behind_it() {
        let (shelves, folders) = in_place_tree();
        // A rung a removal emptied is a home to come back to while the tree's
        // root stands.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        // With the root shelf gone the tree is out of the library and the
        // ground under it is a start of its own.
        let taken_out: Vec<Shelf> = shelves.iter().filter(|s| s.id != "r").cloned().collect();
        assert_eq!(family_for(&folders, &taken_out, "/books/Fiction/Deleted"), None);
    }
}
