//! The question rules' own cases.

use super::*;
use crate::library::conflicts::kit::{
ask, found, linked_row, placing_folder, plain_shelf, stored_row,
};
use crate::library::conflicts::words::describe;
use library_core::book::Row;
use library_core::shelf;
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
