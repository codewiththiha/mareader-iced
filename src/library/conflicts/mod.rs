//! The name question: a level already holds a book of the name that is
//! arriving, and the sheet asks which of three things the reader meant.
//!
//! The rule itself is `library_core::conflict`, pure and host-tested; this is
//! the wiring between it and the things a reader can do about it — the ask
//! every placing surface hands its arrivals through, the small readers the
//! app answers them with, and the answers themselves.

pub mod naming;
pub mod planned;
pub mod words;

pub use naming::{describe_shelf, shelf_offers, ShelfConflictAsk};
pub use planned::screen_planned;
pub use words::describe;

use library_core::book::{find_by_id, find_row, Book, Row};
use library_core::conflict::{collide, next_name, Arrival, Placement};
use library_core::folder::{FolderMode, WatchedFolder};
use library_core::shelf::{self, Shelf};

use super::departure;

/// Which question an ask is, and the facts only that question has: the
/// sheets share one queue and one slot, so the describe and the answer both
/// dispatch on this rather than re-deriving the shape from the arrival.
#[derive(Clone, PartialEq, Debug)]
pub enum AskKind {
    /// WHICH three answers the sheet offers is the arrival's fact rather
    /// than this one's, so this variant carries nothing.
    NameCollision,
    /// A per-file question out of a folder import merging into a shelf the
    /// level already held. The walk's screen raises it when that screen
    /// lands; its words and answers already ride the sheet.
    #[allow(dead_code)] // ported ahead: the walk mints it
    FolderMerge {
        /// A folder that reads in place lands a linked answer now, a
        /// copying one lands it after its copy.
        mode: FolderMode,
        /// The watched folder whose ledger records the placement when the
        /// answer lands, so a later rescan stays quiet.
        folder_id: String,
    },
    /// Two answers rather than three — the library's own stored copy on
    /// this level, or the folder's book lit where it stands — because a
    /// second row of one linked file is the one thing a read-at-place
    /// folder can never make.
    Covered {
        /// The folder whose tree holds the file. Always one: the ask exists
        /// because a specific tree covers the ground.
        folder_id: String,
    },
    /// The question is the library's rather than the level's: the reader
    /// already has this book, somewhere, and what is asked is whether they
    /// meant to add a second instance or to go to the one they have.
    AlreadyHave,
}

impl AskKind {
    /// The folder whose ledger an answered ask settles, when the ask is one
    /// a folder raised.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            AskKind::FolderMerge { folder_id, .. } | AskKind::Covered { folder_id } => {
                Some(folder_id)
            }
            AskKind::NameCollision | AskKind::AlreadyHave => None,
        }
    }

    /// Whether the ask belongs to a folder that reads in place: the answer
    /// that lands a file links it rather than copying it, and the sheet's
    /// twin rule withholds *as new* by it.
    pub fn reads_in_place(&self) -> bool {
        matches!(self, AskKind::FolderMerge { mode, .. } if mode.reads_in_place())
    }

    pub fn is_name_question(&self) -> bool {
        matches!(self, AskKind::NameCollision)
    }

    pub fn is_folder_merge(&self) -> bool {
        matches!(self, AskKind::FolderMerge { .. })
    }

    /// The two-answer sheets: one wording shape, one answer batch, whether
    /// the library noticed the folder's ground or its own held copy.
    pub fn is_two_answer(&self) -> bool {
        matches!(self, AskKind::Covered { .. } | AskKind::AlreadyHave)
    }
}

/// The question on screen: the arrival kept whole, because an answer places
/// it and a placement needs the row and the level it was going to, and the
/// name of the thing already there — read once, because the sheet prints it
/// in three places and a heading and two buttons must not each derive their
/// own.
#[derive(Clone, PartialEq, Debug)]
pub struct ConflictAsk {
    pub arrival: Arrival,
    pub existing_id: String,
    pub existing_name: String,
    pub kind: AskKind,
}

