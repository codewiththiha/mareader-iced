//! One answer to "who owns this path", instead of four.
//!
//! Callers used to each walk the watched-folder and shelf lists for a
//! variation of one question: which read-at-place tree's ledger speaks for
//! this ground, and which of its shelves still stands. Resolved once here.

use crate::folder::{rel_under, FolderMode, WatchedFolder};
use crate::shelf::{ancestors, find as find_shelf, Shelf, ShelfKind};

/// The folder, the rung and the standing shelf that answer for a path. A
/// named struct rather than a tuple: the three strings are easy to transpose,
/// and a gate that read `.1` for a shelf id would be a bug no type catches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub folder_id: String,
    pub rel: String,
    pub shelf_id: String,
}

/// The seat a shelf stands on in a tree: which folder's ground it wears and
/// which rung of that tree it is. The shelf-shaped twin of [`Coverage`],
/// which answers the same question from a path's side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub folder_id: String,
    /// The rung whose tracking decision a toggle from this shelf writes.
    pub rung: String,
}

/// The "who owns this path" questions, asked of one snapshot of the two lists
/// every caller already holds.
pub struct Governance<'a> {
    folders: &'a [WatchedFolder],
    shelves: &'a [Shelf],
}

impl<'a> Governance<'a> {
    /// Borrow the two lists; nothing is cloned.
    pub fn new(folders: &'a [WatchedFolder], shelves: &'a [Shelf]) -> Self {
        Self { folders, shelves }
    }

    /// The shelf a folder's ledger names for a rung, when that shelf still
    /// stands: the one read behind both [`Self::covering`] and [`Self::family`].
    fn standing_rung<'f>(&self, folder: &'f WatchedFolder, rung: &str) -> Option<&'f String> {
        folder
            .shelf_map
            .get(rung)
            .filter(|id| self.shelves.iter().any(|s| s.id == **id))
    }

    /// The standing shelf an in-place tree already holds for `ground`: the
    /// ground is (or is inside) a folder the library reads in place, and the
    /// rung its directory names has a shelf still standing.
    ///
    /// A folder's own tree wins outright (the empty rung); otherwise the tree
    /// whose root sits highest answers.
    pub fn covering(&self, ground: &str) -> Option<Coverage> {
        let mut rung: Option<Coverage> = None;
        for folder in self.folders.iter().filter(|f| f.mode().reads_in_place()) {
            let Some(rel) = rel_under(ground, &folder.root) else {
                continue;
            };
            let Some(shelf_id) = self.standing_rung(folder, &rel) else {
                continue;
            };
            let coverage = Coverage {
                folder_id: folder.id.clone(),
                rel: rel.clone(),
                shelf_id: shelf_id.clone(),
            };
            if rel.is_empty() {
                return Some(coverage);
            }
            if rung.is_none() {
                rung = Some(coverage);
            }
        }
        rung
    }

    /// The family a ground belongs to but is not standing in: the deepest
    /// in-place folder whose root covers `ground` at a rung of its own, when
    /// the ledger names no standing shelf for that rung (a slot a removal
    /// emptied, or a departure) while the folder's root shelf still stands.
    /// `None` when the covering rung is alive — that is [`Self::covering`]'s
    /// answer, not a family to fold back into.
    ///
    /// A tree whose root shelf the reader has taken out is no family: ground
    /// under it is a start of its own.
    pub fn family(&self, ground: &str) -> Option<(String, String)> {
        self.folders
            .iter()
            .filter(|f| f.mode().reads_in_place())
            .filter(|f| self.standing_rung(f, "").is_some())
            .filter_map(|f| {
                rel_under(ground, &f.root)
                    .filter(|rel| !rel.is_empty())
                    .map(|rel| (rel.len(), f, rel))
            })
            .max_by_key(|(len, _, _)| *len)
            .and_then(|(_, folder, rel)| {
                self.standing_rung(folder, &rel)
                    .is_none()
                    .then(|| (folder.id.clone(), rel))
            })
    }

    /// The tree a shelf stands in: a folder rung's own `rel`, or the rung of
    /// the closest folder shelf above a shelf the reader made. `None` for a
    /// shelf no tree answers for — one made at the root, or one that left its
    /// tree ([`ShelfKind::Departed`]).
    fn tree_of(&self, shelf_id: &str) -> Option<Seat> {
        let shelf = find_shelf(self.shelves, shelf_id)?;
        let (folder_id, rung) = match &shelf.kind {
            ShelfKind::Folder { folder_id, rel } => {
                (folder_id.as_str(), rel.as_deref().unwrap_or(""))
            }
            // A departed shelf stands where the reader put it, not on ground
            // the disk names.
            ShelfKind::Departed => return None,
            // Not a rung of any tree: the closest folder shelf above it is
            // the tree it stands inside — and a departed shelf ends that
            // ground, so what stands inside one stands on the reader's own.
            ShelfKind::Virtual => ancestors(self.shelves, shelf_id)
                .iter()
                .rev()
                .take_while(|each| each.kind != ShelfKind::Departed)
                .find_map(|each| match &each.kind {
                    ShelfKind::Folder { folder_id, rel } => {
                        Some((folder_id.as_str(), rel.as_deref().unwrap_or("")))
                    }
                    _ => None,
                })?,
        };
        crate::folder::find(self.folders, folder_id)?;
        Some(Seat {
            folder_id: folder_id.to_string(),
            rung: rung.to_string(),
        })
    }

    /// [`Self::tree_of`] restricted to read-in-place trees: the seat a watch
    /// toggle writes and a dot reads. A copying tree answers no seat —
    /// tracking is a promise about the tree books are read from.
    pub fn seat_of(&self, shelf_id: &str) -> Option<Seat> {
        let seat = self.tree_of(shelf_id)?;
        let folder = crate::folder::find(self.folders, &seat.folder_id)?;
        folder.mode().reads_in_place().then_some(seat)
    }

    /// Where a shelf's books live: [`FolderMode::Copy`] for a shelf the
    /// library keeps copies for (cut from a copying import, or departed by a
    /// move that paid the copy), the tree's own mode for a shelf that is a
    /// door onto a directory, `None` for a shelf no folder answers for.
    pub fn mode_of(&self, shelf_id: &str) -> Option<FolderMode> {
        if find_shelf(self.shelves, shelf_id)?.kind == ShelfKind::Departed {
            return Some(FolderMode::Copy);
        }
        let seat = self.tree_of(shelf_id)?;
        crate::folder::find(self.folders, &seat.folder_id).map(|f| f.mode())
    }

    /// Whether the tree a shelf was cut from tracks the rung that shelf
    /// stands on — the question every watch dot asks. `false` for a shelf no
    /// tree answers for: nothing tracks it.
    pub fn shelf_tracked(&self, shelf_id: &str) -> bool {
        let Some(seat) = self.seat_of(shelf_id) else {
            return false;
        };
        let Some(folder) = crate::folder::find(self.folders, &seat.folder_id) else {
            return false;
        };
        folder.tracks_rung(&seat.rung)
    }

}

#[cfg(test)]
mod tests;
