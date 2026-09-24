//! What the folder's last run knew: the books it recorded, indexed by the
//! fingerprint a scan compares against, and the lookups the diff asks of it.

use std::collections::HashMap;

use crate::book::{book_rows, Book, Fingerprint, Origin, Row};

/// What the ledger needs to know about a book already in the library: id,
/// address, whether the address is known dead, and — for a stored copy — the
/// address it was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBook {
    pub id: String,
    pub path: String,
    pub missing: bool,
    /// `None` for a linked row: its source is its own address, and the rule
    /// this feeds is about a copy and its original.
    pub source: Option<String>,
}

/// Derived from the book list on every scan rather than persisted; a derived
/// index cannot go stale.
pub type Registry = HashMap<Fingerprint, KnownBook>;

/// The [`Registry`] for a row list, built from its book rows: a link has no
/// fingerprint, so a scan never sees or places one. Two rows can share a
/// fingerprint (duplicates the reader kept); shared rows are registered
/// before independent ones, so a scan always resolves to the shared copy.
pub fn registry_of(rows: &[Row]) -> Registry {
    let mut out = Registry::with_capacity(rows.len());
    let books: Vec<&Book> = book_rows(rows).collect();
    for book in books.iter().filter(|b| !b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    for book in books.iter().filter(|b| b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    out
}

/// The row the library already holds for this content, if any: "are these
/// bytes already a book somewhere", not "is this name on the level I am
/// dropping onto" (that is [`crate::conflict::collide`]).
pub fn existing_for(rows: &[Row], fp: Fingerprint) -> Option<ExistingContent> {
    if fp.mtime_ms == 0 {
        return None;
    }
    // Bound rather than chained: the registry is a temporary, and borrowing
    // out of one in the same expression drops it.
    let registry = registry_of(rows);
    let known = registry.get(&fp)?;
    if known.path.is_empty() {
        return None;
    }
    Some(ExistingContent {
        row_id: known.id.clone(),
        missing: known.missing,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingContent {
    pub row_id: String,
    pub missing: bool,
}

fn known_of(book: &crate::book::Book) -> KnownBook {
    KnownBook {
        id: book.id.clone(),
        path: book.path().to_string(),
        missing: book.missing,
        // A stored row's provenance only: a link's source IS its address, so
        // carrying it would make every linked book look like a copy of itself.
        source: match &book.origin {
            Origin::Stored { src, .. } => src.clone(),
            Origin::Linked { .. } => None,
        },
    }
}

/// Borrowed rather than cloned: the menu asking this opens on a click, and
/// copying a whole library to answer one question is a frame drop. Duplicates
/// resolve to the first row.
pub fn index_by_fp(rows: &[Row]) -> HashMap<Fingerprint, &Book> {
    let mut out = HashMap::with_capacity(rows.len());
    for book in book_rows(rows) {
        out.entry(book.fp).or_insert(book);
    }
    out
}
