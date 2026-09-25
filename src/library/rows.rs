//! What a level holds: its folders and its books, in the level's own order and
//! narrowed by the query, plus the selection and drag facts the cells read.

use crate::library::drag::{DragPayload, DropEffect};
use library_core::blob::LibraryBlob;
use library_core::book::Row;
use library_core::shelf::{ALL_SHELF, find, members_of};
use library_core::sort::SortKey;
use library_core::{query, sort};
use std::collections::HashSet;

/// What the level's cells read about a selection: the mode, and the set.
/// One value threaded through both layouts so a grid and a list cannot
/// disagree about who is dimmed, who wears the ring, and where a
/// right-click lands. Borrowed on purpose — the set outlives every frame
/// that paints it.
#[derive(Clone, Copy)]
pub struct SelectionFacts<'a> {
    pub selecting: bool,
    pub selected: &'a HashSet<String>,
    /// What the reveal lit: the cell wears the membership's own ring on the
    /// level the reader just arrived at, so the answer flashes where the
    /// eye was taken. The reveal's own light, in the sheet's own promise.
    pub lit: Option<&'a str>,
}

/// What the level's cells read about a drag in flight: the payload, so
/// every held cell can fade, and the effect a release would commit right
/// now, so the seam, the ring and the tint land on exactly the cells the
/// commit would write. Recomputed per frame by the app from one table —
/// the cells ask questions, they never answer them. Borrowed like the
/// selection's set: both outlive every frame that paints them.
#[derive(Clone, Copy)]
pub struct DragFacts<'a> {
    pub payload: Option<&'a DragPayload>,
    pub effect: Option<&'a DropEffect>,
}

impl DragFacts<'_> {
    /// A drag is in flight: the list's band sensors wear themselves, and
    /// the floor stops taking presses.
    pub fn live(&self) -> bool {
        self.payload.is_some()
    }

    pub fn holds(&self, id: &str) -> bool {
        self.payload.is_some_and(|held| held.contains(id))
    }

    pub fn inserts_before(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::insert_at) == Some((id, false))
    }

    pub fn inserts_after(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::insert_at) == Some((id, true))
    }

    pub fn sibling_before(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::sibling_at) == Some((id, false))
    }

    pub fn sibling_after(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::sibling_at) == Some((id, true))
    }

    pub fn nests_into(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::nest_into) == Some(id)
    }

    pub fn folds_with(&self, id: &str) -> bool {
        self.effect.is_some_and(|effect| match effect {
            DropEffect::CreateFolder { with_book_id } => with_book_id.as_str() == id,
            _ => false,
        })
    }
}

/// The order the level paints, in the four steps the web app ran them:
/// membership first (a shelf reads its own list; the library's root reads
/// the unfiled, or everything when a search is on), then the view's sort,
/// then the query's filter.
pub fn level_rows(library: &LibraryBlob, shelf: &str, terms: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = if shelf == ALL_SHELF {
        if query::is_active(terms) {
            library.books.clone()
        } else {
            let unfiled: HashSet<String> = members_of(&library.books, &library.shelves, ALL_SHELF)
                .into_iter()
                .map(str::to_string)
                .collect();
            library
                .books
                .iter()
                .filter(|row| unfiled.contains(row.id()))
                .cloned()
                .collect()
        }
    } else {
        let members = find(&library.shelves, shelf)
            .map(|shelf| shelf.books.clone())
            .unwrap_or_default();
        sort::ordered(&library.books, &members, SortKey::Manual, true)
    };
    sort::sort_rows(&mut rows, library.view.sort, library.view.sort_asc);
    query::filter(&rows, terms)
}

/// The shelves hanging off the level the page is on, narrowed by the query
/// when one is on — the same rule both layouts draw against.
pub fn level_folders(library: &LibraryBlob, shelf: &str, terms: &str) -> Vec<library_core::shelf::Shelf> {
    let parent = (shelf != ALL_SHELF).then_some(shelf);
    library_core::shelf::children_of(&library.shelves, parent)
        .into_iter()
        .filter(|child| !query::is_active(terms) || query::matches_terms(&child.name, terms))
        .cloned()
        .collect()
}
