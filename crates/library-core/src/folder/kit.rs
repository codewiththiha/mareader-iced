//! Fixtures the cases across this module share.

use std::collections::{BTreeMap, HashSet};
use crate::book::Fingerprint;
use crate::folder::WatchedFolder;
use crate::folder::options::FolderOpts;
use crate::tracking::TrackingTree;

pub(super) fn fp(n: u32) -> Fingerprint {
    crate::testkit::fp_n(n)
}

pub(super) fn folder(root: &str) -> WatchedFolder {
    WatchedFolder {
        id: "f1".into(),
        root: root.into(),
        opts: FolderOpts::default(),
        placed: HashSet::new(),
        ignored: Vec::new(),
        shelf_map: BTreeMap::new(),
        last_seen: Vec::new(),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
        shapes: crate::shape::ShapeTree::default(),
    }
}

/// The watch arrives as a tree, not the flag alone: the flag is now the
/// root rung's mirror, and a fixture setting only the flag would be a
/// folder this build never writes.
pub(super) fn mode(id: &str, root: &str, in_place: bool, watch: bool) -> WatchedFolder {
    let mut folder = WatchedFolder {
        id: id.into(),
        opts: FolderOpts {
            in_place,
            watch,
            ..FolderOpts::default()
        },
        ..folder(root)
    };
    if watch {
        folder.set_tracking("", true);
    }
    folder
}
