//! What one entry's duplicate turns out to be, planned against the lists as they
//! stand: a book, a shelf, or a link whose target decides what it can become.

use library_core::book::{find_row, Book, Row};
use library_core::id;
use library_core::shelf::Shelf;

use super::{plan_tree, TreePlan};

/// What one entry's duplicate turns out to be, planned against the lists as
/// they stand. The app turns each answer into either a landing on the spot
/// or a store batch with a landing behind it.
#[derive(Debug)]
pub enum DupPlan {
    /// A second link at the same shelf, filed beside the first: the one
    /// duplicate that stays a pointer, owing no bytes to the store.
    ShelfLink { row_id: String, name: String, target: String },
    /// One book's copy: the bytes the store will copy and the row the copy
    /// lands beside.
    Book(Box<BookCopy>),
    /// A whole tree: the fresh subtree and every member of it resolved,
    /// waiting on the one store batch the run owes.
    Tree(TreePlan),
    /// Nothing to copy and nothing to say: a book whose address died (the
    /// menu row was disabled before the click could land) or a shelf that is
    /// not there.
    Skip,
    /// A link whose target is gone — the one refusal said out loud, wearing
    /// the toast's own sentence.
    Dead(String),
}

/// One book's copy, resolved at the ask: the id is minted early because the
/// store's request and the item folder it makes both wear it.
#[derive(Debug)]
pub struct BookCopy {
    /// The row id the copy lands as — also the store's own name for the
    /// copy's item folder.
    pub new_id: String,
    /// The book whose bytes the copy wears.
    pub book: Book,
    /// The row the copy is filed beside: the row the reader pointed at, not
    /// the original a link happened to read.
    pub beside: String,
    /// The name the copy wears before the level's counter steps it: the
    /// book's own display name, or the link's name when a link was what was
    /// duplicated.
    pub shown: String,
}

/// What a link points at, for the duplicate it is about to become: the book
/// whose bytes the copy will wear, or the shelf a second pointer stays at. A
/// link whose target is gone, dead or not a book, answers [`LinkAt::Dead`],
/// which is the refusal every caller of this shares.
pub(super) enum LinkAt {
    Book(Book),
    Shelf,
    Dead,
}

pub(super) fn link_at(rows: &[Row], target: &str) -> LinkAt {
    if id::is_shelf(target) {
        return LinkAt::Shelf;
    }
    match find_row(rows, target) {
        Some(Row::Book(book)) if !book.missing => LinkAt::Book(book.clone()),
        _ => LinkAt::Dead,
    }
}

/// One entry's answer, asked of the lists as they stand. ENTRIES because a
/// selection holds shelf ids and row ids alike, and the two namespaces are
/// disjoint by prefix: the row list answering "not mine" is the shelf list's
/// turn.
pub fn plan_one(rows: &[Row], shelves: &[Shelf], entry: &str, now: u64) -> DupPlan {
    let Some(row) = find_row(rows, entry) else {
        return match plan_tree(rows, shelves, entry, now) {
            Some(tree) => DupPlan::Tree(tree),
            None => DupPlan::Skip,
        };
    };
    match row {
        Row::Link { id: row_id, name, target, .. } => match link_at(rows, target) {
            // A shelf's link is the one duplicate that stays a pointer: a
            // level holds no bytes, so there is nothing to store and nothing
            // to separate.
            LinkAt::Shelf => DupPlan::ShelfLink {
                row_id: row_id.clone(),
                name: name.clone(),
                target: target.clone(),
            },
            // A book's link is a doorway onto the target's bytes: the
            // duplicate is the app's own copy of what it opens, filed beside
            // the link itself.
            LinkAt::Book(book) => DupPlan::Book(Box::new(BookCopy {
                new_id: id::next_id(now),
                book,
                beside: row_id.clone(),
                shown: name.clone(),
            })),
            LinkAt::Dead => {
                DupPlan::Dead(format!("“{name}” points at a book that is not there any more."))
            }
        },
        Row::Book(book) => {
            if book.missing {
                return DupPlan::Skip;
            }
            let shown = book.title();
            let beside = book.id.clone();
            DupPlan::Book(Box::new(BookCopy {
                new_id: id::next_id(now),
                book: book.clone(),
                beside,
                shown,
            }))
        }
    }
}
