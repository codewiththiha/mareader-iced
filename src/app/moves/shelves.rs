//! The shelf-scale moves: a whole level's departure, its return home, and the
//! rung a take-apart reclaims.

use crate::app::Mareader;
use crate::app::walk::{Claim, chain_for, flatten_rungs, member_rungs, page_into, rel_of};
use crate::library::departure::{self, ReturnPath, ShelfDeparture, ShelfSeam};
use crate::library;
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::time::Instant;
use library_core::folder::{self as folder_ops};
use library_core::shelf::{self, Shelf};
use std::collections::HashSet;

impl Mareader {
    /// The landing the reader bought: the rungs leave the tree with the copy
    /// they paid for, the other folders' shelves that rode along take the
    /// hand's mark, the folder lets the departed zone go, and the copies
    /// wear the level's next free names — then the gesture's own seam or
    /// nest seats what landed. A shelf whose books the store refused entire
    /// stays where it was, with its own sentence.
    pub(in crate::app) fn land_shelf_moves(
        &mut self,
        deps: Vec<ShelfDeparture>,
        level: Option<String>,
        seam: Option<ShelfSeam>,
        departed: &[String],
    ) -> bool {
        let mut landed: Vec<String> = Vec::new();
        for dep in &deps {
            if dep.books.is_empty() || dep.books.iter().any(|id| departed.contains(id)) {
                landed.push(dep.id.clone());
            } else {
                self.toasts.show(
                    Tone::Info,
                    format!(
                        "“{}” stayed where it was — the library could not copy its books.",
                        dep.name
                    ),
                    Instant::now(),
                );
            }
        }
        if landed.is_empty() {
            return false;
        }
        let going: Vec<&ShelfDeparture> =
            deps.iter().filter(|dep| landed.contains(&dep.id)).collect();
        let mut promised: HashSet<String> =
            shelf::children_of(&self.library.shelves, level.as_deref())
                .into_iter()
                .map(|s| s.name.clone())
                .collect();
        for dep in &going {
            for rung in &dep.rungs {
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, rung) {
                    one.kind = library_core::shelf::ShelfKind::Departed;
                    one.manual_parent = false;
                }
            }
            for id in &dep.subtree {
                if dep.rungs.contains(id) {
                    continue;
                }
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, id)
                    && one.is_folder()
                {
                    one.manual_parent = true;
                }
            }
            if let Some(one) = shelf::find_mut(&mut self.library.shelves, &dep.id) {
                one.name = departure::free_name(&dep.name, &mut promised);
            }
        }
        for dep in &going {
            let Some(folder) = folder_ops::find_mut(&mut self.library.folders, &dep.folder_id)
            else {
                continue;
            };
            folder.shelf_map.retain(|key, shelf_id| {
                !folder_ops::key_in_zone(key, &dep.rel) && !dep.rungs.contains(shelf_id)
            });
        }
        let ids: Vec<String> = going.iter().map(|dep| dep.id.clone()).collect();
        // The seats ride the very gestures the screen wraps — clean by
        // construction now, because a Departed rung owes no second copy.
        if let Some(seam) = &seam {
            library::arrange::reorder_shelves_to_anchor(
                &mut self.library.shelves,
                &ids,
                &seam.anchor_id,
                seam.after,
            );
        } else {
            for id in &ids {
                library::arrange::nest_shelf(&mut self.library.shelves, id, level.as_deref());
            }
        }
        true
    }

    /// No copies: every mover that has a way home takes it, and a mover that
    /// has none stays where the tree put it. The fold is the import's own
    /// `reclaim_rung`, and the reseat rides the very `nest_shelf` the
    /// gesture did. The reveal that lights the first seated shelf waits on
    /// the reveal's own light.
    pub(in crate::app) fn take_them_home(&mut self, returns: &[(String, ReturnPath)]) -> bool {
        let mut moved = false;
        for (shelf_id, path) in returns {
            moved |= match path {
                ReturnPath::Reclaim { tree, gone, rel } => {
                    self.reclaim_rung(tree, gone, rel, shelf_id).is_some()
                }
                ReturnPath::Reseat { seat } => library::arrange::nest_shelf(
                    &mut self.library.shelves,
                    shelf_id,
                    seat.as_deref(),
                ),
            };
        }
        moved
    }

    /// Put a displaced member back on the rung its directory names and fold
    /// the folder that was reading it into the tree that contains it: one
    /// ground, one reader from here on. Three writes, in the order that
    /// keeps them honest; the answer is the shelf the member sat on.
    ///
    /// A tree a walk holds is the one fold to refuse, because that walk's
    /// clone of the ledger lands after this write and drops it. Persists
    /// nothing itself: the caller ends its own transaction.
    pub(in crate::app) fn reclaim_rung(
        &mut self,
        tree_id: &str,
        gone_id: &str,
        rel: &str,
        shelf_id: &str,
    ) -> Option<String> {
        let now = now_ms();
        let mut minted: Vec<Shelf> = Vec::new();
        let mut tree = folder_ops::find(&self.library.folders, tree_id)?.clone();
        let gone = folder_ops::find(&self.library.folders, gone_id)?.clone();
        // Read before anything is written: the answer is about the shelves
        // standing, not the ones this move mints.
        let rungs = member_rungs(&self.library.shelves, gone_id, rel);
        // A shelf that went while the sheet was up is an answer with nothing
        // to move; so is a tree a walk holds, whose ledger clone lands after
        // this write.
        let foreign_walk = !matches!(self.root_claim(&tree.root), Claim::Free);
        if !rungs.iter().any(|(_, id)| id == shelf_id) || foreign_walk {
            return None;
        }
        let root = tree.root.clone();
        // Ground the shape keeps on one shelf has no rung for the member's
        // directory to become: its books come onto the rung the ground
        // answers for and its own shelves go, so an adoption cannot cut a
        // nested rung into an import that asked for none.
        let seat = if tree.cuts(rel) {
            let parent = chain_for(
                &mut tree,
                folder_ops::parent_key(rel).unwrap_or(""),
                now,
                None,
                &root,
                &mut minted,
            );
            for (key, id) in &rungs {
                tree.shelf_map.insert(key.clone(), id.clone());
            }
            page_into(&mut self.library.shelves, minted);
            let nestable = shelf::can_nest(&self.library.shelves, shelf_id, &parent);
            for (key, id) in &rungs {
                let Some(one) = shelf::find_mut(&mut self.library.shelves, id) else {
                    continue;
                };
                one.kind = library_core::shelf::ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(key),
                };
                if id != shelf_id {
                    continue;
                }
                if nestable {
                    one.parent = Some(parent.clone());
                    one.manual_parent = false;
                } else {
                    one.manual_parent = true;
                }
            }
            shelf_id.to_string()
        } else {
            // A pointer to a shelf that went is no seat, and this tree's map
            // is not the one the run pruned: the mint is asked for the key
            // the map has no standing rung under.
            let standing = tree
                .shelf_map
                .get("")
                .filter(|id| shelf::find(&self.library.shelves, id).is_some())
                .cloned();
            let seat = match standing {
                Some(seat) => seat,
                None => {
                    tree.shelf_map.remove("");
                    chain_for(&mut tree, "", now, None, &root, &mut minted)
                }
            };
            page_into(&mut self.library.shelves, minted);
            flatten_rungs(&mut self.library.shelves, gone_id, &seat);
            seat
        };
        // The answer the folded row carried for its own root becomes the
        // rung it becomes: the row that answer was written on is the one
        // this fold retires. A tree that cuts no rungs has one answer for
        // the whole of its ground — the reader's own about its root — and
        // the adoption does not second-guess it.
        if tree.cuts(rel) {
            tree.set_tracking(rel, gone.opts.watch);
        }
        tree.placed.extend(gone.placed.iter().copied());
        for stone in gone.ignored.iter() {
            if !tree.is_ignored(&stone.fp) {
                tree.ignored.push(stone.clone());
            }
        }
        tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);
        self.library.folders.retain(|f| f.id != gone_id);
        match self.library.folders.iter().position(|f| f.id == tree_id) {
            Some(at) => self.library.folders[at] = tree,
            None => self.library.folders.push(tree),
        }
        Some(seat)
    }
}
