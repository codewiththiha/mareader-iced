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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Fingerprint;
    use crate::folder::Tombstone;
    use crate::ledger::kit::{folder, fp, registry, stone};

    #[test]
    fn only_the_folders_that_placed_a_book_take_its_tombstone() {
        let mut folders = vec![folder(&[1], &[]), folder(&[2], &[]), folder(&[], &[])];
        folders[1].id = "f2".into();
        folders[2].id = "f3".into();
        tombstone(&mut folders, &stone(1));
        assert!(folders[0].is_ignored(&fp(1)));
        assert!(folders[1].ignored.is_empty());
        assert!(folders[2].ignored.is_empty());
        tombstone(&mut folders, &stone(9));
        assert!(folders.iter().all(|f| f.ignored.len() <= 1));
    }

    #[test]
    fn pruning_drops_the_tombstone_of_a_book_that_returned() {
        let mut f = folder(&[1], &[1, 2]);
        let reg = registry(&[(1, "b1", "/books/1.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        let left: Vec<Fingerprint> = f.ignored.iter().map(|t| t.fp).collect();
        assert_eq!(left, vec![fp(2)], "only the book that is really gone stays");
    }

    /// A moved-out log outlives the copy carrying its fingerprint: it keeps a
    /// rescan quiet about a file the library answered for once, and an import
    /// of that file spends it.
    #[test]
    fn pruning_keeps_a_moved_out_log_whatever_the_registry_says() {
        let mut f = folder(&[1, 2], &[]);
        f.ignored.push(Tombstone {
            moved: true,
            ..stone(1)
        });
        f.ignored.push(stone(2));
        let reg = registry(&[(1, "b1", "/store/b1.pdf", false), (2, "b2", "/books/2.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        let left: Vec<Fingerprint> = f.ignored.iter().map(|t| t.fp).collect();
        assert_eq!(
            left,
            vec![fp(1)],
            "the copy's own fingerprint in the registry is the log's reason to stand, not to go"
        );
        assert!(f.ignored[0].moved, "and the log that stands is the moved-out one");
    }

    /// The log's spend is a restore's landing: afterwards the file has a
    /// linked book again and the folder needs no log.
    #[test]
    fn a_moved_out_log_is_spent_by_the_restore_that_brings_the_book_back() {
        let mut f = folder(&[1], &[]);
        f.ignored.push(Tombstone {
            moved: true,
            ..stone(1)
        });
        let reg = registry(&[(1, "b1", "/store/b1.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        assert_eq!(f.ignored.len(), 1, "a scan leaves it standing");
        assert!(restore_deleted(&mut f, &fp(1)).is_some(), "a restore takes it");
        prune_tombstones(&mut f, &reg);
        assert!(f.ignored.is_empty(), "and nothing puts it back");
    }

    #[test]
    fn a_restore_takes_the_tombstone_and_leaves_the_placement_to_the_import() {
        let mut f = folder(&[1], &[2]);
        assert!(find_tombstone(&f, &fp(2)).is_some());
        assert!(find_tombstone(&f, &fp(9)).is_none());
        // Peeking must not consume: a failed measurement must leave the
        // removal in place.
        assert!(find_tombstone(&f, &fp(2)).is_some());
        let taken = restore_deleted(&mut f, &fp(2)).expect("present");
        assert_eq!(taken.fp, fp(2));
        assert!(!f.is_ignored(&fp(2)));
        assert!(
            !f.placed.contains(&fp(2)),
            "the import marks the placement, not the restore"
        );
        assert!(restore_deleted(&mut f, &fp(2)).is_none());
    }
}
