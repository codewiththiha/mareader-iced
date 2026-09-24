//! The shelf's own name collision: a shelf called what a folder is called,
//! or one arriving where a shelf of the name already sits.

use library_core::book::Row;
use library_core::conflict::{next_shelf_name, Placement};
use library_core::folder::{FolderOpts, WatchedFolder};
use library_core::ledger;
use library_core::shelf::{self, Shelf};
use super::words::{ChoiceSpec, SheetSpec};

/// A separate ask rather than a variant of [`ConflictAsk`], because its
/// answers are about a whole import run rather than about one placement:
/// the ones that import start the run again with a plan. Raised before the
/// walk, because the answer decides what the walk is for.
#[derive(Clone, PartialEq, Debug)]
pub struct ShelfConflictAsk {
    /// The last segment of the arriving folder's path, the name the reader
    /// picked it by.
    pub incoming_name: String,
    pub existing_id: String,
    pub existing_name: String,
    /// The picked ground: every answer that imports starts its walk on it.
    pub root: String,
    /// The sheet's own answers, so the run the answer starts walks them.
    pub opts: FolderOpts,
    /// Whether the shelf that holds the name is the arriving folder's OWN —
    /// the one its previous run minted — because a re-import of one folder
    /// is a continuation rather than an arrival, and the sheet words it as
    /// one.
    pub own: bool,
}


/// The arrival's MODE decides. A read-at-place arrival gets the pointer and
/// the merge, its *keep both* withheld as the second instance of one ground
/// the family gate exists to prevent, and *replace* with it — neither side
/// a read-at-place collision is the level's to empty.
pub fn shelf_offers(ask: &ShelfConflictAsk) -> &'static [Placement] {
    if ask.opts.mode().reads_in_place() {
        Placement::SHELF_READ_IN_PLACE
    } else {
        Placement::SHELF_STORED
    }
}


/// A pointer row's promise, spelled once because the two sheets naming a
/// link's cost must agree. A link at a folder is not a copy: it lights the
/// folder where it is.
const LINK_FOLDER_NOTE: &str = "A pointer row, not a second shelf: nothing is imported, and tapping it lights the folder where it is";


/// The folder question's own words: the arrival's mode picks the offers,
/// the shelf it collides with picks the counts, and which folder last
/// walked the ground decides whether the question is a continuation.
pub fn describe_shelf(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    ask: &ShelfConflictAsk,
) -> SheetSpec {
    let own = ask.own;
    let arrival_reads_in_place = ask.opts.mode().reads_in_place();
    let reads_in_place =
        folders.iter().any(|f| f.root == ask.root && f.mode().reads_in_place());
    // The row promises the counter rather than asking the reader to take
    // "the next free name" on faith.
    let new_name = next_shelf_name(shelves, None, &ask.incoming_name);
    let replace_rows = if own && reads_in_place {
        // The tree's own linked rows are what the replace sweeps first, so
        // the copies that land come back in the names the shelves showed.
        let placed = folders
            .iter()
            .find(|f| f.root == ask.root && f.mode().reads_in_place())
            .map(|f| f.placed.clone())
            .unwrap_or_default();
        ledger::linked_rows_of(rows, &placed).len()
    } else {
        shelf::members_of(rows, shelves, &ask.existing_id).len()
    };
    let subtitle = if own {
        format!("Already in the library as “{}”", ask.existing_name)
    } else {
        format!("A shelf called “{}” is already here", ask.existing_name)
    };
    let question = if arrival_reads_in_place {
        "A folder read in place cannot mint a second shelf of itself. Leave a \
         pointer to the shelf that is here, or file this folder's books into it."
            .to_string()
    } else if own {
        format!(
            "“{}” is the shelf this folder's last import made. Look at it, \
             replace its books with these copies, or give the copies a shelf of \
             the next free name.",
            ask.existing_name
        )
    } else {
        "The arriving copies are the library's own, so all three answers are \
         open: look at the shelf that is here, replace its books, or shelve the \
         copies under the next free name."
            .to_string()
    };
    let show_note =
        format!("Import nothing — go to “{}” and light it up where it stands", ask.existing_name);
    let new_note = if reads_in_place {
        format!("Import as “{new_name}” — the library's own copies; the tree here keeps reading the folder")
    } else {
        format!("Import as “{new_name}” — its own shelf, its own tree")
    };
    let merge_note = format!(
        "The folder's books join “{}” — a name it already holds asks one by one",
        ask.existing_name
    );
    let replace_note = match replace_rows {
        0 => format!("Nothing to remove — the copies simply take “{}”", ask.existing_name),
        1 => format!(
            "One book leaves, highlights and all — a copy takes its place on “{}”",
            ask.existing_name
        ),
        n => format!("{n} books leave, highlights and all — copies take “{}”", ask.existing_name),
    };
    let choices = shelf_offers(ask)
        .iter()
        .map(|choice| match choice {
            Placement::LinkOnly => ChoiceSpec {
                label: "Make link",
                note: LINK_FOLDER_NOTE.to_string(),
                placement: Placement::LinkOnly,
            },
            Placement::Merge => ChoiceSpec {
                label: "Merge into it",
                note: merge_note.clone(),
                placement: Placement::Merge,
            },
            Placement::Open => ChoiceSpec {
                label: "Show it",
                note: show_note.clone(),
                placement: Placement::Open,
            },
            Placement::Replace => ChoiceSpec {
                label: "Replace",
                note: replace_note.clone(),
                placement: Placement::Replace,
            },
            Placement::KeepBoth => ChoiceSpec {
                label: "Add as new",
                note: new_note.clone(),
                placement: Placement::KeepBoth,
            },
        })
        .collect();
    SheetSpec {
        heading: ask.incoming_name.clone(),
        subtitle,
        question,
        apply_all: false,
        waiting: 0,
        choices,
    }
}

// ── The already-imported note ───────────────────────────────────────────
