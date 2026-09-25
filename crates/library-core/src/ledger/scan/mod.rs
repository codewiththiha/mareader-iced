//! The scan's own decision: one pure diff of a folder against the ledger, and
//! the actions it hands back — import, copy, skip, or heal a relink.

use std::collections::HashSet;

use crate::book::{book_rows, find_by_id, Fingerprint, Origin, Row};
use crate::folder::WatchedFolder;
use crate::scan::FoundFile;
use super::registry::{KnownBook, Registry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanAction {
    /// A book the library does not have. The caller mints the row and records
    /// the fingerprint in [`crate::folder::WatchedFolder::placed`].
    Add(FoundFile),
    /// A known book whose address moved on disk: rewrite the address; leave
    /// the id, resume point and shelf memberships alone.
    Relink { book_id: String, to: String },
    /// The common case: a rescan of an unchanged folder is a list of these.
    Skip,
}

/// Decide what a folder's scan does, file by file, in walk order. Pure: reads
/// the folder's ledger and the global registry, answers one [`ScanAction`]
/// per found file.
pub fn diff_folder(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide(folder, registry, file));
    }
    out
}

/// One file's decision, split out of [`diff_folder`] so a test can name the
/// case it asserts instead of building a whole walk for it.
pub fn decide(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    // A tombstone wins over everything, including content this folder never
    // placed: the reader removed this file and it is still on disk.
    if folder.is_ignored(&file.fp) {
        return ScanAction::Skip;
    }
    match registry.get(&file.fp) {
        None => {
            // Content this folder placed before whose row is gone (a storage
            // trim, a hand-edited blob): do not resurrect it.
            if folder.placed.contains(&file.fp) {
                ScanAction::Skip
            } else if folder.tracks_rung(&folder.shelf_key(file)) {
                ScanAction::Add(file.clone())
            } else {
                // An untracked rung adds nothing: a subfolder turned off
                // under a watched root is off for the walk too. An explicit
                // import answers through [`decide_import`] instead.
                ScanAction::Skip
            }
        }
        Some(known) => known_action(folder, known, file),
    }
}

/// A known fingerprint skips at its own address and relinks when the address
/// moved.
fn known_action(folder: &WatchedFolder, known: &KnownBook, file: &FoundFile) -> ScanAction {
    if known.path == file.path {
        return ScanAction::Skip;
    }
    // The registry named the library's own copy of the file this walk stands
    // on: two addresses, not a move — stay quiet.
    if known.source.as_deref() == Some(file.path.as_str()) {
        return ScanAction::Skip;
    }
    // The address moved, and this folder placed the book or it is already
    // known missing: relink.
    if folder.placed.contains(&file.fp) || known.missing {
        ScanAction::Relink {
            book_id: known.id.clone(),
            to: file.path.clone(),
        }
    } else {
        ScanAction::Skip
    }
}

/// An explicit import's decision: the same questions as [`decide`], except a
/// tombstone is lifted rather than honoured (the lift lands via
/// [`restore_deleted`]) and content another folder placed is still added.
pub(super) fn decide_import(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    match registry.get(&file.fp) {
        None => ScanAction::Add(file.clone()),
        Some(known) => {
            let action = known_action(folder, known, file);
            // The one answer the two tables share differently: a rescan stays
            // quiet about content another folder placed, but an explicit
            // import is the reader asking for THIS folder, and a
            // byte-identical copy of another folder's book is still a file
            // this folder has.
            match action {
                ScanAction::Skip if known.path != file.path => ScanAction::Add(file.clone()),
                other => other,
            }
        }
    }
}

/// [`diff_folder`] for a run the reader asked for by name.
pub fn diff_import(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide_import(folder, registry, file));
    }
    out
}

/// The found files an explicit copies run owes a book of its own: the ones
/// the library already reads in place. This filter is the whole of what
/// separates "a second instance the reader asked for" from "the instance the
/// library already has".
pub fn copy_over_paths(found: &[FoundFile], registry: &Registry, rows: &[Row]) -> HashSet<String> {
    found
        .iter()
        .filter(|file| {
            registry.get(&file.fp).is_some_and(|known| {
                find_by_id(rows, &known.id).is_some_and(|b| {
                    matches!(b.origin, Origin::Linked { .. }) && b.path() == file.path
                })
            })
        })
        .map(|file| file.path.clone())
        .collect()
}

/// The files an unbound copies run owes a book of its own: one per
/// fingerprint, first found wins. "Unbound" means the run must not touch a
/// standing tree's ledger — a copies import of ground a read-at-place tree
/// still reads, whose *as new* answer is a second shelf rather than a rewrite
/// of the first.
pub fn unbound_copies(found: &[FoundFile], registry: &Registry, rows: &[Row]) -> Vec<FoundFile> {
    let copy_paths = copy_over_paths(found, registry, rows);
    let mut seen: HashSet<Fingerprint> = HashSet::new();
    let mut out = Vec::new();
    for file in found {
        let owed = match registry.get(&file.fp) {
            None => true,
            Some(known) => {
                if known.source.as_deref() == Some(file.path.as_str()) {
                    false
                } else if known.path == file.path {
                    copy_paths.contains(&file.path)
                } else {
                    !known.missing
                }
            }
        };
        if owed && seen.insert(file.fp) {
            out.push(file.clone());
        }
    }
    out
}

/// The linked rows a *replace* of this tree sweeps through the removal pass
/// first, so the copies that land come back in the names the shelves showed.
pub fn linked_rows_of(rows: &[Row], placed: &HashSet<Fingerprint>) -> Vec<String> {
    book_rows(rows)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && placed.contains(&b.fp))
        .map(|b| b.id.clone())
        .collect()
}

/// Drop the relinks that would point a book at an address another row reads.
/// Two rows can hold one fingerprint, so a walk of the other folder would
/// rewrite the first one's address out from under it.
pub fn keep_healable_relinks(relinks: &mut Vec<(String, String)>, rows: &[Row]) {
    relinks.retain(|(_, to)| !book_rows(rows).any(|b| b.path() == to.as_str()));
}

#[cfg(test)]
mod tests;
