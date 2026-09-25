//! The shelf tree: which shelves hang under which, and the moves that
//! re-hang them — the cycle-checked nest, the subtree a departing shelf takes
//! with it, and the pass that puts a watched folder's rungs back on the seats
//! their directories name.

use super::{find, Shelf, ShelfKind};

/// The shelves filed directly inside `parent_id`; `None` asks for the root
/// level. Direct children only: a level is a page, and flattening the subtree
/// would show shelves the reader has not opened.
pub fn children_of<'a>(shelves: &'a [Shelf], parent_id: Option<&str>) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.parent.as_deref() == parent_id)
        .collect()
}

/// The chain above `id`, root first, excluding `id` — what a breadcrumb
/// walks. Stops on a shelf it has already seen: a breadcrumb that looped
/// would hang the render.
pub fn ancestors<'a>(shelves: &'a [Shelf], id: &str) -> Vec<&'a Shelf> {
    let mut chain: Vec<&'a Shelf> = Vec::new();
    let mut next = find(shelves, id).and_then(|s| s.parent.as_deref());
    while let Some(parent_id) = next {
        if chain.iter().any(|seen| seen.id == parent_id) {
            break;
        }
        let Some(parent) = shelves.iter().find(|s| s.id == parent_id) else {
            break;
        };
        chain.push(parent);
        next = parent.parent.as_deref();
    }
    chain.reverse();
    chain
}

/// Whether `folder_id` may be filed inside `target_id`. Both refusals are the
/// same failure — a shelf inside itself: the drop on itself, and the drop
/// into one of its own descendants.
pub fn can_nest(shelves: &[Shelf], folder_id: &str, target_id: &str) -> bool {
    if folder_id == target_id {
        return false;
    }
    let mut current = Some(target_id.to_string());
    // Bounded by the list length: a blob that already carries a cycle would
    // otherwise spin here forever.
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return true;
        };
        if id == folder_id {
            return false;
        }
        current = shelves
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.parent.clone());
    }
    false
}

/// File `folder_id` inside `parent`, or at the root when `parent` is `None`.
/// True when the shelf was found and the graph allows the move; a refused drop
/// leaves the list untouched, so the caller answers it by doing nothing.
///
/// Writes the `manual_parent` mark when the new place differs from the seat
/// the disk names — the mark that stops the next re-hang from undoing a hand
/// the disk disagrees with — and clears it when a hand puts the shelf back.
pub fn reparent(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    if let Some(target) = parent
        && !can_nest(shelves, folder_id, target)
    {
        return false;
    }
    // The seat is read before the write borrow: which place the disk names
    // is a fact about the list as it stands.
    let seat = folder_seat(shelves, folder_id);
    let Some(shelf) = shelves.iter_mut().find(|s| s.id == folder_id) else {
        return false;
    };
    if let Some(seat) = seat {
        shelf.manual_parent = seat.as_deref() != parent;
    }
    shelf.parent = parent.map(str::to_string);
    true
}

/// The folder's rungs: `rel` key to shelf id for every shelf of one folder.
/// One map for the seat question, the re-hang and the re-import shape, so the
/// callers cannot drift.
pub fn rungs_of<'a>(
    shelves: &'a [Shelf],
    folder_id: &str,
) -> std::collections::HashMap<String, &'a str> {
    shelves
        .iter()
        .filter_map(|s| match &s.kind {
            ShelfKind::Folder {
                folder_id: owner,
                rel,
            } if owner == folder_id => Some((rel.clone().unwrap_or_default(), s.id.as_str())),
            _ => None,
        })
        .collect()
}

/// The nearest rung standing above `key`. A level taken apart leaves
/// everything under it inside the tree — a key whose own rung is gone would
/// otherwise answer the library's top level, where the folder's next scan
/// cannot see the shelf. `None` only for the root rung or a tree whose rungs
/// have all gone.
pub fn rung_above(shelves: &[Shelf], folder_id: &str, key: &str) -> Option<String> {
    let rungs = rungs_of(shelves, folder_id);
    let mut above = crate::folder::parent_key(key)?;
    loop {
        if let Some(id) = rungs.get(above) {
            return Some(id.to_string());
        }
        above = crate::folder::parent_key(above)?;
    }
}