impl ConflictAsk {
    pub fn name_collision(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::NameCollision }
    }

    /// Raised by the walk's own screen when that screen lands; the two
    /// tests below mint it directly today.
    #[allow(dead_code)] // ported ahead: the walk's screen raises it
    pub fn folder_merge(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        mode: FolderMode,
        folder_id: String,
    ) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::FolderMerge { mode, folder_id } }
    }

    pub fn already_have(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::AlreadyHave }
    }

    pub fn covered(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        folder_id: String,
    ) -> Self {
        Self { arrival, existing_id, existing_name, kind: AskKind::Covered { folder_id } }
    }
}

/// Every placing surface hands its placements through here before writing
/// anything, and applies the clean half at once: what collides waits on the
/// sheet, what does not lands now.
pub fn screen(
    rows: &[Row],
    shelves: &[Shelf],
    arrivals: Vec<Arrival>,
) -> (Vec<Arrival>, Vec<ConflictAsk>) {
    let mut clean = Vec::with_capacity(arrivals.len());
    let mut asks = Vec::new();
    for arrival in arrivals {
        match collide(rows, shelves, &arrival) {
            Some(existing_id) => {
                let existing_name = existing_name_of(rows, &existing_id, &arrival);
                asks.push(ConflictAsk::name_collision(arrival, existing_id, existing_name));
            }
            None => clean.push(arrival),
        }
    }
    (clean, asks)
}

/// One spelling, because the sheet prints this name in its heading and in
/// every button's sentence: a site that derived its own would eventually
/// disagree with the others about which book the question is about.
pub fn existing_name_of(rows: &[Row], existing_id: &str, arrival: &Arrival) -> String {
    find_row(rows, existing_id)
        .map(|row| row.display_name())
        .unwrap_or_else(|| arrival.name.clone())
}

/// A drag of four books is four arrivals, and a row that went between the
/// lift and the drop is not one of them. The name is read here rather than
/// by the rule, because the rule is pure and holds no rows.
pub fn moved_arrivals(
    rows: &[Row],
    row_ids: &[String],
    to: &str,
    index: Option<usize>,
    from: Option<&str>,
) -> Vec<Arrival> {
    row_ids
        .iter()
        .filter_map(|row_id| {
            let row = find_row(rows, row_id)?;
            let arrival = Arrival::moved(row_id.clone(), row.display_name(), to, index);
            Some(match from {
                Some(from) => arrival.leaving(from),
                None => arrival,
            })
        })
        .collect()
}

/// The import half of a screen is the import's own landing; nothing a move
/// raises carries a file, so the clean half of a move's screen is ids.
pub fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// The row being dragged is a read-at-place book an in-place folder placed,
/// and the row already on the level is one of the library's own stored
/// copies: neither side is the reader's to destroy, so the sheet offers
/// *make link* in place of *replace*.
pub fn link_shape(rows: &[Row], folders: &[WatchedFolder], ask: &ConflictAsk) -> bool {
    let Some(moved_id) = ask.arrival.moving.as_deref() else {
        return false;
    };
    let existing_is_a_copy = find_row(rows, &ask.existing_id)
        .and_then(|row| row.book())
        .is_some_and(|book| book.origin.is_stored());
    existing_is_a_copy
        && departure::converts_on_move(rows, folders, moved_id, &ask.arrival.shelf_id)
}

/// One function rather than a branch in the sheet and a second in the
/// answer, so the two cannot drift about which buttons a given arrival gets.
/// The kind decides first — the two-answer and the folder-merge sheets carry
/// their own fixed lists — and only the name question derives its offers
/// from the arrival and the shapes on the level.
pub fn offers_for(rows: &[Row], folders: &[WatchedFolder], ask: &ConflictAsk) -> &'static [Placement] {
    match &ask.kind {
        AskKind::Covered { .. } | AskKind::AlreadyHave => Placement::COVERED,
        AskKind::FolderMerge { .. } => Placement::FOLDER_MERGE,
        AskKind::NameCollision => {
            if ask.arrival.is_import() {
                Placement::FILE
            } else if link_shape(rows, folders, ask) {
                Placement::MOVE_KEEPING_BOTH
            } else {
                Placement::MOVE
            }
        }
    }
}

