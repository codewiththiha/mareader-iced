//! The question itself: the arrival, the level it is asked at, and the placements it can take.
use super::arrival::Arrival;
use super::placement::{Placement, Scope};

/// One question about an arrival that met something the library already
/// holds: the arrival, the thing it met, that thing's name, and the subset of
/// [`Placement`] offered. A sheet renders `offers` and does not need to know
/// why.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementAsk {
    /// Kept whole: an answer places it, and a placement needs the file or row
    /// and the level it was going to.
    pub arrival: Arrival,
    /// The thing already there: *open* reveals it, *merge* folds into it,
    /// *replace* purges it, *link* points at it.
    pub existing: Scope,
    /// Derived once: a sheet prints it in a heading and on buttons, and three
    /// derivations of one string can disagree.
    pub existing_name: String,
    pub offers: &'static [Placement],
}

impl PlacementAsk {
    pub fn book(
        arrival: Arrival,
        row_id: String,
        existing_name: String,
        offers: &'static [Placement],
    ) -> Self {
        Self {
            arrival,
            existing: Scope::Book { row_id },
            existing_name,
            offers,
        }
    }

    pub fn shelf(
        arrival: Arrival,
        shelf_id: String,
        existing_name: String,
        offers: &'static [Placement],
    ) -> Self {
        Self {
            arrival,
            existing: Scope::Shelf { shelf_id },
            existing_name,
            offers,
        }
    }

    /// A button the ask did not offer would be an answer with nothing to
    /// apply.
    pub fn offers_placement(&self, choice: Placement) -> bool {
        self.offers.contains(&choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Row;
    use crate::conflict::names::collide;
    use crate::conflict::placement::Placement;
    use crate::conflict::kit::{drag, import, link, shelf, titled};

    #[test]
    fn each_ask_offers_only_the_answers_it_can_apply() {
        assert!(!Placement::FILE.contains(&Placement::Merge));
        assert!(!Placement::FILE.contains(&Placement::Replace));
        assert!(Placement::FILE.contains(&Placement::Open));
        // A move has no "go and look at it": the reader holds the arrival.
        assert!(!Placement::MOVE.contains(&Placement::Open));
        assert!(Placement::MOVE.contains(&Placement::Replace));
        // The keeping-both shape swaps the destructive answer for a pointer.
        assert!(!Placement::MOVE_KEEPING_BOTH.contains(&Placement::Replace));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::LinkOnly));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::Merge));
        // A covered file is two answers: a second linked row of one
        // read-at-place file is the one thing that rule can never make.
        assert_eq!(Placement::COVERED, &[Placement::Open, Placement::KeepBoth]);
        // A folder's offers depend on its mode: stored gets the level's own
        // three; read-at-place under another folder's name gets pointer and
        // merge.
        assert_eq!(
            Placement::SHELF_STORED,
            &[Placement::Open, Placement::Replace, Placement::KeepBoth]
        );
        assert_eq!(
            Placement::SHELF_READ_IN_PLACE,
            &[Placement::LinkOnly, Placement::Merge]
        );
        assert!(
            !Placement::SHELF_READ_IN_PLACE.contains(&Placement::KeepBoth),
            "a second instance of one ground is the answer the gate withholds"
        );
        for list in [
            Placement::FILE,
            Placement::MOVE,
            Placement::MOVE_KEEPING_BOTH,
            Placement::COVERED,
            Placement::FOLDER_MERGE,
            Placement::SHELF_STORED,
            Placement::SHELF_READ_IN_PLACE,
        ] {
            assert!(!list.is_empty());
            assert!(list.iter().all(|p| Placement::ALL.contains(p)));
        }
    }

    #[test]
    fn an_ask_answers_for_the_thing_it_met_and_refuses_an_answer_it_did_not_offer() {
        let book =
            PlacementAsk::book(import("dune", "s1"), "b1".into(), "Dune".into(), Placement::FILE);
        assert_eq!(book.existing.id(), "b1");
        assert!(!book.existing.is_shelf());
        assert!(book.offers_placement(Placement::KeepBoth));
        assert!(
            !book.offers_placement(Placement::Merge),
            "an import cannot fold a row it does not have"
        );

        let shelf = PlacementAsk::shelf(
            import("dune", "s1"),
            "s2".into(),
            "Sci-fi".into(),
            Placement::ALL,
        );
        assert_eq!(shelf.existing.id(), "s2");
        assert!(shelf.existing.is_shelf());
        assert!(shelf.offers_placement(Placement::Replace));
    }

    #[test]
    fn an_empty_shelf_never_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &[]), shelf("t", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1", "s")), None);
        let empty: Vec<Row> = Vec::new();
        assert_eq!(collide(&empty, &shelves, &import("1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &import("1", "gone")), None);
    }

    #[test]
    fn a_counter_copy_beside_its_original_never_asks() {
        // A minted name is not a reason to ask again: `1_1` beside `1` is a
        // second book.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
        ];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &drag("b2", "1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &import("1", "s")).as_deref(), Some("b1"));
    }

    #[test]
    fn a_link_neither_asks_nor_blocks() {
        // A link carries its target's name but is never what a collision is
        // found against.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            link("l1", "1", "b1"),
        ];
        let shelves = vec![shelf("s", &["l1"]), shelf("t", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")),
            None,
            "the only row on this shelf is a pointer"
        );
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "t")).as_deref(), Some("b1"));
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "s")), None);
    }

    #[test]
    fn a_twin_on_another_shelf_is_not_this_shelfs_question() {
        // The collision is a name on a level: a `1` on Fiction says nothing
        // about a `1` arriving on Sci-Fi.
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("fiction", &["b1"]), shelf("scifi", &[])];
        assert_eq!(collide(&rows, &shelves, &import("1", "scifi")), None);
    }
}
