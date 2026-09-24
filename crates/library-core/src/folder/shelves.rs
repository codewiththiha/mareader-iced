//! Where a found file belongs: the shelf key a rung resolves to, and the chain a
//! deep file walks up to reach it.

use crate::scan::{subfolder_of, FoundFile};
use crate::shelf::Shelf;
use super::{key_chain, rel_under, WatchedFolder};

impl WatchedFolder {
    /// The ledger key for a found file: [`Self::rung_for`] the file's own
    /// subfolder. One owner of the choice, so the walk, shelf creation and the
    /// persisted map cannot disagree.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        self.rung_for(found.subfolder())
    }

    /// The two shelves this folder's tree names for an address: the one the
    /// address answers for under the shape, and the folder's root one. Both are
    /// `None` when the address is not under this folder.
    ///
    /// Two answers because the questions differ: *where does this file come
    /// back to* wants the root as a fallback, while *has this file left its
    /// ground* wants the first alone — treating the root as a second home
    /// would let a book be dragged rung to rung while still answering to the
    /// folder that placed it.
    pub fn rungs_for(&self, path: &str) -> (Option<&str>, Option<&str>) {
        let Some(rel) = rel_under(path, &self.root) else {
            return (None, None);
        };
        let key = self.rung_for(subfolder_of(&rel));
        (
            self.shelf_map.get(key.as_str()).map(String::as_str),
            self.shelf_map.get("").map(String::as_str),
        )
    }

    /// The shelf a found file belongs on, minting every rung between the
    /// folder's root shelf and the file's own subfolder and reporting each mint
    /// through `made`.
    ///
    /// A walk reports files, not directories, so minting only the leaf would
    /// hang a subfolder's shelf off the root with a hole above it. Rungs
    /// already in [`WatchedFolder::shelf_map`] are reused, which is what makes
    /// a rescan continue the tree instead of growing a twin beside it.
    pub fn shelf_chain_for(
        &mut self,
        key: &str,
        mut mint: impl FnMut(&str) -> String,
        mut name_of: impl FnMut(&str) -> String,
        mut made: impl FnMut(&str, &str, String, Option<String>),
    ) -> String {
        let mut current: Option<String> = None;
        let mut id = String::new();
        for rung in key_chain(key) {
            id = match self.shelf_map.get(rung) {
                Some(known) => known.clone(),
                None => {
                    let fresh = mint(rung);
                    self.shelf_map.insert(rung.to_string(), fresh.clone());
                    made(rung, &fresh, name_of(rung), current.clone());
                    fresh
                }
            };
            current = Some(id.clone());
        }
        id
    }

    /// Drop the map's pointers at shelves that no longer stand, answering
    /// whether it dropped any. A dead pointer is a rung the walk reuses instead
    /// of minting, and the placement that rides it lands on no shelf at all.
    pub fn prune_shelf_map(&mut self, shelves: &[Shelf]) -> bool {
        let before = self.shelf_map.len();
        self.shelf_map
            .retain(|_, id| shelves.iter().any(|s| &s.id == id));
        self.shelf_map.len() != before
    }
}
