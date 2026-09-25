//! The names a level will and will not hold: what collides, what is free, and the next free one.
use std::collections::HashSet;

use crate::book::{duplicate_title, Row};
use crate::shelf::Shelf;
use super::arrival::Arrival;

/// Whether two names are the same name. Case-insensitive and nothing else:
/// `Report` beside `report` is one name however the filesystem spells them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The row already on the target level whose name this arrival carries, if
/// any.
///
/// Only book rows on the target level are compared: a link never collides, and
/// the collision is the level's.
pub fn collide(rows: &[Row], shelves: &[Shelf], at: &Arrival) -> Option<String> {
    let index = crate::book::index_by_id(rows);
    crate::shelf::members_of(rows, shelves, &at.shelf_id)
        .into_iter()
        .find_map(|member| {
            let row = *index.get(member)?;
            if row.is_link() || Some(row.id()) == at.moving.as_deref() {
                return None;
            }
            same_name(&row.display_name(), &at.name).then(|| row.id().to_string())
        })
}

/// The next free name on one level, or `name` itself when the level does not
/// hold it. Only a colliding level gets the file-manager counter
/// (`1` → `1_1` → `1_2`), counted against that level's names rather than the
/// whole library.
pub fn next_name(rows: &[Row], shelves: &[Shelf], shelf_id: &str, name: &str) -> String {
    let index = crate::book::index_by_id(rows);
    let in_use: Vec<String> = crate::shelf::members_of(rows, shelves, shelf_id)
        .iter()
        .filter_map(|member| index.get(*member).copied())
        .map(Row::display_name)
        .collect();
    free_name(name, &in_use)
}

/// The shelf already at one level whose name an arriving folder carries, if
/// any: the shelf half of [`collide`], asked before a folder import mints its
/// root shelf.
pub fn collide_shelf(shelves: &[Shelf], parent: Option<&str>, name: &str) -> Option<String> {
    crate::shelf::children_of(shelves, parent)
        .into_iter()
        .find(|shelf| same_name(&shelf.name, name))
        .map(|shelf| shelf.id.clone())
}

/// The next free shelf name on one level (`Books` → `Books_1` → `Books_2`),
/// counted against the shelf names that level holds rather than its rows.
pub fn next_shelf_name(shelves: &[Shelf], parent: Option<&str>, name: &str) -> String {
    let in_use: Vec<String> = crate::shelf::children_of(shelves, parent)
        .into_iter()
        .map(|shelf| shelf.name.clone())
        .collect();
    free_name(name, &in_use)
}

/// One spelling of the file manager's copy-into-directory rule for both the
/// book and the shelf side; they differ only in whose names the level holds.
fn free_name(name: &str, in_use: &[String]) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && !in_use.iter().any(|held| same_name(held, trimmed)) {
        return trimmed.to_string();
    }
    let pool: HashSet<String> = in_use.iter().cloned().collect();
    duplicate_title(name, &pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::arrival::Arrival;
    use crate::shelf::ALL_SHELF;
    use crate::conflict::kit::{drag, file, import, plain_shelf, shelf, titled};

    #[test]
    fn a_name_already_on_the_shelf_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")).as_deref(),
            Some("b1")
        );
        let named = vec![titled("b1", "/books/report.pdf", "Report")];
        assert_eq!(
            collide(&named, &shelves, &import("report", "s")).as_deref(),
            Some("b1"),
            "a shelf is read by a person, not by a byte comparison"
        );
    }

    #[test]
    fn a_row_never_collides_with_itself() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &[])];
        assert_eq!(collide(&rows, &shelves, &drag("b1", "1", "s")), None);
        let mut reorder = drag("b1", "1", "s");
        reorder.index = Some(0);
        assert_eq!(collide(&rows, &shelves, &reorder), None);
    }

    #[test]
    fn the_root_is_a_level_and_its_list_is_the_unfiled_rows() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/books/2.pdf", "2"),
        ];
        let shelves = vec![shelf("s", &["b2"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", ALL_SHELF)).as_deref(),
            Some("b1")
        );
        assert_eq!(collide(&rows, &shelves, &import("2", ALL_SHELF)), None);
    }

    #[test]
    fn the_counter_counts_against_the_level_it_lands_on() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
            titled("b3", "/elsewhere/1.pdf", "1"),
        ];
        let shelves = vec![shelf("s", &["b1", "b2"]), shelf("t", &["b3"])];
        // `1` and `1_1` are taken on s, so the next free counter is `1_2`.
        assert_eq!(next_name(&rows, &shelves, "s", "1"), "1_2");
        assert_eq!(next_name(&rows, &shelves, "t", "1"), "1_1");
        // An empty level holds no name: a free name lands as itself, not as a
        // counter of a collision that never happened.
        assert_eq!(next_name(&rows, &shelves, "empty", "1"), "1");
        assert_eq!(next_name(&rows, &shelves, "t", " 1 "), "1_1");
        assert_eq!(next_name(&rows, &shelves, "s", "1_1"), "1_2");
    }

    #[test]
    fn a_move_names_the_level_it_leaves_and_nothing_else_does() {
        // A merge inherits the shelves the moved row keeps; the level it
        // lifts off is the one membership it does not keep.
        let leaving = Arrival::moved("b1", "Dune", "s", None).leaving("t");
        assert_eq!(leaving.from.as_deref(), Some("t"));
        assert_eq!(
            Arrival::moved("b1", "Dune", "s", None).from,
            None,
            "a filing names no departure: the row stays where it was"
        );
        assert_eq!(
            Arrival::import(file("1.pdf"), "s", None).from,
            None,
            "and an import leaves no level at all"
        );
        let rows = vec![titled("b1", "/books/1.pdf", "1"), titled("b2", "/books/2.pdf", "2")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &["b2"])];
        assert_eq!(
            collide(&rows, &shelves, &drag("b2", "1", "s").leaving("t")).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn a_shelf_name_the_level_already_holds_asks() {
        let shelves = vec![plain_shelf("s1", "Books")];
        assert_eq!(collide_shelf(&shelves, None, "Books").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, None, "books").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, None, "Comics"), None);
        assert_eq!(collide_shelf(&shelves, None, "Books_1"), None);
    }

    #[test]
    fn a_shelf_collision_is_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Fiction"),
            Shelf { parent: Some("s1".to_string()), ..plain_shelf("s2", "Deep") },
        ];
        assert_eq!(collide_shelf(&shelves, None, "Fiction").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, Some("s1"), "Fiction"), None);
        assert_eq!(collide_shelf(&shelves, Some("s1"), "Deep").as_deref(), Some("s2"));
    }

    #[test]
    fn the_shelf_counter_counts_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Books"),
            plain_shelf("s2", "Books_1"),
            Shelf { parent: Some("s1".to_string()), ..plain_shelf("s3", "Books") },
        ];
        assert_eq!(next_shelf_name(&shelves, None, "Books"), "Books_2");
        assert_eq!(next_shelf_name(&shelves, Some("s1"), "Books"), "Books_1");
        assert_eq!(next_shelf_name(&shelves, Some("s2"), "Books"), "Books");
    }
}