/// Read at the click rather than at the raise, in one place: the two answers
/// that mint a name must mint the same one for the same arrival.
pub fn minted_name(rows: &[Row], shelves: &[Shelf], ask: &ConflictAsk) -> String {
    next_name(rows, shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

/// The slot the displaced row holds on the level the arrival is landing on:
/// a replace seats the arrival where the reader pointed at, not at the end.
pub fn member_slot(shelves: &[Shelf], shelf_id: &str, row_id: &str) -> Option<usize> {
    shelf::find(shelves, shelf_id).and_then(|s| s.books.iter().position(|m| m == row_id))
}

/// The shelves a row is filed on, in shelf order: the memberships a merge's
/// survivor takes over and a replace's arrival inherits.
pub fn memberships(shelves: &[Shelf], book_id: &str) -> Vec<(String, String)> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect()
}

/// Whether the row that survives is the library's own copy OF the row that
/// dissolves. One question in one place, because the two answers that
/// dissolve a row — a merge into the copy and a link at it — both write the
/// folder's moved-out log on this condition and nothing else.
pub fn survivor_is_the_copy_of(rows: &[Row], survivor: &str, gone: &Book) -> bool {
    find_by_id(rows, survivor).is_some_and(|keep| keep.origin.is_store_copy_of(gone.path()))
}

/// File a row onto every shelf named, in one write: the shelves a dissolved
/// row held are the shelves its survivor takes over.
pub fn file_on_all(shelves: &mut [Shelf], row_id: &str, shelves_named: &[String]) {
    for one in shelves.iter_mut() {
        if shelves_named.contains(&one.id) {
            shelf::shelf_add(one, row_id);
        }
    }
}

/// The name a rename gives a row, whichever shape the row is: a book wears
/// it as the reader's own title, a link as the name it was minted with.
pub fn rename_row(rows: &mut [Row], row_id: &str, name: &str) -> bool {
    let Some(row) = library_core::book::find_row_mut(rows, row_id) else {
        return false;
    };
    match row {
        Row::Book(b) => {
            b.title = Some(name.to_string());
            b.title_locked = true;
        }
        Row::Link { name: own, .. } => *own = name.to_string(),
    }
    true
}

/// What the note says happened: nothing the import looked for was new, or
/// the folder walked back inside its own tree. The fold's own sentence is
/// the family's business — its raising waits on the fold landing, and the
/// variant stands where the web's words already are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteKind {
    NothingNew,
    /// A re-picked folder that had left its family folds back onto the rung
    /// its directory names: raised by the walk's own fold, which has not
    /// landed in the native app yet.
    #[allow(dead_code)] // ported ahead: the fold raises it
    Returned,
}

impl NoteKind {
    /// The line under the shelf's name, in the web sheet's own words.
    pub fn sublabel(self) -> &'static str {
        match self {
            NoteKind::NothingNew => "Nothing new to import",
            NoteKind::Returned => "Back where its folder names",
        }
    }
}

