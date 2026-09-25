//! What a drag holds and what a drop means.
//!
//! The decision is pure — four facts go in (what is held, what is under the
//! pointer, which part of the target's box the pointer is on, and how long
//! it has rested there) and one answer comes out. No window, no widget and
//! no state is read here, which makes the table testable on the host rather
//! than through a running app.
//!
//! The web app's `dnd/effect.rs`, carried over whole: the same variants,
//! the same order of refusals, the same tests.

use crate::library::card::THUMB_CAP;

/// Two, because a shelf of one is a shelf the view menu already makes. The
/// book under the pointer counts as the second, so the offer starts at the
/// first item held.
pub const FOLD_MIN_ITEMS: usize = 2;

/// One struct rather than a book-or-folder enum, because a selection holds
/// both: every move is a pair of operations on one shelf list, so a
/// pre-split payload is one the commit step does not have to sort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DragPayload {
    pub books: Vec<String>,
    pub folders: Vec<String>,
    /// What a move takes its books off: a drag lifted inside a shelf is a
    /// move out of that shelf, and reading the open level instead would
    /// unfile a book from the shelf it was showing in.
    pub source: Option<String>,
}

impl DragPayload {
    pub fn len(&self) -> usize {
        self.books.len() + self.folders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Asked by every cell on every frame of a drag: a question about the
    /// payload rather than a derived set of its own.
    pub fn contains(&self, id: &str) -> bool {
        self.books.iter().any(|each| each.as_str() == id)
            || self.folders.iter().any(|each| each.as_str() == id)
    }
}

/// What a drop can land on. `Shelf` and `Ellipsis` are the breadcrumb's —
/// they arrive with the crumb targets the bar's geometry owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetKind {
    Book,
    Folder,
    /// The way back to a level is also a way to file what is held onto that
    /// level from anywhere in the library — the bar's crumbs, and the
    /// fold panel's.
    Shelf,
    /// A hover target, not a drop: resting on it during a drag opens the
    /// panel, because a drag raises no hover of its own.
    Ellipsis,
    Level,
}

/// Which part of the target's box the pointer is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Top,
    Middle,
    Bottom,
}

impl Band {
    pub fn after(self) -> bool {
        self == Band::Bottom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropEffect {
    InsertBefore {
        book_id: String,
        /// Named by the effect rather than re-derived at the commit: the
        /// container a row renders in is not always the open level.
        shelf: Option<String>,
        after: bool,
    },
    ShelfSibling { anchor_id: String, after: bool },
    FileToShelf { shelf_id: String },
    NestInto { folder_id: String },
    CreateFolder { with_book_id: String },
    /// Distinct from "nothing under the pointer" (a `None` effect), so a
    /// card can tell "not a target" from "a target that says no".
    Refused,
}

impl DropEffect {
    pub fn insert_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::InsertBefore { book_id, after, .. } => Some((book_id.as_str(), *after)),
            _ => None,
        }
    }

    pub fn sibling_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::ShelfSibling { anchor_id, after } => Some((anchor_id.as_str(), *after)),
            _ => None,
        }
    }

    pub fn nest_into(&self) -> Option<&str> {
        match self {
            DropEffect::NestInto { folder_id } => Some(folder_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldPreview {
    /// Capped at the plate's own cap: a preview showing more cells would
    /// preview a shape the library does not draw.
    pub filled: usize,
    pub with_book_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropQuery<'a> {
    pub held_books: usize,
    pub held_folders: usize,
    pub target_kind: DropTargetKind,
    pub target_id: &'a str,
    pub target_is_held: bool,
    pub can_nest: bool,
    /// The parent's `can_nest` answer: filing beside a shelf is filing into
    /// the level that holds it.
    pub can_sibling: bool,
    pub band: Band,
    pub target_shelf: Option<&'a str>,
    pub dwell_armed: bool,
}

impl DropQuery<'_> {
    pub fn held(&self) -> usize {
        self.held_books + self.held_folders
    }
}

/// Held items plus the target. A target that counted itself made a drag of
/// two onto one a shelf holding that member twice.
pub fn fold_items(query: &DropQuery<'_>) -> usize {
    query.held() + 1
}

pub fn drop_effect(query: DropQuery<'_>) -> DropEffect {
    match query.target_kind {
        DropTargetKind::Book => {
            let id = query.target_id;
            let shelf = query.target_shelf.map(str::to_string);
            // A book the pointer is already carrying is a position and never
            // a partner — including a single book dragged onto itself, which
            // lands where it started: the no-op it looks like.
            if query.target_is_held {
                return DropEffect::InsertBefore {
                    book_id: id.to_string(),
                    shelf,
                    after: false,
                };
            }
            // A row is a seam between books and a shelf is not a book: no
            // position to take and no fold to brew.
            if query.held_books == 0 {
                return DropEffect::Refused;
            }
            // The dwell is the whole difference, and not a refinement:
            // without it a reorder would be unreachable, because every card
            // a drag crossed would offer a new shelf.
            if query.dwell_armed && fold_items(&query) >= FOLD_MIN_ITEMS {
                return DropEffect::CreateFolder { with_book_id: id.to_string() };
            }
            DropEffect::InsertBefore {
                book_id: id.to_string(),
                shelf,
                after: query.band.after(),
            }
        }
        DropTargetKind::Folder => {
            let id = query.target_id;
            // A book is always welcome; a shelf is welcome unless filing it
            // here would put it inside itself — a folder no level renders
            // and a reader can never open again.
            if query.band == Band::Middle || query.held_books > 0 || query.held_folders == 0 {
                if query.held_folders > 0 && !query.can_nest {
                    return DropEffect::Refused;
                }
                return DropEffect::NestInto { folder_id: id.to_string() };
            }
            if query.target_is_held || !query.can_sibling {
                return DropEffect::Refused;
            }
            DropEffect::ShelfSibling { anchor_id: id.to_string(), after: query.band.after() }
        }
        DropTargetKind::Shelf | DropTargetKind::Level => {
            DropEffect::FileToShelf { shelf_id: query.target_id.to_string() }
        }
        DropTargetKind::Ellipsis => DropEffect::Refused,
    }
}

pub fn fold_preview(items: usize, with_book_id: &str) -> Option<FoldPreview> {
    (items >= FOLD_MIN_ITEMS).then(|| FoldPreview {
        filled: items.min(THUMB_CAP),
        with_book_id: with_book_id.to_string(),
    })
}

#[cfg(test)]
mod tests;
