//! The shape tree of a watched folder: which rungs the reader folded, and which
//! keys a walk may cut at.

use super::{key_chain, WatchedFolder};

impl WatchedFolder {
    /// The shelf shape at `key`: the deepest rung that answered for itself,
    /// else the folder's own import answer. Per-rung rather than per-import, so
    /// re-importing one nested folder moves that folder's books and leaves the
    /// rest of the tree alone.
    pub fn shape_at(&self, key: &str) -> bool {
        self.shapes.at(key).unwrap_or(self.opts.groups)
    }

    /// Record the shelf shape one rung answers with. The root's answer IS
    /// [`FolderOpts::groups`] and stands for the whole tree, so setting it
    /// takes every deeper answer with it; a rung below the root answers for
    /// itself and its subtree only.
    pub fn set_shape(&mut self, rung: &str, grouped: bool) {
        if rung.is_empty() {
            self.opts.groups = grouped;
            self.shapes.prune_zone("");
        } else {
            self.shapes.set(rung, grouped);
        }
    }

    /// Whether the shape cuts a rung at this key: the root is always cut; a
    /// deeper rung is cut where the shape answers for a shelf per folder.
    /// One rule for the walk, the re-shape and the fold, so the three cannot
    /// disagree about where a book belongs.
    pub fn cuts(&self, key: &str) -> bool {
        key.is_empty() || self.shape_at(key)
    }

    /// The rung an address's own folder answers for: the deepest rung of the
    /// folder's chain that the shape cuts. A folder in a subfolder of a
    /// one-shelf tree answers for the root rung.
    pub fn rung_for(&self, key: &str) -> String {
        key_chain(key)
            .into_iter()
            .rev()
            .find(|rung| self.cuts(rung))
            .unwrap_or_default()
            .to_string()
    }
}
