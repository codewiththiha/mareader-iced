//! Reading a document: what it is, how it is shaped, and the words it says.
//! Everything here is also the place the engine's advice is written.

use pdfium_render::prelude::{PdfBookmark, PdfDocument, PdfDocumentMetadataTagType, PdfPage};
use std::path::{Path, PathBuf};
use crate::formats::pdf::protocol::{Opened, PageBox};

const MAX_DEPTH: u32 = 8;

const MAX_ENTRIES: usize = 2_000;

/// Everything the open flow needs, read while the document is in hand: the page
/// count, the geometry of every page, and the document's own idea of its title
/// and author — so no later request has to ask again for a fact that was free
/// at open time.
pub(super) fn read_open(path: &Path, document: &PdfDocument<'_>) -> Opened {
    let pages = document.pages();
    let num_pages = pages.len().max(0) as u32;
    let mut page_sizes = Vec::with_capacity(num_pages as usize);
    for index in 0..num_pages as i32 {
        // Per page rather than in one sweep, because one page whose box Pdfium
        // refuses must not cost the book its geometry: the vector is filled in
        // order, and a page that fails carries a zero box the reader reads as
        // "unmeasured".
        //
        // The box is the page as the reader will see it: Pdfium reports the
        // media box *after* the page's own `/Rotate`, so a scanned landscape
        // sheet arrives as 792 × 612 rather than as the 612 × 792 its
        // dictionary spells. That is also why nothing here rotates anything.
        let box_ = pages
            .page_size(index)
            .map(|rect| PageBox {
                width: f64::from(rect.width().value),
                height: f64::from(rect.height().value),
            })
            .unwrap_or(PageBox {
                width: 0.0,
                height: 0.0,
            });
        page_sizes.push(box_);
    }
    Opened {
        path: path.to_path_buf(),
        num_pages,
        page_sizes,
        title: metadata_field(document, PdfDocumentMetadataTagType::Title),
        author: metadata_field(document, PdfDocumentMetadataTagType::Author),
    }
}

/// One field of the document's information dictionary, trimmed, and absent when
/// the document carries nothing usable there.
///
/// It reads through the tag list rather than through named accessors because
/// that is the only door the crate leaves open in 0.9 — `metadata().title()` and
/// friends were removed along with the rest of the deprecated surface — and
/// because a document whose `/Title` is a single space has no title: the reader
/// then falls back to the library's own name for the book.
fn metadata_field(
    document: &PdfDocument<'_>,
    tag: PdfDocumentMetadataTagType,
) -> Option<String> {
    let metadata = document.metadata();
    let found = metadata.get(tag)?;
    usable(Some(found.value()))
}

/// The document's chapter tree, flattened in document order with each
/// chapter's depth — the shape `pdf_core::outline::to_nodes` cleans into the
/// reader's own nodes.
///
/// Walked by hand rather than through the crate's own depth-first iterator, for
/// the one thing that iterator does not report: depth. Two ceilings keep a
/// malformed outline from becoming a liability — a graph that points at itself
/// stops at [`MAX_DEPTH`], and a pathological book stops at [`MAX_ENTRIES`].
pub(super) fn outline_of(document: &PdfDocument<'_>) -> Vec<pdf_core::outline::OutlineEntry> {
    let mut entries = Vec::new();
    let Some(root) = document.bookmarks().root() else {
        return entries;
    };
    // `root()` answers the first bookmark of the top level; its siblings are
    // the rest of that level.
    let siblings = root.iter_siblings();
    for node in std::iter::once(root).chain(siblings) {
        push_bookmark(&node, 0, &mut entries);
    }
    entries
}

fn push_bookmark(
    node: &PdfBookmark<'_>,
    depth: u32,
    entries: &mut Vec<pdf_core::outline::OutlineEntry>,
) {
    if entries.len() >= MAX_ENTRIES || depth > MAX_DEPTH {
        return;
    }
    let title = node.title().unwrap_or_default().trim().to_string();
    if !title.is_empty() {
        // A bookmark whose destination never resolved arrives as page 0, which
        // `to_nodes` drops: a chapter nobody can jump to is not a chapter the
        // panel should offer.
        let page = node
            .destination()
            .and_then(|destination| destination.page_index().ok())
            .map(|index| index.max(0) as u32 + 1)
            .unwrap_or(0);
        entries.push(pdf_core::outline::OutlineEntry { title, page, depth });
    }
    for child in node.iter_direct_children() {
        push_bookmark(&child, depth + 1, entries);
    }
}

/// The page at a 1-based number, as the reader counts pages.
///
/// The returned page carries the document's own lifetime rather than the
/// borrow of the reference: Pdfium's handles are the document's, and the
/// caller only ever uses the page while the document it came from is in hand.
pub(super) fn page_of<'a>(document: &PdfDocument<'a>, page: u32) -> Result<PdfPage<'a>, String> {
    let index = page.saturating_sub(1).min(i32::MAX as u32) as i32;
    document
        .pages()
        .get(index)
        .map_err(|error| error.to_string())
}

/// A metadata field worth showing: trimmed, and absent when blank. A document
/// whose title is a single space has no title, and the reader then falls back
/// to the library's own name for the book.
fn usable(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Pdfium reports errors as codes; the reader gets a sentence, and Pdfium's own
/// words are kept inside it for a bug report.
pub(super) fn readable_error(path: &Path, error: &str) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| PathBuf::from(path).display().to_string());
    format!("Could not open {name}: {error}")
}
