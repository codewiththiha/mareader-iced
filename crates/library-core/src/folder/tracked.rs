//! What the folder already placed: the fingerprints it owes nothing to, the rungs
//! whose new files are watched, and the stones that keep a removal removed.

use crate::book::Fingerprint;
use crate::scan::FoundFile;
use crate::tracking::Track;
use super::WatchedFolder;

impl WatchedFolder {
    /// Record that this folder placed a file, so the next rescan skips it.
    pub fn mark_placed(&mut self, fp: Fingerprint) {
        self.placed.insert(fp);
    }

    /// Whether the tree is tracked from its root — the whole-tree answer the
    /// single [`FolderOpts::watch`] flag used to be. Every surface that draws
    /// a watch dot asks this rather than the flag.
    pub fn tracked(&self) -> bool {
        self.tracking.tracked()
    }

    /// The rung's own tracking decision, or the nearest ancestor that has one.
    pub fn tracks_rung(&self, key: &str) -> bool {
        self.tracking.resolve(key)
    }

    /// Whether this folder is watched anywhere: its root, or any rung turned
    /// on under a root that is off. A tree only a subfolder of which is watched
    /// still owes the walk.
    pub fn tracks_anything(&self) -> bool {
        self.tracking.tracked() || self.tracking.any_on()
    }

    /// Whether a walk is owed. Tracking is a promise about the folder books
    /// are read from; a copying folder has none to keep, so only a
    /// read-in-place folder with any rung on owes the walk.
    pub fn owes_walk(&self) -> bool {
        self.mode().reads_in_place() && self.tracks_anything()
    }

    /// Turn tracking on or off for one rung, mirroring the root's answer back
    /// onto [`FolderOpts::watch`] so the import sheet's switch agrees with the
    /// tree. The flag is the legacy half of one decision, not a second source
    /// of truth.
    pub fn set_tracking(&mut self, key: &str, on: bool) {
        self.tracking.set(key, if on { Track::On } else { Track::Off });
        self.opts.watch = self.tracking.tracked();
    }

    /// Turn the whole tree on or off: every rung's override goes with it.
    pub fn set_tracking_whole(&mut self, on: bool) {
        self.tracking.set_root(on);
        self.opts.watch = on;
    }

    /// Whether this folder holds a removal against `fp`. A tombstone wins
    /// over everything: the reader said no.
    pub fn is_ignored(&self, fp: &Fingerprint) -> bool {
        self.ignored.iter().any(|entry| &entry.fp == fp)
    }

    /// Remember what this scan saw, for placed fingerprints only. Written on
    /// every scan, including one that changed nothing: the menu's "moved out
    /// of this folder" answer is only as fresh as the last walk.
    pub fn record_seen(&mut self, found: &[FoundFile]) {
        let seen: Vec<(Fingerprint, String)> = found
            .iter()
            .filter(|file| self.placed.contains(&file.fp))
            .map(|file| (file.fp, file.path.clone()))
            .collect();
        self.last_seen = seen;
    }
}
