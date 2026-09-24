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