/// The parent the folder's own shelves name for a folder shelf: the nearest
/// rung still standing above its `rel`, or the library's root for a top-level
/// rung. `None` for a shelf that is no folder's rung — no disk answer to
/// compare against.
fn folder_seat(shelves: &[Shelf], shelf_id: &str) -> Option<Option<String>> {
    let shelf = find(shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let key = rel.clone().unwrap_or_default();
    Some(rung_above(shelves, folder_id, &key))
}

/// Every shelf below any of `roots`, at any depth, without repeats and
/// without the roots themselves. An explicit stack rather than recursion:
/// this reads a list that can be caught between two writes, and recursing
/// over a graph with a loop in it is a stack overflow.
pub fn subtree_ids(shelves: &[Shelf], roots: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = roots.to_vec();
    while let Some(parent) = stack.pop() {
        for child in children_of(shelves, Some(parent.as_str())) {
            if roots.iter().any(|each| each == &child.id)
                || out.iter().any(|each| each == &child.id)
            {
                continue;
            }
            stack.push(child.id.clone());
            out.push(child.id.clone());
        }
    }
    out
}

/// Move a shelf's children up to the level it was on: a child left pointing
/// at a gone parent renders on no level at all.
pub fn lift_children(shelves: &mut [Shelf], folder_id: &str) {
    let inherited = shelves
        .iter()
        .find(|s| s.id == folder_id)
        .and_then(|s| s.parent.clone());
    for shelf in shelves.iter_mut() {
        if shelf.parent.as_deref() == Some(folder_id) {
            shelf.parent = inherited.clone();
        }
    }
}

/// The moves a watched folder's rescan owes its own shelves: every
/// non-hand-moved shelf the folder owns whose `rel` resolves to a different
/// parent than the one it hangs on. A folder card cut from a watched tree is
/// a view of that tree, so the disk's shape wins.
pub fn rehang_moves(shelves: &[Shelf], folder_id: &str) -> Vec<(String, Option<String>)> {
    let mut moved = Vec::new();
    for shelf in shelves.iter() {
        // The reader's placement wins over the disk's shape.
        if shelf.manual_parent {
            continue;
        }
        let ShelfKind::Folder {
            folder_id: owner,
            rel,
        } = &shelf.kind
        else {
            continue;
        };
        if owner != folder_id {
            continue;
        }
        let want = rung_above(shelves, folder_id, rel.as_deref().unwrap_or(""));
        if shelf.parent != want {
            moved.push((shelf.id.clone(), want));
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::ALL_SHELF;
    use crate::shelf::kit::{cut, ids_of, in_place_tree, nested, plain, shelf};
    use crate::shelf::tree::ancestors;
    use crate::shelf::tree::can_nest;
    use crate::shelf::tree::children_of;
    use crate::shelf::tree::lift_children;
    use crate::shelf::tree::rehang_moves;
    use crate::shelf::tree::reparent;
    use crate::shelf::tree::subtree_ids;

    #[test]
    fn a_level_is_the_shelves_filed_directly_inside_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Crime", "s1"),
            nested("s4", "Space", "s2"),
        ];
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2", "s3"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s2"))), vec!["s4"]);
        assert!(children_of(&shelves, Some("nope")).is_empty());
    }

    #[test]
    fn the_way_out_of_a_shelf_is_the_chain_above_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
        ];
        assert!(ancestors(&shelves, "s1").is_empty());
        assert_eq!(ids_of(&ancestors(&shelves, "s2")), vec!["s1"]);
        assert_eq!(ids_of(&ancestors(&shelves, "s3")), vec!["s1", "s2"]);
        assert!(ancestors(&shelves, ALL_SHELF).is_empty());
        assert!(ancestors(&shelves, "nope").is_empty());
    }

    #[test]
    fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
        // s3 is inside s2 and both sit at the root, so s1 is the one shelf
        // that is nobody's ancestor.
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(can_nest(&shelves, "s1", "s3"), "s1 is above nothing, so it can go deepest");
        assert!(can_nest(&shelves, "s2", "s1"));
        assert!(can_nest(&shelves, "s3", "s1"), "and a shelf may be lifted out of its branch");
        assert!(!can_nest(&shelves, "s2", "s2"), "a shelf is not inside itself");
        assert!(!can_nest(&shelves, "s1", "s1"));
        assert!(!can_nest(&shelves, "s2", "s3"), "s3 is already inside s2");
        assert!(!can_nest(&shelves, "s2", "s2"));
        assert!(can_nest(&shelves, "s9", "s1"));
        let mut none: Vec<Shelf> = Vec::new();
        assert!(!reparent(&mut none, "s9", Some("s1")));
    }

    #[test]
    fn a_nest_writes_one_parent_and_a_refusal_writes_nothing() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2"]);
        assert!(!can_nest(&shelves, "s1", "s3"));
        assert!(!reparent(&mut shelves, "s1", Some("s3")));
        assert_eq!(shelves[0].parent, None);
        assert!(reparent(&mut shelves, "s2", None));
        assert_eq!(shelves[1].parent, None);
    }

    #[test]
    fn taking_a_shelf_apart_lifts_the_shelves_inside_it() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        lift_children(&mut shelves, "s2");
        assert_eq!(
            shelves[2].parent.as_deref(),
            Some("s1"),
            "s3 inherits the level s2 was on, not the top of the library"
        );
        assert_eq!(shelves[3].parent, None, "a shelf elsewhere is not touched");
        lift_children(&mut shelves, "s1");
        assert_eq!(shelves[1].parent, None, "a root shelf's children become roots");
        assert_eq!(
            shelves[2].parent, None,
            "including the one that just moved up into it"
        );
    }

    #[test]
    fn a_flat_subfolder_shelf_rehangs_under_the_rung_it_was_cut_from() {
        // An older build minted "2/deep" as a sibling of the root; the disk's
        // tree says it hangs on "2".
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("two", "f1", Some("2"), None),
            cut("deep", "f1", Some("2/deep"), None),
        ];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(
            moves,
            vec![
                ("two".to_string(), Some("r".to_string())),
                ("deep".to_string(), Some("two".to_string())),
            ]
        );
    }

    #[test]
    fn a_hand_moved_shelf_keeps_its_place_and_still_routes_its_subtree() {
        let mut two = cut("two", "f1", Some("2"), Some("r"));
        two.manual_parent = true;
        let shelves = vec![
            cut("r", "f1", None, None),
            two,
            // The subtree the reader carried off re-hangs together.
            cut("deep", "f1", Some("2/deep"), None),
        ];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(moves, vec![("deep".to_string(), Some("two".to_string()))]);
    }

    #[test]
    fn a_rehang_never_touches_a_virtual_shelf_or_another_folders() {
        let shelves = vec![
            cut("r", "f1", None, None),
            plain("mine", &[]),
            cut("other", "f2", None, None),
        ];
        assert!(rehang_moves(&shelves, "f1").is_empty());
    }

    #[test]
    fn a_stale_rung_that_names_the_shelf_itself_rehangs_on_the_rung_above_it() {
        // The shelf's own key is never its seat: the rung above it answers,
        // and with no root rung in the list that is the library's top level.
        let shelves = vec![cut("loop", "f1", Some("loop"), Some("r"))];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(moves, vec![("loop".to_string(), None)]);
    }

    #[test]
    fn a_rung_under_a_level_that_was_taken_apart_hangs_inside_the_tree() {
        // `2nd` stood inside `1st` and `1st` is gone: its books come up to
        // the rung the tree still stands on, never out of the tree.
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("second", "f1", Some("1st/2nd"), None),
        ];
        assert_eq!(
            rehang_moves(&shelves, "f1"),
            vec![("second".to_string(), Some("r".to_string()))]
        );
    }

    #[test]
    fn a_hand_that_puts_a_shelf_back_on_its_seat_gives_the_scan_its_place_back() {
        let (mut shelves, _) = in_place_tree();
        assert!(reparent(&mut shelves, "sf", Some("mine")));
        assert!(marked(&shelves, "sf"));
        // Back on the seat the disk names, the mark comes off.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
        // A re-order on the shelf's current seat writes the same answer it had.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
    }

    #[test]
    fn the_subtree_is_everything_below_and_never_the_root_itself() {
        let tree = vec![
            shelf("a", "A", &[]),
            nested("b", "B", "a"),
            nested("c", "C", "b"),
            nested("d", "D", "a"),
        ];
        let mut under_a = subtree_ids(&tree, &["a".to_string()]);
        under_a.sort();
        assert_eq!(
            under_a,
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
        assert!(
            subtree_ids(&tree, &["d".to_string()]).is_empty(),
            "an empty leaf has no subtree"
        );
        let mut under_both = subtree_ids(&tree, &["a".to_string(), "b".to_string()]);
        under_both.sort();
        assert_eq!(under_both, vec!["c".to_string(), "d".to_string()]);
        // A loop a hand-edited blob can carry still terminates: the seen-set
        // refuses the second visit of a root.
        let looped = vec![nested("x", "X", "y"), nested("y", "Y", "x")];
        assert_eq!(
            subtree_ids(&looped, &["x".to_string()]),
            vec!["y".to_string()]
        );
    }

    fn marked(shelves: &[Shelf], id: &str) -> bool {
        shelves
            .iter()
            .find(|s| s.id == id)
            .is_some_and(|s| s.manual_parent)
    }
}
