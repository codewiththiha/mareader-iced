//! The book schema: [`Fingerprint`], [`Origin`], [`Book`] and the [`Row`] a
//! library list holds. The rules that act on rows live one module per question —
//! [`read`], [`merge`], [`check`], [`sanitize`], [`naming`], [`query`] — with the
//! cases for all of it in `tests.rs`.

pub mod check;
pub mod merge;
pub mod naming;
pub mod query;
pub mod read;
pub mod sanitize;
pub use check::{add_book, apply_check, drop_dangling_links, drop_dead_shelf_links, remove_row};
pub use merge::{fold_books, further_point};
pub use naming::{duplicate_title, stem_of};
pub use query::{find_book_mut, find_by_id, find_by_path, index_by_id, resume_point};
pub use read::{ReadPoint, record_read, record_read_row, rows_for_read};
pub use sanitize::{sanitize};

#[cfg(test)]
mod tests;

use reader_core::format::Format;
use serde::{Deserialize, Serialize};

/// Storage guard on the library's size, not a "recent books" cap: it keeps the
/// persisted blob inside the browser's quota. Past it, the least-recently-read
/// books go.
pub const BOOKS_CAP: usize = 2000;

/// A book's content identity, compared against files on disk during a rescan.
/// `size` + `mtime_ms` survive a move; `head_hash` separates two different
/// books with the same length and stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

/// How the app holds a book's bytes: [`Origin::Linked`] is the path itself
/// (the default), [`Origin::Stored`] is a copy inside the app's own store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// The book is the path; the app never moves, renames or deletes it. A
    /// vanished path is a missing book with a Relink affordance.
    Linked { src: String },
    /// A copy in the store. `src` is provenance — what the copy was made from,
    /// kept so Relink can copy again while the source still exists.
    Stored { src: Option<String>, store: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// Stable through a relink, a rename and a move between shelves; it is the
    /// drag payload and the shelf member.
    pub id: String,
    pub fp: Fingerprint,
    /// The reader's rename ([`Book::title_locked`]), else the title captured at
    /// open time. `None` until one of the two happens, which is why
    /// [`Book::title`] falls back to the stem rather than storing a guess.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    pub format: Format,
    pub origin: Origin,
    /// The "Date added" sort's key.
    #[serde(default)]
    pub added_ms: u64,
    /// `0` for a book imported but never read, which sorts last rather than first.
    #[serde(default)]
    pub last_read_ms: u64,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    /// Written only while stream mode is live; `None` elsewhere, where `page`
    /// is the whole truth.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// Set by a path check, never by a scan: a missing book keeps its row and
    /// shelf membership so Relink can heal it.
    #[serde(default)]
    pub missing: bool,
    /// Placeholder mark a migrated book carries until the first path check
    /// replaces it. No rescan may run while one is set: a real fingerprint
    /// never matches a placeholder, so the folder would re-add books the reader
    /// already has.
    #[serde(default)]
    pub fp_pending: bool,
    /// Marks a copy the conflict sheet answered *as new*. Two rows of one
    /// address are otherwise twins (reads, path checks, highlights and covers
    /// are all keyed by the address); an independent row reads, resumes and
    /// moves on its own.
    #[serde(default)]
    pub independent: bool,
    /// A name the reader typed or accepted from the conflict sheet.
    /// [`crate::book::sanitize`] treats it as a promise: its rule that drops
    /// filename-shaped titles heals download debris, and a chosen name is not
    /// debris.
    #[serde(default)]
    pub title_locked: bool,
}

fn default_page() -> u32 {
    1
}

/// One row of the library's list: a [`Book`], or a [`Row::Link`] pointing at
/// one. A link is the row a second copy of one file would otherwise have been;
/// it has no fingerprint, address or resume point of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    Book(Book),
    /// A pointer at a book. Nothing measures it; its name is the target's at
    /// the moment the link was made.
    #[serde(rename_all = "camelCase")]
    Link {
        id: String,
        name: String,
        target: String,
        #[serde(default)]
        added_ms: u64,
    },
}

/// What every content rule walks: a link has no fingerprint to compare, no
/// address to check and no resume point to write.
pub fn book_rows(rows: &[Row]) -> impl Iterator<Item = &Book> {
    rows.iter().filter_map(Row::book)
}

pub fn book_rows_mut(rows: &mut [Row]) -> impl Iterator<Item = &mut Book> {
    rows.iter_mut().filter_map(Row::as_book_mut)
}

pub fn find_row<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.id() == id)
}

pub fn find_row_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Row> {
    rows.iter_mut().find(|r| r.id() == id)
}

impl Fingerprint {
    pub fn of(size: u64, mtime_ms: u64, head: &[u8]) -> Self {
        Self {
            size,
            mtime_ms,
            head_hash: crate::hash::head_hash(head),
        }
    }

    /// Stand-in fingerprint for a book known by address only (a row migrated
    /// from the `v1` schema, which measured nothing). Derived from the address
    /// so two books never share one; `mtime_ms == 0` marks it as a placeholder.
    pub fn placeholder(path: &str) -> Self {
        let bytes = path.as_bytes();
        Self {
            size: bytes.len() as u64,
            mtime_ms: 0,
            head_hash: crate::hash::head_hash(bytes),
        }
    }
}

