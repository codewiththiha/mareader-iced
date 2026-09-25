//! Shelf membership: the ids a shelf holds, the level query that answers
//! for the root, and the edits a placement, a filing and a removal make.

use super::{Shelf, ALL_SHELF};

/// Put `id` on a member list at `index`, or move it there when it is already a
/// member; `None` appends. Moving within a list removes first and then inserts,
/// so dropping a book on its own neighbour does not shift the tail.
pub fn place(members: &mut Vec<String>, id: &str, index: Option<usize>) {
    members.retain(|m| m != id);
    let at = index.unwrap_or(members.len()).min(members.len());
    members.insert(at, id.to_string());
}

/// The ids of the rows one level holds, in the order it holds them: the one
/// answer to "what is on this level", read by the collision check, the count,
/// the render and the purge. A shelf answers with its member list; the root
/// answers with the rows no shelf holds.
pub fn members_of<'a>(
    rows: &'a [crate::book::Row],
    shelves: &'a [Shelf],
    shelf_id: &str,
) -> Vec<&'a str> {
    if let Some(shelf) = shelves.iter().find(|s| s.id == shelf_id) {
        return shelf.books.iter().map(String::as_str).collect();
    }
    if shelf_id != ALL_SHELF {
        return Vec::new();
    }
    // One pass over the memberships rather than one per row: this is asked per
    // arrival and per render.
    let filed: std::collections::HashSet<&str> = shelves
        .iter()
        .flat_map(|s| s.books.iter().map(String::as_str))
        .collect();
    rows.iter()
        .map(crate::book::Row::id)
        .filter(|id| !filed.contains(id))
        .collect()
}

/// Put `id` on a shelf unless it is already there. Not [`place`]: a restore and
/// a "show it here as well" may add a book that is already a member, and
/// appending it again would move a book for no reason.
pub fn shelf_add(shelf: &mut Shelf, book_id: &str) {
    if !shelf.books.iter().any(|member| member == book_id) {
        shelf.books.push(book_id.to_string());
    }
}

pub fn forget(members: &mut Vec<String>, id: &str) -> bool {
    let before = members.len();
    members.retain(|m| m != id);
    members.len() != before
}

/// Drop a book from every shelf at once. Membership is per shelf, so the sweep
/// is the only way to be sure no shelf keeps pointing at a row that is gone.
pub fn forget_everywhere(shelves: &mut [Shelf], book_id: &str) {
    for shelf in shelves.iter_mut() {
        forget(&mut shelf.books, book_id);
    }
}

pub fn containing<'a>(shelves: &'a [Shelf], book_id: &str) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.books.iter().any(|m| m == book_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Row};
    use crate::shelf::kit::{ids, plain, shelf};
    use crate::shelf::members::containing;
    use crate::shelf::members::forget;
    use crate::shelf::members::forget_everywhere;
    use crate::shelf::members::members_of;
    use crate::shelf::members::place;
    use crate::shelf::members::shelf_add;

    #[test]
    fn a_level_s_members_are_its_shelf_s_or_the_unfiled_rows() {
        let rows = vec![
            book("b1", "Dune"),
            book("b2", "Apple"),
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
        ];
        let shelves = vec![plain("s", &["b1", "l1"]), plain("t", &["b2"])];
        assert_eq!(members_of(&rows, &shelves, "s"), vec!["b1", "l1"]);
        assert_eq!(members_of(&rows, &shelves, "gone"), Vec::<&str>::new());
        assert_eq!(members_of(&rows, &shelves, ALL_SHELF), Vec::<&str>::new());
        let one_filed = vec![plain("s", &["b1"])];
        assert_eq!(members_of(&rows, &one_filed, ALL_SHELF), vec!["b2", "l1"]);
    }

    #[test]
    fn a_drop_appends_by_default() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        place(&mut m, "c", None);
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_drop_lands_at_the_index_pointed_at() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "d", Some(1));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c"]);
        place(&mut m, "e", Some(99));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c", "e"]);
    }

    #[test]
    fn moving_a_member_does_not_duplicate_or_shift_the_tail() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "c", Some(0));
        assert_eq!(ids(&m), vec!["c", "a", "b"]);
        place(&mut m, "a", Some(1));
        assert_eq!(ids(&m), vec!["c", "a", "b"], "a book stays where it is dropped");
        place(&mut m, "c", Some(3));
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn filing_a_book_that_is_already_filed_moves_nothing() {
        // Appending an existing member would reshuffle a shelf for an
        // instruction that was not about position.
        let mut s = shelf("s1", "One", &["a", "b"]);
        shelf_add(&mut s, "b");
        assert_eq!(ids(&s.books), vec!["a", "b"]);
        shelf_add(&mut s, "c");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
        shelf_add(&mut s, "a");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_book_leaves_one_shelf_or_all_of_them() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        assert!(forget(&mut m, "a"));
        assert!(!forget(&mut m, "a"));
        assert_eq!(ids(&m), vec!["b"]);

        let mut shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        forget_everywhere(&mut shelves, "b");
        assert_eq!(ids(&shelves[0].books), vec!["a"]);
        assert!(shelves[1].books.is_empty());
    }

    #[test]
    fn the_shelves_a_book_is_on_are_found_in_order() {
        let shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        let names: Vec<&str> = containing(&shelves, "b").iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["One", "Two"]);
        assert!(containing(&shelves, "zzz").is_empty());
    }

    fn book(id: &str, title: &str) -> Row {
        Row::Book(Book {
            title: Some(title.to_string()),
            added_ms: 1,
            ..crate::testkit::book(id)
        })
    }
}
