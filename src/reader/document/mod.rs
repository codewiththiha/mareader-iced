//! The document the reader has open, in the fields every format shares.
//!
//! A port of the web app's `DocumentState` — the status ladder, the identity,
//! the page count and the geometry — with the parts that belonged to other
//! subsystems left where they live: the search model is `pdf_core`'s, the
//! gloss marks are P7's, and the reflowable content is P4's.
//!
//! The ladder is the whole reason this is a type rather than a handful of
//! fields: `Opening` is what tells the route to show the reading surface
//! before there is anything to read, `Ready` is the only state in which a
//! frame is asked for, and `Error` is a sentence the reader can act on. The
//! web app kept the same three, for the same reasons.

use std::path::PathBuf;

use reader_core::outline::OutlineNode;

use crate::formats::pdf::{PageBox, fallback_box, measured};

use super::Open;

/// How far along a document's open is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocStatus {
    /// Nothing is open: the shelf is what is on screen.
    #[default]
    Idle,
    /// The engine has the file and has not answered yet. The reading surface
    /// is already mounted — a placeholder is what a reader expects to see the
    /// moment they open a book — and no frame has been asked for.
    Opening,
    /// The engine answered: the geometry is known and the pages can be read.
    Ready,
    /// The file could not be read, or there is no engine to read it with. The
    /// sentence is the reader's; the surface shows it and keeps the way back.
    Error,
}

impl DocStatus {
    /// Whether the reading surface may ask the engine for a page. The one
    /// guard in front of every render request.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// The open document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub status: DocStatus,
    /// The address on disk. `None` while nothing is open.
    pub path: Option<PathBuf>,
    /// The page the library remembers for this book, read before the engine
    /// was asked for anything and clamped to the book that actually opened.
    pub resume: u32,
    /// The library row this open belongs to, when the reader named one. The
    /// shelf's Open passes the row it clicked; a drop or a dialog passes
    /// nothing, and the app settles the address onto a shared row before the
    /// open — the web app's rule, kept so a private book of its own is never
    /// hijacked by a drop of the same file.
    pub book_id: Option<String>,
    /// The library's own name for the row, when the reader opened one: the
    /// row's *display* name, so a book copied into the store never reads
    /// "source" in the title bar. It answers for the title only when the
    /// document itself supplied nothing worth showing.
    pub row_title: Option<String>,
    /// The document's own title, when it carries one worth showing.
    pub title: Option<String>,
    pub author: Option<String>,
    pub num_pages: u32,
    /// The page each sheet measured at, in document order. Empty for a
    /// document whose pages could not be measured — the reader then answers
    /// with the A4 fallback for every page rather than laying out against a
    /// zero.
    pub sizes: Vec<PageBox>,
    /// The chapter tree, resolved after the open (the web app asked for it only
    /// once page 1 was on screen, because resolving destinations is not free).
    /// Empty until it answers, and empty for a book without one.
    pub outline: Vec<OutlineNode>,
    /// What went wrong, in the reader's own words.
    pub error: Option<String>,
    /// The name the surfaces show — the bar's centre, the error card, the
    /// window's own title. Kept rather than resolved per frame, because the
    /// rule walks three inputs through a filter and the bar asks for it on
    /// every frame; it is written by the two writes that move the identity,
    /// the open and the seed.
    name: String,
}

impl Document {
    /// What a document being opened looks like before the engine has said
    /// anything about it: the address, the row and its name, and the resume
    /// page the library remembered. Written under `Opening`, so the surface can
    /// paint the book's name and a placeholder at once.
    pub fn begin(&mut self, open: &Open) {
        self.status = DocStatus::Opening;
        self.path = Some(open.path.clone());
        self.book_id = open.book_id.clone();
        self.row_title = open.row_title.clone();
        self.title = None;
        self.author = None;
        self.num_pages = 0;
        self.sizes.clear();
        self.outline.clear();
        self.error = None;
        self.resume = open.resume;
        // The book already has a name at this point — the row's, or the
        // file's — which is what the placeholder shows while the engine reads
        // the file.
        self.refresh_name();
    }

    /// Back to nothing open. Called when the reader leaves for the shelf.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The box of a 1-based page: its measured size, else page 1's, else the
    /// A4 fallback. Never a zero, because the fit arithmetic divides by it.
    pub fn page_box(&self, page: u32) -> PageBox {
        let first = self
            .sizes
            .first()
            .copied()
            .filter(|box_| measured(*box_))
            .unwrap_or_else(fallback_box);
        page.checked_sub(1)
            .and_then(|index| self.sizes.get(index as usize).copied())
            .filter(|box_| measured(*box_))
            .unwrap_or(first)
    }

    /// Re-resolve the name the surfaces show.
    ///
    /// Called wherever the identity moves: the open, which writes the address
    /// and the row's name, and the seed, which writes the document's own title.
    pub fn refresh_name(&mut self) {
        self.name = self.resolve_name();
    }

    /// The name the title bar, the error card and the window title read.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether a document is open at all — the address is written the moment
    /// an open starts, so this is true from the first placeholder frame.
    pub fn is_open(&self) -> bool {
        self.path.is_some()
    }

    /// The name to show for this document, in the order the web app resolved
    /// it: `services::document::open::enter::identity` wrote the document's own
    /// title when it was one worth showing, and the library's name for the row
    /// when it was not; both surfaces then rendered that through
    /// `reader_core::filename::display_name`, which is where the placeholder
    /// filter and the address's stem live.
    ///
    /// The row arrives as its *display* name — the library's `Book::title`,
    /// which falls back to the name the file was imported under rather than the
    /// address it now lives at. That is what keeps a book copied into the store
    /// from reading `source` in the title bar: the copy's address is the
    /// store's own `source.pdf`, and the row's name is the only thing that
    /// knows better.
    fn resolve_name(&self) -> String {
        // The document's own title, filtered: a PDF's `/Title` is free-form
        // and frequently a download's file name, which is the whole reason
        // `is_usable_title` exists.
        if let Some(own) = reader_core::filename::document_title(self.title.as_deref()) {
            return own;
        }
        // Then the library's name for the row, through the same filter: a
        // stored name is a promise, but it still has to look like a name.
        if let Some(row) = self.row_title.as_deref().map(str::trim).filter(|t| !t.is_empty())
            && let Some(named) = reader_core::filename::document_title(Some(row))
        {
            return named;
        }
        // Last, the address: the file's stem where the open had no row to name
        // it — a drop, a picker, a recent address.
        let address = self.path.as_deref().and_then(std::path::Path::to_str);
        reader_core::filename::display_name(None, address)
            .unwrap_or_else(|| "the document".to_string())
    }
}

#[cfg(test)]
mod tests;
