//! The folder's tombstones: what a removal left behind, pruned as the ledger
//! grows, and the restore a stone answers.

use crate::book::Fingerprint;
use crate::folder::{Tombstone, WatchedFolder};
use super::registry::Registry;

/// Record a deliberate removal so the next rescan stays quiet about the file.
/// Only folders that placed the book take the tombstone: removing a book added
/// by hand must not poison a watched folder holding the same file.
pub fn tombstone(folders: &mut [WatchedFolder], entry: &Tombstone) {
    for folder in folders.iter_mut() {
        if folder.placed.contains(&entry.fp) && !folder.is_ignored(&entry.fp) {
            folder.ignored.push(entry.clone());
        }
    }
}

/// Drop the tombstones whose books came back: a fingerprint can rejoin the
/// library by any route, and a stale tombstone is a restore row offering what
/// the reader already has. Moved-out logs are kept — both their jobs outlive
/// the copy.
pub fn prune_tombstones(folder: &mut WatchedFolder, registry: &Registry) {
    folder
        .ignored
        .retain(|entry| entry.moved || !registry.contains_key(&entry.fp));
}

/// Peeks without consuming: a restore measures the file before it promises
/// anything, and a removal that stays put when the measurement fails is the
/// difference between "that file is gone" and a book quietly lost.
pub fn find_tombstone<'a>(folder: &'a WatchedFolder, fp: &Fingerprint) -> Option<&'a Tombstone> {
    folder.ignored.iter().find(|entry| &entry.fp == fp)
}

/// Lift a tombstone. Does not touch `placed`: the import that follows marks
/// the placement when the book lands, and marking it here would leave a
/// fingerprint the ledger skips with no book behind it.
pub fn restore_deleted(folder: &mut WatchedFolder, fp: &Fingerprint) -> Option<Tombstone> {
    let at = folder.ignored.iter().position(|entry| &entry.fp == fp)?;
    Some(folder.ignored.remove(at))
}
