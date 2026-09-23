//! The moves a reader makes by hand: a filing, a lift out to the library's
//! own floor, a nesting, a sibling reorder.
//!
//! One rule covers all of them — **a move never touches a file the reader
//! owns.** A shelf holds book ids, so a move edits a list of ids, and the
//! OS file a read-in-place book points at is never renamed, moved or
//! deleted from here.
//!
//! The gates the web app wraps around these edits — the departure's copy
//! question, the conflict sheet's name screen, the moved-out log a return
//! binds — arrive with the systems that own them; until then a move is the
//! membership edit alone, which is also all a reorder ever was. The drag's
//! own moves (the bulk lift-and-place, the unfile, the sibling seam) land
//! with the drag session that commits them.

use library_core::shelf::{self, Shelf};

/// Membership only, so the same rule covers a bulk filing and a drag's
/// second answer: nothing here touches a filesystem, and a book already on
/// the shelf is not moved to the end for being named twice.
pub fn file_many(shelves: &mut [Shelf], book_ids: &[String], shelf_id: &str) -> bool {
    if book_ids.is_empty() {
        return false;
    }
    let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
        return false;
    };
    let before = shelf.books.len();
    for book_id in book_ids {
        shelf::shelf_add(shelf, book_id);
    }
    shelf.books.len() != before
}

/// Books as memberships, folders as nestings, one persist for the batch.
/// No name question is asked: a nesting writes no membership, so nothing
/// arrives on the parent's level. A folder that cannot be nested there (it
/// would end up inside itself) is left where it is rather than failing the
/// batch — the cycle check is `library_core::shelf::reparent`'s, not the
/// caller's, because a folder filed inside itself renders on no level and
/// can never be opened again.
pub fn nest_many(shelves: &mut [Shelf], folder_ids: &[String], parent: &str) -> bool {
    let mut moved = false;
    for folder_id in folder_ids {
        moved |= shelf::reparent(shelves, folder_id, Some(parent));
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn own(id: &str, name: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
        let mut shelf =
            Shelf::virtual_shelf(id.to_string(), name.to_string(), parent.map(str::to_string));
        shelf.books = owned(books);
        shelf
    }

    #[test]
    fn a_filing_names_a_book_already_a_member_without_moving_it() {
        let mut shelves = vec![own("to", "To", None, &["a", "b"])];
        assert!(!file_many(&mut shelves, &owned(&["a"]), "to"));
        assert_eq!(shelves[0].books, owned(&["a", "b"]));
        assert!(file_many(&mut shelves, &owned(&["c"]), "to"));
        assert_eq!(shelves[0].books, owned(&["a", "b", "c"]));
    }

    #[test]
    fn a_filing_answers_false_for_a_shelf_that_is_not_there() {
        let mut shelves = vec![own("to", "To", None, &[])];
        assert!(!file_many(&mut shelves, &owned(&["a"]), "gone"));
        assert!(shelves[0].books.is_empty());
    }

    #[test]
    fn a_folder_cannot_nest_inside_itself() {
        let mut shelves =
            vec![own("outer", "Outer", None, &[]), own("inner", "Inner", Some("outer"), &[])];
        assert!(
            !nest_many(&mut shelves, &owned(&["outer"]), "inner"),
            "the cycle check is reparent's, and it refuses"
        );
        assert_eq!(shelves[0].parent, None, "the tree stands as it was");
    }

    #[test]
    fn a_nesting_moves_the_folder_and_reports_the_move() {
        let mut shelves =
            vec![own("a", "A", None, &[]), own("b", "B", None, &[]), own("home", "Home", None, &[])];
        assert!(nest_many(&mut shelves, &owned(&["a", "b"]), "home"));
        assert_eq!(shelves[0].parent.as_deref(), Some("home"));
        assert_eq!(shelves[1].parent.as_deref(), Some("home"));
    }
}