impl Origin {
    /// The address every other layer speaks: cover cache keys, resume-point
    /// lookups and the shell's `read_file_*` gate all use it.
    pub fn path(&self) -> &str {
        match self {
            Origin::Linked { src } => src,
            Origin::Stored { store, .. } => store,
        }
    }

    pub fn source(&self) -> Option<&str> {
        match self {
            Origin::Linked { src } => Some(src),
            Origin::Stored { src, .. } => src.as_deref(),
        }
    }

    pub fn is_stored(&self) -> bool {
        matches!(self, Origin::Stored { .. })
    }

    /// Whether this is the library's own copy of `path`. A folder's moved-out
    /// log is only honest when the survivor really is a copy of the file the
    /// dissolving row read.
    pub fn is_store_copy_of(&self, path: &str) -> bool {
        matches!(self, Origin::Stored { src: Some(src), .. } if src == path)
    }
}

impl Row {
    pub fn link(id: String, name: String, target: String, added_ms: u64) -> Self {
        Row::Link {
            id,
            name,
            target,
            added_ms,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Row::Book(b) => &b.id,
            Row::Link { id, .. } => id,
        }
    }

    /// The name the shelf shows and a collision compares: a book's title or
    /// address stem (never empty), a link's made-with name.
    pub fn display_name(&self) -> String {
        match self {
            Row::Book(b) => b.title(),
            Row::Link { name, .. } => name.clone(),
        }
    }

    pub fn is_link(&self) -> bool {
        matches!(self, Row::Link { .. })
    }

    pub fn book(&self) -> Option<&Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn as_book_mut(&mut self) -> Option<&mut Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn fp(&self) -> Option<Fingerprint> {
        match self {
            Row::Book(b) => Some(b.fp),
            Row::Link { .. } => None,
        }
    }

    pub fn target(&self) -> Option<&str> {
        match self {
            Row::Link { target, .. } => Some(target),
            Row::Book(_) => None,
        }
    }

    pub fn added_ms(&self) -> u64 {
        match self {
            Row::Book(b) => b.added_ms,
            Row::Link { added_ms, .. } => *added_ms,
        }
    }
}

impl Book {
    /// One constructor for every importing path, so a joined book always
    /// starts at page 1, with no title of its own and not missing.
    pub fn new(id: String, fp: Fingerprint, format: Format, origin: Origin, added_ms: u64) -> Self {
        Self {
            id,
            fp,
            title: None,
            author: None,
            format,
            origin,
            added_ms,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
            title_locked: false,
        }
    }

    pub fn path(&self) -> &str {
        self.origin.path()
    }

    /// The name to show: the document's title, else the file stem, else the
    /// address — never empty. A stored book falls back to its source's stem,
    /// since store bytes are always named `source.<ext>`.
    pub fn title(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), self.name_source())
    }

    /// The stem of this book's address: the name an untitled book shows and a
    /// collision compares.
    pub fn stem(&self) -> String {
        stem_of(self.name_source())
    }

    /// One spelling so [`Book::title`] and [`Book::stem`] cannot fall back to
    /// different names.
    fn name_source(&self) -> &str {
        match &self.origin {
            Origin::Stored { src: Some(src), .. } => src,
            origin => origin.path(),
        }
    }

    pub fn author(&self) -> Option<String> {
        crate::text::non_blank(self.author.as_deref()).map(str::to_string)
    }

    pub fn progress(&self) -> Option<f64> {
        if self.num_pages == 0 {
            return self.fraction.filter(|f| (0.0..=1.0).contains(f));
        }
        Some((self.page.min(self.num_pages) as f64 / self.num_pages as f64).clamp(0.0, 1.0))
    }

    /// Take a measurement of this book's own bytes as its identity. A stored
    /// row's fingerprint is the copy's, which leaves the source address free
    /// for whatever folder reads it — a departure can leave the book on its
    /// shelf and still hand the OS file back to the folder's ledger.
    pub fn adopt_measurement(&mut self, measured: Option<Fingerprint>) {
        match measured {
            Some(fp) => {
                self.fp = fp;
                self.fp_pending = false;
            }
            None => self.fp_pending = true,
        }
    }

    /// Make this row the library's own copy of the file it reads. Everything
    /// the reader put into the row travels with it; the store name is minted
    /// from the row's id rather than its title.
    pub fn become_stored(&mut self, src: &str, store: String, measured: Option<Fingerprint>) {
        if self.title.is_none() {
            self.title = Some(crate::text::display_or_stem(None, src));
        }
        self.origin = Origin::Stored {
            src: Some(src.to_string()),
            store,
        };
        self.adopt_measurement(measured);
        self.missing = false;
    }

    /// The heal every path check and folder walk goes through: the row's
    /// identity becomes the measurement, the placeholder mark goes, and the
    /// address is not missing. A `v1` placeholder never matched a measurement,
    /// so this is also what stops a rescan from adding a second copy.
    pub fn heal(&mut self, fp: Fingerprint) {
        self.fp = fp;
        self.fp_pending = false;
        self.missing = false;
    }
}

#[cfg(test)]
mod tests;
