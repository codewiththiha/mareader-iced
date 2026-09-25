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

#[cfg(test)]
mod tests {
    use crate::library::conflicts::kit::{linked_row, plain_shelf, stored_row};
    use crate::library::conflicts::naming::ShelfConflictAsk;
    use crate::library::conflicts::naming::describe_shelf;
    use crate::library::conflicts::naming::shelf_offers;
    use library_core::conflict::Placement;
    use library_core::folder::FolderOpts;
    use library_core::folder::WatchedFolder;
    use library_core::shelf::Shelf;
    use library_core::testkit;

    #[test]
    fn the_shelf_question_s_offer_follows_the_arrival_s_mode() {
        let stored = shelf_ask("Books", "s1", "/mine/Books", storing_opts(), false);
        assert_eq!(shelf_offers(&stored), Placement::SHELF_STORED);
        let in_place = shelf_ask("Books", "s1", "/mine/Books", in_place_opts(), false);
        assert_eq!(shelf_offers(&in_place), Placement::SHELF_READ_IN_PLACE);
    }

    #[test]
    fn the_stored_folder_s_question_is_the_level_s_own() {
        // An arrival the library will own: no pointer, and every cost is the
        // level's own to name — the replace counts the shelf's own members.
        let rows = vec![
            stored_row("e1", "Dune", "/mine/dune.md", "/store/e1.md", 9),
            stored_row("e2", "Other", "/elsewhere/x.md", "/store/e2.md", 11),
        ];
        let shelves = vec![named_shelf("s1", "Books", &["e1", "e2"])];
        let ask = shelf_ask("Books", "s1", "/mine/Books", storing_opts(), false);
        let spec = describe_shelf(&rows, &shelves, &[], &ask);
        assert_eq!(spec.heading, "Books");
        assert_eq!(spec.subtitle, "A shelf called “Books” is already here".to_string());
        assert!(spec.question.contains("all three answers are open"), "the stored arrival's question");
        assert!(!spec.apply_all && spec.waiting == 0, "a whole-run question has no queue");
        assert_eq!(
            spec.choices.iter().map(|c| c.placement).collect::<Vec<_>>(),
            Placement::SHELF_STORED
        );
        let replace = spec.choices.iter().find(|c| c.placement == Placement::Replace).unwrap();
        assert_eq!(
            replace.note,
            "2 books leave, highlights and all — copies take “Books”"
        );
        let show = spec.choices.iter().find(|c| c.placement == Placement::Open).unwrap();
        assert_eq!(show.note, "Import nothing — go to “Books” and light it up where it stands");
        let new = spec.choices.iter().find(|c| c.placement == Placement::KeepBoth).unwrap();
        assert_eq!(new.note, "Import as “Books_1” — its own shelf, its own tree");
    }

    #[test]
    fn the_own_tree_s_question_words_a_continuation() {
        // The arriving folder is the tree's own: the replace sweeps the
        // tree's LINKED rows — a stored copy beside the tree stays — and
        // every sentence says which import made the shelf.
        let rows = vec![
            linked_row("e1", "Dune", "/mine/Books/dune.md", 7),
            stored_row("e2", "Copy", "/mine/Books/copy.md", "/store/e2.md", 12),
        ];
        let shelves = vec![named_shelf("s1", "Books", &["e1", "e2"])];
        let folders = vec![in_place_folder("/mine/Books", &[7])];
        let ask = shelf_ask("Books", "s1", "/mine/Books", storing_opts(), true);
        let spec = describe_shelf(&rows, &shelves, &folders, &ask);
        assert_eq!(spec.subtitle, "Already in the library as “Books”");
        assert!(
            spec.question.contains("the shelf this folder's last import made"),
            "the continuation's own words: {}",
            spec.question
        );
        let replace = spec.choices.iter().find(|c| c.placement == Placement::Replace).unwrap();
        assert_eq!(
            replace.note,
            "One book leaves, highlights and all — a copy takes its place on “Books”",
            "the stored copy on the shelf is not the tree's to empty"
        );
        let new = spec.choices.iter().find(|c| c.placement == Placement::KeepBoth).unwrap();
        assert_eq!(
            new.note,
            "Import as “Books_1” — the library's own copies; the tree here keeps reading the folder"
        );
    }

    #[test]
    fn the_in_place_arrival_offers_the_pointer_and_the_merge() {
        let rows = vec![linked_row("e1", "Dune", "/mine/Books/dune.md", 7)];
        let shelves = vec![plain_shelf("s1", &["e1"])];
        let ask = shelf_ask("Books", "s1", "/mine/Books", in_place_opts(), true);
        let spec = describe_shelf(&rows, &shelves, &[], &ask);
        assert!(spec.question.contains("cannot mint a second shelf of itself"));
        assert_eq!(
            spec.choices.iter().map(|c| c.placement).collect::<Vec<_>>(),
            Placement::SHELF_READ_IN_PLACE
        );
        let link = spec.choices.iter().find(|c| c.placement == Placement::LinkOnly).unwrap();
        assert!( link.note.contains("lights the folder where it is"), "the pointer's promise");
        let merge = spec.choices.iter().find(|c| c.placement == Placement::Merge).unwrap();
        assert_eq!(merge.note, "The folder's books join “Books” — a name it already holds asks one by one");
    }

    #[test]
    fn an_empty_level_s_replace_asks_nothing_back() {
        let shelves = vec![plain_shelf("s1", &[])];
        let ask = shelf_ask("Books", "s1", "/mine/Books", storing_opts(), false);
        let spec = describe_shelf(&[], &shelves, &[], &ask);
        let replace = spec.choices.iter().find(|c| c.placement == Placement::Replace).unwrap();
        assert_eq!(replace.note, "Nothing to remove — the copies simply take “Books”");
    }

    /// A shelf of its own name, which is what every collision is about: the
    /// arriving folder's name is a name the level already holds.
    fn named_shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf { name: name.to_string(), ..plain_shelf(id, books) }
    }

    fn shelf_ask(incoming: &str, existing_id: &str, root: &str, opts: FolderOpts, own: bool) -> ShelfConflictAsk {
        ShelfConflictAsk {
            incoming_name: incoming.to_string(),
            existing_id: existing_id.to_string(),
            existing_name: incoming.to_string(),
            root: root.to_string(),
            opts,
            own,
        }
    }

    fn storing_opts() -> FolderOpts {
        // The second instance the library owns outright: a copy of every
        // admitted file in the store.
        FolderOpts { in_place: false, ..FolderOpts::default() }
    }

    fn in_place_opts() -> FolderOpts {
        // Read at place and watched: every answer the in-place family makes,
        // the tree answered first.
        FolderOpts { watch: true, ..FolderOpts::default() }
    }

    fn in_place_folder(root: &str, placed: &[u32]) -> WatchedFolder {
        WatchedFolder {
            placed: placed.iter().map(|n| testkit::fp_n(*n)).collect(),
            ..testkit::watched_folder("tree1", root)
        }
    }
}
