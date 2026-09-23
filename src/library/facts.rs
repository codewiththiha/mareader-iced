//! The facts about a folder shelf the two layouts paint: the badge saying
//! where its books live, the watch dot, and the counts under the name.
//!
//! Computed fresh on the frame they are asked for — the web app read them
//! back out of its signals by id because a keyed row was not re-created
//! when the shelf's contents changed; a native view rebuilds every frame,
//! so the facts are read rather than remembered and can never go stale.

use library_core::blob::LibraryBlob;
use library_core::governance::Governance;
use library_core::shelf::{badge_kind, children_of, ContentKind};
use library_core::text::plural;

/// The badge's two facts: the words on the chip, and whether the shelf
/// holds both kinds of book — the mixed shelf wears the louder styling, so
/// the reader notices it holds two kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub words: &'static str,
    pub mixed: bool,
}

/// What one folder shelf wears on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderFacts {
    /// Where the books live; `None` for a shelf neither its content nor a
    /// folder answers for — a reader-made shelf wears no badge.
    pub badge: Option<Badge>,
    /// Whether the tree the shelf was cut from tracks the rung it stands
    /// on — the question the dot asks, rung by rung rather than per import.
    pub watched: bool,
    /// Direct members: the summary's first half.
    pub books: usize,
    /// Shelves inside: the summary's second half.
    pub inside: usize,
}

/// One shelf's facts. What the shelf HOLDS decides the badge — a folder
/// whose books are stored copies says so even when its seat still reads in
/// place — and a folder with no books of its own falls back to the
/// governance mode, the promise its import answered with.
pub fn folder_facts(library: &LibraryBlob, shelf_id: &str) -> FolderFacts {
    let kind = badge_kind(&library.books, &library.shelves, shelf_id);
    let governance = Governance::new(&library.folders, &library.shelves);
    let badge = if kind != ContentKind::Empty {
        kind.badge().map(|words| Badge { words, mixed: kind.is_mixed() })
    } else {
        governance.mode_of(shelf_id).map(|mode| Badge { words: mode.badge(), mixed: false })
    };
    FolderFacts {
        badge,
        watched: governance.shelf_tracked(shelf_id),
        books: library
            .shelves
            .iter()
            .find(|shelf| shelf.id == shelf_id)
            .map(|shelf| shelf.books.len())
            .unwrap_or_default(),
        inside: children_of(&library.shelves, Some(shelf_id)).len(),
    }
}

/// The count line, shared by the card and the list row so one shelf cannot
/// describe itself two ways: "3 books · 1 shelf" counts both halves —
/// "3 books" on a folder with shelves inside it would leave out the rest of
/// the library down that path — and a shelf with neither says "Empty".
pub fn summary(books: usize, inside: usize) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if books > 0 {
        parts.push(plural(books, "book", "books"));
    }
    if inside > 0 {
        parts.push(plural(inside, "shelf", "shelves"));
    }
    if parts.is_empty() {
        "Empty".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_line_says_both_halves_and_neither_when_there_is_nothing() {
        assert_eq!(summary(0, 0), "Empty");
        assert_eq!(summary(1, 0), "1 book");
        assert_eq!(summary(3, 0), "3 books");
        assert_eq!(summary(0, 1), "1 shelf");
        assert_eq!(summary(0, 2), "2 shelves");
        assert_eq!(summary(3, 1), "3 books · 1 shelf");
        assert_eq!(summary(1, 4), "1 book · 4 shelves");
    }
}
