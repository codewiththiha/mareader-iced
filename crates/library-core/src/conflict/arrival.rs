//! What is arriving at a level: the name, the format and whether a copy comes with it.
use crate::book::stem_of;
use crate::scan::FoundFile;

/// What is arriving, and where it is going. One value for both routes in, so
/// an import and a drag cannot disagree about what a collision is.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the shelf shows it: a file's stem for an
    /// import, the moved row's own name for a drag.
    pub name: String,
    pub moving: Option<String>,
    /// The file being imported: the address and measurement the row is minted
    /// from. `None` for a move.
    pub file: Option<FoundFile>,
    pub shelf_id: String,
    /// The level the arrival leaves, when it leaves one: a drag's source
    /// shelf, or the shelf a lift-out takes a book off. `None` for an import,
    /// a filing, or a drag that began at the root.
    pub from: Option<String>,
    /// The slot the drop pointed at; `None` appends. Carried rather than
    /// re-derived, because the level it was aimed at may have moved.
    pub index: Option<usize>,
}

impl Arrival {
    pub fn import(file: FoundFile, shelf_id: impl Into<String>, index: Option<usize>) -> Self {
        let name = stem_of(&file.path);
        Self {
            name,
            moving: None,
            file: Some(file),
            shelf_id: shelf_id.into(),
            from: None,
            index,
        }
    }

    /// A row being moved or filed onto a level. `name` is the row's own
    /// [`Row::display_name`]; the caller reads it because the caller holds the
    /// row list.
    pub fn moved(
        row_id: impl Into<String>,
        name: impl Into<String>,
        shelf_id: impl Into<String>,
        index: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            moving: Some(row_id.into()),
            file: None,
            shelf_id: shelf_id.into(),
            from: None,
            index,
        }
    }

    /// Name the level this arrival leaves, so an answer that dissolves the
    /// moving row knows the survivor does not inherit that membership.
    pub fn leaving(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// A folder arriving under a name its level already holds: nothing
    /// measured, nothing moving. The answers are about a whole import run
    /// rather than one placement.
    pub fn folder(name: impl Into<String>, shelf_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            moving: None,
            file: None,
            shelf_id: shelf_id.into(),
            from: None,
            index: None,
        }
    }

    pub fn is_import(&self) -> bool {
        self.file.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflict::placement::Placement;
    use crate::shelf::ALL_SHELF;
    use crate::conflict::kit::{book, file, link};

    #[test]
    fn every_answer_a_sheet_can_offer_is_one_of_five() {
        let all = Placement::ALL;
        for offer in [
            Placement::Open,
            Placement::KeepBoth,
            Placement::Merge,
            Placement::Replace,
            Placement::LinkOnly,
        ] {
            assert!(all.contains(&offer));
        }
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn every_button_has_one_sentence_whichever_sheet_wears_it() {
        let labels: Vec<&str> = Placement::ALL.iter().map(|p| p.label()).collect();
        let mut deduped = labels.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len(), "no two answers share a label");
        assert!(labels.iter().all(|l| !l.is_empty()));
    }

    #[test]
    fn an_arrival_carries_its_own_name_and_its_file() {
        let at = Arrival::import(file("1.pdf"), "s", Some(2));
        assert_eq!(at.name, "1", "the name is the stem: a title is not a file name");
        assert!(at.is_import());
        assert_eq!(at.moving, None);
        assert_eq!(at.index, Some(2));
        assert_eq!(at.file.as_ref().map(|f| f.path.as_str()), Some("/books/1.pdf"));
        let moved = Arrival::moved("b1", "Dune", ALL_SHELF, None);
        assert!(!moved.is_import());
        assert_eq!(moved.moving.as_deref(), Some("b1"));
        assert_eq!(moved.name, "Dune");
    }

    #[test]
    fn a_link_row_is_not_a_book_and_says_so() {
        let rows = [book("b1", "/books/1.pdf"), link("l1", "1", "b1")];
        assert!(rows[0].book().is_some() && !rows[0].is_link());
        assert!(rows[1].is_link() && rows[1].book().is_none());
        assert_eq!(rows[1].book(), None);
        assert_eq!(rows[1].fp(), None, "a pointer has no content identity");
        assert_eq!(rows[1].target(), Some("b1"));
        assert_eq!(rows[0].target(), None);
        assert_eq!(rows[1].display_name(), "1");
        assert_eq!(rows[1].id(), "l1");
        assert_eq!(rows[0].id(), "b1");
    }
}
