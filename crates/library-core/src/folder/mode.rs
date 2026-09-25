//! How a folder is read: the modes a folder's options resolve to, and what each
//! says on its shelf.

use super::FolderOpts;

/// The three ways a folder import can hold its books, computed from the two
/// switches [`FolderOpts`] persists — a view over the pair rather than a
/// fourth field to migrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMode {
    /// Copy every admitted file into the library's own store.
    Copy,
    LinkInPlace,
    /// Read each book at the address it was found at, and walk the tree again
    /// when the app opens or regains focus.
    LinkInPlaceWatched,
}

impl FolderMode {
    /// A match over the pair so the folding of the unofferable combination is
    /// visible in one place: a watching copy is a copy, because tracking is a
    /// promise about the tree the books are read from.
    pub fn from_opts(opts: &FolderOpts) -> Self {
        match (opts.in_place, opts.watch) {
            (false, _) => FolderMode::Copy,
            (true, false) => FolderMode::LinkInPlace,
            (true, true) => FolderMode::LinkInPlaceWatched,
        }
    }

    pub fn copies_files(self) -> bool {
        matches!(self, FolderMode::Copy)
    }

    pub fn reads_in_place(self) -> bool {
        !self.copies_files()
    }

    /// The root's answer only; which rungs are actually tracked is the tree's
    /// question ([`WatchedFolder::tracked`], [`WatchedFolder::tracks_rung`]).
    pub fn tracks_new_files(self) -> bool {
        matches!(self, FolderMode::LinkInPlaceWatched)
    }

    /// The mode's one user-facing wording; a mode described two ways reads as
    /// two modes.
    pub fn label(self) -> &'static str {
        match self {
            FolderMode::Copy => "Copy into the library",
            FolderMode::LinkInPlace => "Read at place",
            FolderMode::LinkInPlaceWatched => "Read at place, and watch for new books",
        }
    }

    /// Two-word form of [`FolderMode::label`] for a folder card's badge: where
    /// the books are — the dot beside it is the watched signal.
    pub fn badge(self) -> &'static str {
        match self {
            FolderMode::Copy => "Copied",
            FolderMode::LinkInPlace | FolderMode::LinkInPlaceWatched => "On disk",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::kit::{folder, mode};
    use crate::folder::options::FolderOpts;

    #[test]
    fn the_two_switches_add_up_to_one_mode_and_its_questions() {
        // One of the four switch combinations is not offerable; the folding
        // happens where the mode is computed.
        let opts = |in_place: bool, watch: bool| FolderOpts {
            in_place,
            watch,
            ..FolderOpts::default()
        };
        assert_eq!(opts(true, false).mode(), FolderMode::LinkInPlace);
        assert_eq!(opts(true, true).mode(), FolderMode::LinkInPlaceWatched);
        assert_eq!(opts(false, false).mode(), FolderMode::Copy);
        assert_eq!(
            opts(false, true).mode(),
            FolderMode::Copy,
            "a copy does not care what the source folder does next"
        );
        assert!(opts(false, true).mode().copies_files());
        assert!(!opts(false, true).mode().reads_in_place());
        assert!(opts(true, true).mode().reads_in_place());
        assert!(!opts(true, true).mode().copies_files());
        assert!(opts(true, true).mode().tracks_new_files());
        assert!(!opts(true, false).mode().tracks_new_files());
        let mut folder = folder("/books");
        folder.opts.in_place = true;
        folder.set_tracking("", true);
        assert!(folder.opts.watch, "the root's answer is mirrored onto the flag");
        assert_eq!(folder.mode(), FolderMode::LinkInPlaceWatched);
        assert_eq!(folder.opts.mode(), folder.mode());
    }

    #[test]
    fn a_folder_the_library_copies_is_owed_no_walk_whatever_its_tree_says() {
        // The unofferable pair (`in_place=false, watch=true`) folds into
        // `Copy`: a tree left standing on a copying folder is chrome, not a
        // second opinion.
        let copying = mode("f1", "/books", false, true);
        assert_eq!(copying.mode(), FolderMode::Copy, "a watching copy is a copy");
        assert!(copying.tracks_anything(), "and its tree is still standing");
        assert!(!copying.owes_walk(), "so nothing walks it");

        // A root turned off with one subfolder left on is still watched.
        let mut partly = mode("f2", "/dvds", true, true);
        partly.set_tracking("", false);
        partly.set_tracking("Films", true);
        assert!(!partly.opts.watch, "the root's own answer is off");
        assert!(partly.owes_walk(), "and the subfolder still owes the walk");

        let quiet = mode("f3", "/comics", true, false);
        assert!(!quiet.owes_walk());
    }
}
