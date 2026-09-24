//! A watched folder: its root, the options the import was made with, and the
//! ledger of what that import already did. The ledger is why this is a struct and
//! not a settings row — "which files did I already place" is not "which files are
//! in the library", and a rescan runs on every window focus.
mod keys;
mod mode;
mod options;
mod sanitize;
mod shapes;
mod shelves;
mod tombstone;
mod tracked;

pub use keys::*;
pub use mode::*;
pub use options::*;
pub use sanitize::*;
pub use tombstone::*;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::book::Fingerprint;
use crate::shape::ShapeTree;
use crate::tracking::TrackingTree;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: String,
    /// Never rewritten by the app: a moved folder is a missing folder, and the
    /// sheet offers a new path rather than guessing.
    pub root: String,
    #[serde(default)]
    pub opts: FolderOpts,
    /// Fingerprints this folder has already placed. Membership is what makes a
    /// rescan honest: a book the reader moved keeps its fingerprint here, so
    /// the next scan skips it instead of putting it back.
    #[serde(default)]
    pub placed: HashSet<Fingerprint>,
    /// Books the reader deliberately removed, one [`Tombstone`] per removal:
    /// the file is still on disk and still admitted by `opts`, so without one
    /// the next rescan would re-add exactly what was just deleted. The
    /// folder's import menu reads it too.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest scan saw, restricted to placed fingerprints:
    /// fingerprint to the address it was found at. Lets the import menu open
    /// instantly instead of walking the tree again.
    #[serde(default)]
    pub last_seen: Vec<(Fingerprint, String)>,
    /// Rung key to shelf id, persisted so a rescan adds to the shelf the last
    /// run created rather than minting a second one of the same name.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, String>,
    /// `0` until the first scan completes. Diagnostic only: no decision reads
    /// it, so a stale stamp can never suppress a scan.
    #[serde(default)]
    pub scanned_ms: u64,
    /// Per-rung tracking answers. [`crate::tracking::TrackingTree`] owns the
    /// inheritance; [`sanitize`] carries the legacy [`FolderOpts::watch`] flag
    /// into it.
    #[serde(default)]
    pub tracking: TrackingTree,
    /// Per-rung shelf-shape answers. [`ShapeTree`] owns the inheritance;
    /// [`WatchedFolder::set_shape`] keeps the root's answer equal to
    /// [`FolderOpts::groups`].
    #[serde(default)]
    pub shapes: ShapeTree,
}

/// Lookup by id. A folder that is gone answers `None` everywhere rather than
/// a silent no-match.
pub fn find<'a>(folders: &'a [WatchedFolder], id: &str) -> Option<&'a WatchedFolder> {
    folders.iter().find(|f| f.id == id)
}

pub fn find_mut<'a>(folders: &'a mut [WatchedFolder], id: &str) -> Option<&'a mut WatchedFolder> {
    folders.iter_mut().find(|f| f.id == id)
}

impl WatchedFolder {
    /// A row that has never been walked: no placements, no logs, no rungs. One
    /// constructor so every mint site starts with the same empty ledger — a
    /// field added to the struct is a field this fills, not one every call site
    /// has to remember.
    pub fn new(id: impl Into<String>, root: impl Into<String>, opts: FolderOpts) -> Self {
        Self {
            id: id.into(),
            root: root.into(),
            opts,
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::new(),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
            shapes: ShapeTree::default(),
        }
    }

    /// The folder's [`FolderMode`]: what a run does with the files it finds,
    /// and whether a later one walks the tree again.
    pub fn mode(&self) -> FolderMode {
        self.opts.mode()
    }
}