/// The note's full sentence, off the web's own sheet: the closing is part
/// of the promise — the shelf lights up as the note comes down.
pub fn note_sentence(kind: NoteKind, name: &str) -> String {
    match kind {
        NoteKind::NothingNew => {
            "The import walked the folder again and found nothing new — every \
             book is already on the shelf. The shelf lights up when you close \
             this."
                .to_string()
        }
        NoteKind::Returned => format!(
            "“{name}” went back inside the folder it belongs to, onto the \
             shelf its directory names. Nothing was copied, and nothing on disk \
             moved. It lights up where it stands now when you close this."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::conflicts::kit::{
    ask, found, linked_row, placing_folder, plain_shelf, stored_row,
};
    use crate::library::conflicts::words::describe;
    use library_core::book::Row;
    use library_core::book::find_by_id;
    use library_core::conflict::Arrival;
    use library_core::conflict::Placement;
    use library_core::folder::FolderMode;
    use library_core::shelf::ALL_SHELF;
    use library_core::testkit;

    #[test]
    fn a_screen_splits_the_clean_from_the_colliding() {
        let rows = vec![
            linked_row("e1", "Dune", "/mine/dune.md", 3),
            linked_row("b2", "Hyperion", "/mine/hyperion.md", 5),
        ];
        let shelves = vec![plain_shelf("s1", &["e1"])];
        let arrivals = vec![
            Arrival::moved("b2", "Hyperion", "s1", None),
            Arrival::moved("b3", "Dune", "s1", Some(2)),
        ];
        let (clean, asks) = screen(&rows, &shelves, arrivals);
        assert_eq!(clean_move_ids(clean), vec!["b2".to_string()], "no name on the level, no question");
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].existing_id, "e1");
        assert_eq!(asks[0].existing_name, "Dune", "read once, off the row that is there");
        assert_eq!(asks[0].arrival.index, Some(2), "the arrival is kept whole for the answer");
    }

    #[test]
    fn arrivals_skip_rows_that_went_between_the_lift_and_the_drop() {
        let rows = vec![linked_row("b1", "Dune", "/mine/dune.md", 3)];
        let arrivals = moved_arrivals(
            &rows,
            &["b1".to_string(), "gone".to_string()],
            "s1",
            Some(1),
            Some("from1"),
        );
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].name, "Dune", "the name is the row's own display name");
        assert_eq!(arrivals[0].from.as_deref(), Some("from1"), "the level the drag left");
        assert_eq!(arrivals[0].index, Some(1));
    }

    #[test]
    fn the_link_shape_offers_link_in_place_of_the_destructive_replace() {
        // The dragged row reads in place under a folder that placed it, and
        // the row already on the level is the library's own copy: neither
        // side is the reader's to destroy.
        let rows = vec![
            stored_row("e1", "Dune", "/books/dune.md", "/store/e1.md", 9),
            linked_row("m1", "Dune", "/books/dune.md", 7),
        ];
        let folders = vec![placing_folder(7)];
        let moving = ask(Some("m1"), "Dune", "s1");
        assert_eq!(
            offers_for(&rows, &folders, &moving),
            Placement::MOVE_KEEPING_BOTH,
            "merge, link, keep both — no replace"
        );

        // An existing row that is not the library's own copy gets the move's
        // three: merge, replace, keep both.
        let plain = ask(Some("m2"), "Dune", "s1");
        assert_eq!(offers_for(&rows, &folders, &plain), Placement::MOVE);
    }

    #[test]
    fn the_survivor_question_reads_the_copy_s_provenance() {
        let rows = vec![
            stored_row("e1", "Dune", "/books/dune.md", "/store/e1.md", 9),
            stored_row("e2", "Other", "/elsewhere/x.md", "/store/e2.md", 11),
            linked_row("m1", "Dune", "/books/dune.md", 7),
        ];
        let gone = find_by_id(&rows, "m1").expect("the dragged row");
        assert!(survivor_is_the_copy_of(&rows, "e1", gone), "the copy OF the gone row's file");
        assert!(!survivor_is_the_copy_of(&rows, "e2", gone), "another content wearing one name");
    }

    #[test]
    fn a_minted_name_is_the_next_free_one_beside_the_collision() {
        let rows = vec![linked_row("e1", "Dune", "/mine/dune.md", 3)];
        let shelves = vec![plain_shelf("s1", &["e1"])];
        let ask = ask(Some("m1"), "Dune", "s1");
        let name = minted_name(&rows, &shelves, &ask);
        assert_ne!(name, "Dune");
        assert!(name.contains("Dune"), "the counter name keeps the name recognisable: {name}");
    }

    #[test]
    fn the_slot_and_the_memberships_read_the_level_as_it_stands() {
        let shelves = vec![plain_shelf("s1", &["x", "e1", "y"]), plain_shelf("s2", &["e1"])];
        assert_eq!(member_slot(&shelves, "s1", "e1"), Some(1));
        assert_eq!(member_slot(&shelves, "s1", "zz"), None);
        assert_eq!(
            memberships(&shelves, "e1"),
            vec![("s1".to_string(), "s1".to_string()), ("s2".to_string(), "s2".to_string())],
            "in shelf order"
        );

        let mut shelves = shelves;
        file_on_all(&mut shelves, "m1", &["s2".to_string(), "gone".to_string()]);
        assert!(shelf::find(&shelves, "s2").unwrap().books.contains(&"m1".to_string()));
        assert!(!shelf::find(&shelves, "s1").unwrap().books.contains(&"m1".to_string()));
    }

    #[test]
    fn a_rename_writes_the_reader_s_own_name_on_both_row_shapes() {
        let mut rows = vec![
            linked_row("b1", "Dune", "/mine/dune.md", 3),
            Row::Link {
                id: "l1".to_string(),
                name: "Old".to_string(),
                target: "b1".to_string(),
                added_ms: 0,
            },
        ];
        assert!(rename_row(&mut rows, "b1", "Dune (2)"));
        let book = find_by_id(&rows, "b1").expect("renamed");
        assert_eq!(book.title.as_deref(), Some("Dune (2)"));
        assert!(book.title_locked, "the minted name is the reader's own: a rescan does not wash it");
        assert!(rename_row(&mut rows, "l1", "Pointer"));
        assert!(!rename_row(&mut rows, "gone", "X"));
    }

    #[test]
    fn the_sheet_words_the_move_s_question_and_counts_the_queue() {
        let rows = vec![
            stored_row("e1", "Dune", "/mine/dune.md", "/store/e1.md", 9),
            linked_row("m1", "Dune", "/elsewhere/dune.md", 7),
        ];
        let shelves = vec![plain_shelf("Reading", &["e1"])];
        let folders = vec![testkit::watched_folder("f0", "/nowhere")];
        let ask = ConflictAsk {
            arrival: Arrival::moved("m1", "Dune", "Reading", None).leaving("Other"),
            existing_id: "e1".to_string(),
            existing_name: "Dune".to_string(),
            kind: AskKind::NameCollision,
        };
        let spec = describe(&rows, &shelves, &folders, &ask, &[ask.clone(), ask.clone()]);
        assert_eq!(spec.heading, "Dune", "the arriving name is the heading");
        assert_eq!(spec.subtitle, "Already on “Reading” · 2 more waiting");
        assert!(spec.question.contains("A book called “Dune” is already on “Reading”"));
        assert!(spec.question.contains("Keep one book, keep this one instead"), "the plain move's three");
        assert_eq!(
            spec.choices.iter().map(|c| c.placement).collect::<Vec<_>>(),
            Placement::MOVE,
            "the rows the sheet renders are the answers the apply takes"
        );
        let replace = spec.choices.iter().find(|c| c.placement == Placement::Replace).unwrap();
        assert!(replace.note.contains("leaves the library"), "the destructive answer says so");
        assert!(replace.note.contains("every shelf it was on"));

        // The root names the library itself, and a shelf that went while the
        // sheet was up falls back to the quiet spelling.
        let root_ask = ConflictAsk {
            arrival: Arrival::moved("m1", "Dune", ALL_SHELF, None),
            existing_id: "e1".to_string(),
            existing_name: "Dune".to_string(),
            kind: AskKind::NameCollision,
        };
        let spec = describe(&rows, &shelves, &folders, &root_ask, &[]);
        assert_eq!(spec.subtitle, "Already in your library");
        let gone_ask = ConflictAsk {
            arrival: Arrival::moved("m1", "Dune", "gone-shelf", None),
            existing_id: "e1".to_string(),
            existing_name: "Dune".to_string(),
            kind: AskKind::NameCollision,
        };
        let spec = describe(&rows, &shelves, &folders, &gone_ask, &[]);
        assert_eq!(spec.subtitle, "Already on this shelf");
    }

    #[test]
    fn the_kind_carries_the_question_s_facts() {
        let covered =
            import_ask(AskKind::Covered { folder_id: "f1".to_string() }, "/in/tree/Dune.md", "s1", 3);
        assert_eq!(covered.kind.folder_id(), Some("f1"));
        assert!(covered.kind.is_two_answer());
        assert!(!covered.kind.is_folder_merge());
        assert!(!covered.kind.is_name_question());

        let have = import_ask(AskKind::AlreadyHave, "/elsewhere/Dune.md", ALL_SHELF, 4);
        assert_eq!(have.kind.folder_id(), None, "no folder raised it");
        assert!(have.kind.is_two_answer());

        let in_place = import_ask(
            AskKind::FolderMerge { mode: FolderMode::LinkInPlaceWatched, folder_id: "f2".to_string() },
            "/tree/Dune.md",
            "s1",
            5,
        );
        assert!(in_place.kind.reads_in_place());
        assert!(in_place.kind.is_folder_merge());
        assert_eq!(in_place.kind.folder_id(), Some("f2"));

        let copying = import_ask(
            AskKind::FolderMerge { mode: FolderMode::Copy, folder_id: "f2".to_string() },
            "/tree/Dune.md",
            "s1",
            5,
        );
        assert!(!copying.kind.reads_in_place(), "a copying folder lands after its copy");
    }

    #[test]
    fn the_import_kinds_offer_their_own_fixed_lists() {
        let rows = vec![linked_row("e1", "Dune", "/mine/Dune.md", 3)];
        let folders = vec![testkit::watched_folder("f1", "/in/tree")];
        let covered =
            import_ask(AskKind::Covered { folder_id: "f1".to_string() }, "/in/tree/Dune.md", "s1", 3);
        assert_eq!(offers_for(&rows, &folders, &covered), Placement::COVERED);
        let have = import_ask(AskKind::AlreadyHave, "/elsewhere/Dune.md", ALL_SHELF, 4);
        assert_eq!(offers_for(&rows, &folders, &have), Placement::COVERED);
        let merge = import_ask(
            AskKind::FolderMerge { mode: FolderMode::Copy, folder_id: "f1".to_string() },
            "/in/tree/Dune.md",
            "s1",
            3,
        );
        assert_eq!(offers_for(&rows, &folders, &merge), Placement::FOLDER_MERGE);
        let loose = import_ask(AskKind::NameCollision, "/loose/Dune.md", "s1", 6);
        assert_eq!(offers_for(&rows, &folders, &loose), Placement::FILE);
    }

    #[test]
    fn the_covered_sheet_words_the_folder_s_ground() {
        let rows = vec![linked_row("e1", "Dune", "/in/tree/Dune.md", 3)];
        let shelves = vec![plain_shelf("Reading", &["e1"])];
        let folders = vec![testkit::watched_folder("f1", "/in/tree")];
        let covered =
            import_ask(AskKind::Covered { folder_id: "f1".to_string() }, "/in/tree/Dune.md", "Reading", 3);
        let spec = describe(&rows, &shelves, &folders, &covered, std::slice::from_ref(&covered));
        assert_eq!(spec.heading, "Dune", "the arriving file's stem is the heading");
        assert_eq!(spec.subtitle, "Inside “tree” · 1 more waiting");
        assert!(spec.question.contains("which the library reads in place"));
        assert!(spec.question.contains("Import your own copy on “Reading”"));
        assert!(spec.apply_all);
        assert_eq!(spec.waiting, 1);
        assert_eq!(
            spec.choices.iter().map(|c| c.placement).collect::<Vec<_>>(),
            vec![Placement::KeepBoth, Placement::Open]
        );
        assert_eq!(spec.choices[0].label, "Import a copy here");
        assert_eq!(spec.choices[1].label, "Show the imported one");
        assert!(spec.choices[1].note.contains("light it up"));

        // A name question in the queue does not count toward this sheet's
        // batch: the switch would promise an answer it cannot carry.
        let mixed = describe(
            &rows,
            &shelves,
            &folders,
            &covered,
            &[covered.clone(), ask(Some("m1"), "Dune", "Reading")],
        );
        assert_eq!(mixed.waiting, 1);
    }

    #[test]
    fn the_already_have_sheet_words_the_library_s_copy() {
        let rows = vec![stored_row("e1", "Dune", "/mine/Dune.md", "/store/e1.md", 3)];
        let shelves = vec![plain_shelf("Reading", &["e1"])];
        let folders = vec![testkit::watched_folder("f1", "/in/tree")];
        let ask = import_ask(AskKind::AlreadyHave, "/elsewhere/Dune.md", ALL_SHELF, 4);
        let spec = describe(&rows, &shelves, &folders, &ask, &[]);
        assert_eq!(spec.subtitle, "Already in your library");
        assert!(spec.question.contains("The library already holds this book as “Dune”"));
        assert!(spec.question.contains("Import your own copy in your library"));
        assert_eq!(spec.choices[1].note, "Add nothing — go to “Dune” and light it up");
        assert!(spec.apply_all);
        assert_eq!(spec.waiting, 0);
    }

    #[test]
    fn the_folder_merge_sheet_withholds_as_new_from_a_twin() {
        let rows = vec![
            linked_row("e1", "Dune", "/tree/Dune.md", 3),
            stored_row("e2", "Dune", "/mine/Dune.md", "/store/e2.md", 5),
        ];
        let shelves = vec![plain_shelf("s1", &["e1", "e2"])];
        let folders = vec![testkit::watched_folder("f1", "/tree")];
        // The arriving file IS the linked row's file under a read-at-place
        // folder: two rows of one file is the one thing the library cannot
        // make, so the sheet offers two answers.
        let twin = import_ask(
            AskKind::FolderMerge { mode: FolderMode::LinkInPlaceWatched, folder_id: "f1".to_string() },
            "/tree/Dune.md",
            "s1",
            3,
        );
        let spec = describe(&rows, &shelves, &folders, &twin, &[]);
        assert_eq!(spec.subtitle, "Into “Dune”");
        assert!(spec.question.contains("reads this very file"));
        assert_eq!(spec.choices.len(), 2, "the twin gets no *as new*");
        assert_eq!(spec.choices[0].label, "Merge");
        assert_eq!(spec.choices[1].label, "Replace");
        assert!(spec.choices[1].note.contains("takes its slot"));
        assert!(spec.apply_all);

        // A stored folder's namesake keeps all three: its copy is not the
        // arriving file, so a second row is a second book.
        let namesake = import_ask(
            AskKind::FolderMerge { mode: FolderMode::Copy, folder_id: "f1".to_string() },
            "/elsewhere/Dune.md",
            "s1",
            9,
        );
        let spec = describe(&rows, &shelves, &folders, &namesake, std::slice::from_ref(&namesake));
        assert_eq!(spec.subtitle, "Into “Dune” · 1 more waiting");
        assert_eq!(spec.choices.len(), 3);
        let new = &spec.choices[2];
        assert_eq!(new.label, "As new");
        assert_eq!(new.note, "Keep both — this file becomes “Dune_1”");
        assert_eq!(new.placement, Placement::KeepBoth);
    }

    #[test]
    fn the_note_words_name_the_close_s_promise() {
        assert_eq!(NoteKind::NothingNew.sublabel(), "Nothing new to import");
        assert_eq!(NoteKind::Returned.sublabel(), "Back where its folder names");
        let quiet = note_sentence(NoteKind::NothingNew, "Books");
        assert!(quiet.contains("found nothing new"), "the nothing-new sentence: {quiet}");
        assert!(quiet.contains("lights up when you close this"), "the close is part of the promise");
        let back = note_sentence(NoteKind::Returned, "Books");
        assert!(back.contains("“Books” went back inside"), "the returned sentence: {back}");
        assert!(back.contains("lights up where it stands now"), "the light stands where it did");
    }

    fn import_ask(kind: AskKind, path: &str, shelf: &str, n: u32) -> ConflictAsk {
        ConflictAsk {
            arrival: Arrival::import(found(path, n), shelf, None),
            existing_id: "e1".to_string(),
            existing_name: "Dune".to_string(),
            kind,
        }
    }
}

#[cfg(test)]
mod kit;
