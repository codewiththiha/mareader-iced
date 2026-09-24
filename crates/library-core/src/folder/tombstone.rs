//! The stone a removal leaves: the fingerprint that must not come back, and the
//! label the import menu shows for it.

use reader_core::format::Format;
use serde::{Deserialize, Serialize};

use crate::book::{Book, Fingerprint};

/// A book the reader removed, remembered by the folder that placed it. Keeps
/// the file out of every later rescan, and is the record the import menu reads
/// to offer the book back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub fp: Fingerprint,
    /// `None` for a book imported and never opened; the menu falls back to the
    /// file's stem.
    #[serde(default)]
    pub title: Option<String>,
    pub format: Format,
    /// A restore re-measures this address first: a file that has since moved
    /// is a relink, not a restore.
    pub last_path: String,
    /// A restore puts the book back on this shelf if it still exists, else on
    /// the folder's root shelf.
    #[serde(default)]
    pub shelf_id: Option<String>,
    #[serde(default)]
    pub removed_ms: u64,
    /// True when the removal was a move, not a deletion: the library still
    /// holds the book, so the restore menu must not offer it back as gone.
    #[serde(default)]
    pub moved: bool,
    /// The row that represents this file to the folder, when one came home: a
    /// later import lights that row up instead of minting a neighbour beside it.
    #[serde(default)]
    pub returned_row: Option<String>,
}

impl Tombstone {
    /// `shelf_id` is the first of the folder's shelves the book was on: a
    /// book on three of them still comes back to one.
    pub fn of(book: &Book, shelf_id: Option<String>, now_ms: u64) -> Self {
        Self {
            fp: book.fp,
            // The name the shelf showed, not the raw title field: an unopened
            // stored book has no title of its own, and the log would otherwise
            // label itself from the store's `source.pdf`.
            title: Some(book.title()),
            format: book.format,
            last_path: book.path().to_string(),
            shelf_id,
            removed_ms: now_ms,
            moved: false,
            returned_row: None,
        }
    }

    pub fn label(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), &self.last_path)
    }
}
