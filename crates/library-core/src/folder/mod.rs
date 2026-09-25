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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use crate::folder::kit::{folder, fp, mode};
    use crate::folder::sanitize::sanitize;
    use crate::scan::FoundFile;

    #[test]
    fn a_file_stands_on_the_rung_its_own_subfolder_names() {
        let f = WatchedFolder {
            shelf_map: BTreeMap::from([
                ("".to_string(), "shelf1".to_string()),
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            ..folder("/books")
        };
        let deep = "/books/Fiction/SciFi/dune.pdf";
        assert_eq!(f.rungs_for(deep), (Some("shelf3"), Some("shelf1")));
        // A drag from shelf3 to shelf2 has left the ground the tree names for
        // this file — the whole of the departure rule.
        assert_eq!(
            f.rungs_for("/books/Fiction/other.pdf"),
            (Some("shelf2"), Some("shelf1"))
        );
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("shelf1"), Some("shelf1")));
        assert_eq!(f.rungs_for("/books/Unmapped/x.pdf"), (None, Some("shelf1")));
        assert_eq!(f.rungs_for("/other/x.pdf"), (None, None));
    }

    #[test]
    fn the_rung_a_walk_names_and_the_rung_an_address_names_agree() {
        // One key arithmetic behind both: a rescan and a drag cannot
        // disagree about where a file belongs.
        let f = WatchedFolder {
            shelf_map: BTreeMap::from([("Fiction/SciFi".to_string(), "shelf3".to_string())]),
            ..folder("/books")
        };
        let found = FoundFile {
            path: "/books/Fiction/SciFi/dune.pdf".into(),
            rel: "Fiction/SciFi/dune.pdf".into(),
            ext: "pdf".into(),
            size: 1,
            fp: fp(1),
        };
        assert_eq!(f.shelf_key(&found), "Fiction/SciFi");
        assert_eq!(f.rungs_for(&found.path).0, Some("shelf3"));
    }

    #[test]
    fn a_blob_from_before_tracking_was_a_tree_keeps_watching() {
        // A folder from an older build carries `opts.watch` and no tree, and
        // an empty tree tracks nothing — without carrying the flag across, a
        // load would silently stop rescanning every watched folder.
        let raw = r#"{"id":"f1","root":"/books","opts":{"inPlace":true,"watch":true}}"#;
        let mut folders: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw}]")).unwrap();
        assert!(folders[0].tracking.is_empty(), "the old blob has no tree");
        assert!(folders[0].opts.watch, "and the flag it did have");
        sanitize(&mut folders);
        assert!(folders[0].tracked(), "the flag became the root rung's decision");
        assert!(folders[0].tracks_rung("Fiction"), "and the tree below it inherits");
        assert!(folders[0].opts.watch, "the flag is left agreed with the tree");
        let raw_off = r#"{"id":"f2","root":"/dvds","opts":{"inPlace":true,"watch":false}}"#;
        let mut off: Vec<WatchedFolder> =
            serde_json::from_str(&format!("[{raw_off}]")).unwrap();
        sanitize(&mut off);
        assert!(!off[0].tracked());
        // The tree is the answer from here on; a stale flag yields to it.
        let mut written = vec![mode("f3", "/books", true, false)];
        written[0].set_tracking("", true);
        assert!(written[0].opts.watch, "set_tracking mirrors the root onto the flag");
        written[0].opts.watch = false;
        sanitize(&mut written);
        assert!(written[0].tracked(), "the tree wins");
        assert!(written[0].opts.watch, "and the flag is brought back into agreement");
    }

    #[test]
    fn a_backslash_never_survives_into_the_shelf_map() {
        // A `\` in a key means the writer did not normalise it; such a key
        // would never match a found file again.
        let mut folders = vec![WatchedFolder {
            shelf_map: BTreeMap::from([
                ("scifi".to_string(), "s1".to_string()),
                ("scifi\\deep".to_string(), "s2".to_string()),
            ]),
            ..folder("/books")
        }];
        sanitize(&mut folders);
        let keys: Vec<&String> = folders[0].shelf_map.keys().collect();
        assert_eq!(keys, vec!["scifi"]);
    }

    #[test]
    fn a_folder_is_found_by_id_for_a_read_and_for_a_write() {
        let mut folders = vec![folder("/one"), folder("/two")];
        folders[1].id = "f2".into();
        assert_eq!(find(&folders, "f2").map(|f| f.root.as_str()), Some("/two"));
        assert!(find(&folders, "gone").is_none());
        // Through the writer, not the field: `set_tracking` keeps the tree and
        // the flag agreed.
        find_mut(&mut folders, "f2").unwrap().set_tracking("", true);
        assert!(find(&folders, "f2").is_some_and(|f| f.tracked() && f.opts.watch));
    }
}

#[cfg(test)]
mod kit;
