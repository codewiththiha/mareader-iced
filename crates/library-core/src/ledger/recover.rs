//! What a folder can still give back: the books a removal took, gathered from
//! its stones, and the relink that puts one back on its shelf.

use std::collections::HashMap;

use crate::book::{book_rows_mut, Book, Fingerprint, Row};
use crate::folder::{Tombstone, WatchedFolder};
use crate::shelf::Shelf;

/// A book this folder could give the reader back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// Removed by the reader and still not in the library. The file may or
    /// may not still be on disk; a restore measures before it promises.
    Deleted(Tombstone),
    /// Still in the library and still inside this folder on disk, but no
    /// longer on any shelf this folder owns: the reader moved it elsewhere in
    /// the app. Nothing is re-imported — the offer is to show it here as well,
    /// or to go look at where it went.
    Moved {
        book_id: String,
        /// The name the shelf shows: the document's title when it had one,
        /// else the stem the reader knows — a stored book's source, never the
        /// store's own `source.pdf`.
        title: Option<String>,
        path: String,
        /// The first shelf the book is on, by name, for the row's "now in
        /// Fiction" half. `None` when the book is on no shelf.
        home_shelf: Option<String>,
    },
}

/// What this folder's import menu can offer to give back.
///
/// Pure and synchronous by design: the menu opens on a click and answers from
/// the last scan's `last_seen` plus the folder's tombstones — a walk over two
/// short lists, not over a directory tree. A restore re-measures the one file
/// it is about to import, which is where freshness actually matters.
///
/// Takes the shelf list rather than a membership callback: which shelves a
/// book is on, and which the folder owns, are rules [`crate::shelf`] owns.
/// "First membership" is shelf order, so the row's home is deterministic.
pub fn recoverables(
    folder: &WatchedFolder,
    books_by_fp: &HashMap<Fingerprint, &Book>,
    shelves: &[Shelf],
) -> Vec<Recovered> {
    let mut out = Vec::new();

    // Removed books first: an offer to give something back outranks an offer
    // to show you what you already have.
    for entry in &folder.ignored {
        if books_by_fp.contains_key(&entry.fp) {
            continue;
        }
        // A moved-out log is not a removal: the library still holds the book.
        if entry.moved {
            continue;
        }
        out.push(Recovered::Deleted(entry.clone()));
    }

    let owned_by_folder = |shelf: &Shelf| shelf.kind.folder_id() == Some(folder.id.as_str());
    for (fp, path) in &folder.last_seen {
        let Some(book) = books_by_fp.get(fp) else {
            continue;
        };
        // The card already offers a relink; a second door here would not know
        // the address is bad.
        if book.missing {
            continue;
        }
        let on = crate::shelf::containing(shelves, &book.id);
        if on.iter().any(|shelf| owned_by_folder(shelf)) {
            continue;
        }
        out.push(Recovered::Moved {
            book_id: book.id.clone(),
            // The display name: the menu's `path` fallback would read the
            // store's own `source.pdf` for a stored book.
            title: Some(book.title()),
            path: path.clone(),
            home_shelf: on.first().map(|shelf| shelf.name.clone()),
        });
    }
    out
}

/// Apply a `Relink`: rewrite the address, clear `missing`, keep everything
/// else. A stored book does not take the new address — its bytes are the app's
/// own copy — so only its recorded source moves.
pub fn relink(rows: &mut [Row], book_id: &str, to: &str) -> bool {
    // A link has no address to move, so `book_rows_mut` is the whole guard.
    let Some(book) = book_rows_mut(rows).find(|b| b.id == book_id) else {
        return false;
    };
    match &mut book.origin {
        crate::book::Origin::Linked { src } => {
            *src = to.to_string();
            book.missing = false;
        }
        crate::book::Origin::Stored { src, .. } => {
            *src = Some(to.to_string());
        }
    }
    true
}
