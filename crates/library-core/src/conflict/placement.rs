//! Where an arrival can go: the placements a level offers, in the order the sheet shows them.

/// What the reader decided to do about a thing the library already holds.
/// Every collision sheet offers some subset of these five.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Place nothing: take the reader to the thing that is already there.
    Open,
    /// Land it beside what is there, under the next free name.
    KeepBoth,
    /// Fold the arrival into the thing that is there: the further read point
    /// wins, the arrival's shelves and marks join the survivor, and the
    /// arrival goes ([`crate::book::fold_books`]).
    Merge,
    /// The thing that is there goes and the arrival takes its place.
    Replace,
    /// Put a pointer at the thing that is there instead of a second instance.
    LinkOnly,
}

impl Placement {
    /// The button's wording — one spelling per answer, so five sheets cannot
    /// drift.
    pub fn label(self) -> &'static str {
        match self {
            Placement::Open => "Already imported",
            Placement::KeepBoth => "Add as new",
            Placement::Merge => "Merge",
            Placement::Replace => "Replace",
            Placement::LinkOnly => "Make link",
        }
    }

    /// Whether the answer destroys anything; the reason *replace* is the only
    /// one of the five that reads as a warning.
    pub fn is_destructive(self) -> bool {
        matches!(self, Placement::Replace)
    }

    /// Offered for an import of a file: no row to fold and none to displace.
    pub const FILE: &'static [Placement] =
        &[Placement::Open, Placement::KeepBoth, Placement::LinkOnly];

    /// Offered when a row is moved onto a row: the reader is holding the
    /// arrival.
    pub const MOVE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// [`Placement::MOVE`] with *link* standing in for the destructive
    /// *replace*.
    pub const MOVE_KEEPING_BOTH: &'static [Placement] =
        &[Placement::Merge, Placement::LinkOnly, Placement::KeepBoth];

    /// Offered for a covered file: the library's own copy on this level, or
    /// the book the folder already holds.
    pub const COVERED: &'static [Placement] = &[Placement::Open, Placement::KeepBoth];

    /// Offered per file of a merging folder on the compact sheet.
    pub const FOLDER_MERGE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// Offered for a stored folder arrival — copies the library owns, so the
    /// question is the level's own.
    pub const SHELF_STORED: &'static [Placement] =
        &[Placement::Open, Placement::Replace, Placement::KeepBoth];

    /// Offered for a read-at-place folder arrival under a different folder's
    /// name.
    pub const SHELF_READ_IN_PLACE: &'static [Placement] =
        &[Placement::LinkOnly, Placement::Merge];

    pub const ALL: &'static [Placement] = &[
        Placement::Open,
        Placement::KeepBoth,
        Placement::Merge,
        Placement::Replace,
        Placement::LinkOnly,
    ];
}

/// Which thing the reader's answer is about: the row already there, or the
/// shelf already there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Book { row_id: String },
    Shelf { shelf_id: String },
}

impl Scope {
    pub fn id(&self) -> &str {
        match self {
            Scope::Book { row_id } => row_id,
            Scope::Shelf { shelf_id } => shelf_id,
        }
    }

    pub fn is_shelf(&self) -> bool {
        matches!(self, Scope::Shelf { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_replace_destroys_the_thing_that_is_already_there() {
        for offer in Placement::ALL {
            assert_eq!(offer.is_destructive(), *offer == Placement::Replace);
        }
    }
}
