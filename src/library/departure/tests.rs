use std::collections::{BTreeMap, HashSet};


use library_core::book::{find_book_mut, Row};
use library_core::folder::{Tombstone, WatchedFolder};
use library_core::shelf::{ALL_SHELF, Shelf};
use library_core::testkit;
use reader_core::format::Format;

use super::*;
use super::leaving::{departing_book_ids, departing_sets};
use super::returning::{return_path, target_is_family};

fn nested(n: u32) -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([testkit::fp_n(n)]),
        shelf_map: BTreeMap::from([
            (String::new(), "shelf1".to_string()),
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..testkit::watched_folder("f1", "/books")
    }
}

fn folder_with_moved_log() -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([testkit::fp_n(7)]),
        ignored: vec![Tombstone {
            fp: testkit::fp_n(7),
            title: Some("Dune".to_string()),
            format: Format::Markdown,
            last_path: "/books/Fiction/SciFi/dune.md".to_string(),
            shelf_id: Some("shelf3".to_string()),
            removed_ms: 5,
            moved: true,
            returned_row: None,
        }],
        shelf_map: BTreeMap::from([
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..testkit::watched_folder("f1", "/books")
    }
}

#[test]
fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
    let folders = vec![nested(7)];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    // Reading the tie as the folder's shelf tree instead — "any shelf
    // this folder owns" — left the row linked at an address it had been
    // dragged off.
    assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
    assert!(converts_on_move(&rows, &folders, "b1", "shelf1"));
    assert!(converts_on_move(&rows, &folders, "b1", "elsewhere"));
}

#[test]
fn a_reorder_on_the_book_s_own_rung_copies_nothing() {
    let folders = vec![nested(7)];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    // Re-ordering the books a folder placed, on the rung it placed them
    // on, is the folder's own business.
    assert!(!converts_on_move(&rows, &folders, "b1", "shelf3"));
}

#[test]
fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
    let folders = vec![nested(7)];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    assert!(converts_on_move(&rows, &folders, "b1", ALL_SHELF));
    assert!(converts_on_move(&rows, &folders, "b1", "mine"));
}

#[test]
fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
    let mut folder = nested(7);
    folder.shelf_map.remove("Fiction/SciFi");
    let folders = vec![folder];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
    assert!(converts_on_move(&rows, &folders, "b1", "shelf3"));
}

#[test]
fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
    let mut folder = nested(7);
    folder.opts.groups = false;
    folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
    let folders = vec![folder];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    assert!(!converts_on_move(&rows, &folders, "b1", "flat"));
    assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
}

#[test]
fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
    let mut copying = nested(7);
    copying.id = "f2".into();
    copying.opts.in_place = false;
    let folders = vec![nested(7), copying];
    let rows = vec![
        testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7),
        testkit::stored_row("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
        testkit::row_at_n("b3", "/elsewhere/loose.md", 11),
    ];

    assert!(converts_on_move(&rows, &folders, "b1", "shelf2"));
    assert!(!converts_on_move(&rows, &folders, "b2", "shelf2"));
    assert!(!converts_on_move(&rows, &folders, "b3", "shelf2"));
    assert!(!converts_on_move(&rows, &folders, "gone", "shelf2"));
}

#[test]
fn the_ask_names_the_ground_and_the_cost() {
    let folders = vec![nested(7)];
    let rows = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    let ids = vec!["b1".to_string()];
    let converting = converting_rows(&rows, &folders, &ids, "mine");
    assert_eq!(converting, ids, "the screen keeps the gesture's own order");

    let hand = RowMove::Seat { from: Some("shelf3".into()), to: "mine".into(), index: None };
    let ask = ask_of_rows(&rows, &folders, &converting, hand).expect("one book owes a copy");
    assert_eq!(ask.action, "Move books");
    assert_eq!(ask.subject, "1 book from “books”", "the folder names the ground");
    assert!(ask.lines[0].contains("read in place"), "the cost, in the sheet's own sentence");
    assert_eq!(ask.lines[1], UNTOUCHED);
    assert_eq!(ask.options.len(), 1, "the move's door has one way through");
    assert_eq!(ask.options[0].answer, CopyAnswer::Copy);

    // A gesture nothing converts raises no sheet.
    let hand = RowMove::Seat { from: None, to: "mine".into(), index: None };
    assert!(ask_of_rows(&rows, &folders, &[], hand).is_none());
}

#[test]
fn a_departure_writes_its_moved_log_on_the_placing_folder() {
    let rows = [testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    let book = rows[0].book().expect("the row is a book");
    let mut held = testkit::folder_shelf("shelf3", "shelf3", "f1", Some("Fiction/SciFi"), &[], None);
    held.books = vec!["b1".to_string()];
    let shelves = vec![held];
    let mut folders = vec![nested(7)];

    write_moved_stones(&mut folders, &shelves, book, None, 42);

    let entry = folders[0].ignored.iter().find(|t| t.moved).expect("the moved log");
    assert_eq!(entry.last_path, "/books/Fiction/SciFi/dune.md", "the address the bind reads");
    assert_eq!(
        entry.shelf_id.as_deref(),
        Some("shelf3"),
        "the rung that held it is the restore's seat"
    );
    assert!(entry.returned_row.is_none(), "a departure names no return of its own");
}

#[test]
fn a_return_binds_the_log_by_the_address_they_share() {
    let mut rows = vec![testkit::stored_row("b1", "/books/Fiction/SciFi/dune.md", "/store/b1.md", 9)];
    find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
    let shelves = vec![testkit::folder_shelf("shelf2", "shelf2", "f1", Some("Fiction"), &[], None)];
    let mut folders = vec![folder_with_moved_log()];

    // The shape of a return: a stored row whose source is the log's own
    // last address, landing on a shelf the folder names.
    assert!(bind_returned(&rows, &shelves, &mut folders, "b1", "shelf2"));
    assert_eq!(
        folders[0].ignored[0].returned_row.as_deref(),
        Some("b1"),
        "bound to the row by the address they share"
    );
    assert!(
        !bind_returned(&rows, &shelves, &mut folders, "b1", "shelf2"),
        "a log already bound to this row binds nothing again"
    );

    // A departure's own landing is no return: the resume carries the
    // copies and skips the bind, so the log stays free for the row that
    // actually comes home. That skip is the app's resume; the bind it
    // skips is the one above.
    let mut fresh = vec![folder_with_moved_log()];
    let linked = vec![testkit::row_at_n("b1", "/books/Fiction/SciFi/dune.md", 7)];
    assert!(
        !bind_returned(&linked, &shelves, &mut fresh, "b1", "shelf2"),
        "a linked row is nobody's return — only the library's own copies bind"
    );
}

fn reading_folder() -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([
            testkit::fp_n(7),
            testkit::fp_n(8),
            testkit::fp_n(9),
            testkit::fp_n(14),
        ]),
        shelf_map: BTreeMap::from([
            (String::new(), "r".to_string()),
            ("Fiction".to_string(), "fic".to_string()),
            ("Fiction/SciFi".to_string(), "sf".to_string()),
        ]),
        ..testkit::watched_folder("f1", "/books")
    }
}

fn tree() -> Vec<Shelf> {
    vec![
        testkit::folder_shelf("r", "r", "f1", None, &["top", "shown2"], None),
        testkit::folder_shelf("fic", "fic", "f1", Some("Fiction"), &["mid"], Some("r")),
        testkit::folder_shelf("sf", "sf", "f1", Some("Fiction/SciFi"), &["deep", "shown2", "loose", "kept"], Some("fic")),
        testkit::shelf("mine", "Mine", &[], Some("fic")),
        testkit::shelf("elsewhere", "Elsewhere", &[], None),
    ]
}

fn tree_rows() -> Vec<Row> {
    vec![
        testkit::row_at_n("top", "/books/top.md", 9),
        testkit::row_at_n("mid", "/books/Fiction/other.md", 8),
        testkit::row_at_n("deep", "/books/Fiction/SciFi/dune.md", 7),
        testkit::row_at_n("shown2", "/books/top2.md", 14),
        testkit::row_at_n("loose", "/loose/x.md", 12),
        testkit::stored_row("kept", "/books/Fiction/SciFi/old.md", "/store/kept.md", 13),
    ]
}

/// `home/root/1st/2nd`: a read-at-place tree with a rung per folder, its
/// books on the lowest one.
fn deep_tree() -> (Vec<Shelf>, Vec<Row>, WatchedFolder) {
    let mut folder = reading_folder();
    folder.shelf_map = BTreeMap::from([
        (String::new(), "root".to_string()),
        ("1st".to_string(), "one".to_string()),
        ("1st/2nd".to_string(), "two".to_string()),
    ]);
    let shelves = vec![
        testkit::folder_shelf("root", "root", "f1", None, &["b0"], None),
        testkit::folder_shelf("one", "one", "f1", Some("1st"), &[], Some("root")),
        testkit::folder_shelf("two", "two", "f1", Some("1st/2nd"), &["b1", "b2"], Some("one")),
    ];
    let books = vec![
        testkit::row_at_n("b0", "/books/notes.md", 8),
        testkit::row_at_n("b1", "/books/1st/2nd/a.md", 7),
        testkit::row_at_n("b2", "/books/1st/2nd/b.md", 9),
    ];
    (shelves, books, folder)
}

#[test]
fn a_departing_rung_carries_the_books_standing_on_the_rungs_it_takes() {
    let shelves = tree();
    let folder = reading_folder();
    let (subtree, rungs) = departing_sets(&shelves, "f1", "sf");
    assert!(subtree.contains("sf"));
    assert!(rungs.contains("sf"));
    let ids = departing_book_ids(&tree_rows(), &shelves, &folder, &rungs, &subtree);
    assert_eq!(ids, vec!["deep".to_string()], "only the book whose OWN rung is the one leaving");
}

#[test]
fn a_book_shown_on_a_departing_rung_keeps_its_link_when_its_ground_stays() {
    let shelves = tree();
    let folder = reading_folder();
    // "shown2" is a member of "sf" below it, but its address stands on
    // the ROOT rung, which is not departing: the tree keeps answering
    // for it, and a copy of a book whose rung stays is a copy the reader
    // never asked for.
    let (subtree, rungs) = departing_sets(&shelves, "f1", "fic");
    let ids = departing_book_ids(&tree_rows(), &shelves, &folder, &rungs, &subtree);
    assert!(ids.contains(&"mid".to_string()), "the book of the level itself goes");
    assert!(!ids.contains(&"shown2".to_string()), "the guest of a departing rung stays linked");
    assert!(
        ids.contains(&"deep".to_string()),
        "a MOVE takes the whole subtree the directory rides with; the take-APART below \
         pays for the level's own book alone"
    );
}

#[test]
fn the_rung_question_counts_only_the_level_s_own_books() {
    let (shelves, rows) = (tree(), tree_rows());
    let folders = vec![reading_folder()];
    assert_eq!(
        books_the_rung_takes(&rows, &shelves, &folders, "fic"),
        vec!["mid".to_string()],
        "asked and answered through one walk, so the sheet and the copies cannot drift"
    );
    // The lowest rung: its own book goes; the guest on the root's ground,
    // the file no folder placed, and the library's own copy all stay out.
    assert_eq!(books_the_rung_takes(&rows, &shelves, &folders, "sf"), vec!["deep".to_string()]);
    // A reader's own shelf is nobody's ground, and an empty rung owes
    // nothing: neither is a question.
    assert!(ask_of_rung(&rows, &shelves, &folders, "mine").is_none());
    assert!(ask_of_rung(&rows, &shelves, &folders, "elsewhere").is_none());
}

#[test]
fn a_level_the_library_already_stores_comes_apart_without_a_question() {
    let mut shelves = tree();
    shelves
        .iter_mut()
        .find(|s| s.id == "sf")
        .expect("the lowest rung")
        .books = vec!["loose".to_string(), "kept".to_string()];
    let rows = tree_rows();
    let folders = vec![reading_folder()];
    // `kept` is the library's own copy already and `loose` is a file no
    // folder placed here: neither is a book to make a copy of.
    assert!(ask_of_rung(&rows, &shelves, &folders, "sf").is_none());
}

#[test]
fn the_rung_question_words_the_level_and_the_way_up() {
    let (shelves, rows, folder) = deep_tree();
    let folders = vec![folder];
    let ask = ask_of_rung(&rows, &shelves, &folders, "two").expect("the level reads in place");
    assert_eq!(ask.action, "Take shelf apart", "the count belongs to the subject, not the verb");
    assert!(ask.subject.starts_with("“two”"), "the level the reader picked: {}", ask.subject);
    assert!(ask.subject.contains("2 books from “books”"), "the cost and the ground: {}", ask.subject);
    assert!(
        ask.lines[0].contains("The 2 books read in place here become copies and come up to “one”"),
        "the way up is the nearest rung still standing: {}",
        ask.lines[0]
    );
    assert_eq!(ask.lines[1], UNTOUCHED);
    assert_eq!(ask.options.len(), 1);
    assert_eq!(ask.options[0].label, "Copy and take apart");
    assert_eq!(ask.options[0].answer, CopyAnswer::Copy);
    assert_eq!(ask.work, CopyWork::Rung { id: "two".to_string() });

    // An empty rung of the same tree is no question.
    assert!(ask_of_rung(&rows, &shelves, &folders, "one").is_none());
}

#[test]
fn the_removal_question_offers_the_copy_and_the_folder_s_own_remaking() {
    let (shelves, rows, folder) = deep_tree();
    let folders = vec![folder];
    let going = vec!["two".to_string()];
    let ask =
        ask_of_removal(&rows, &shelves, &folders, &[], &going).expect("books read in place");
    assert_eq!(ask.action, "Remove shelf");
    assert_eq!(ask.subject, "1 shelf");
    assert_eq!(ask.lines[0], "2 Read in place, so removing them stores copies.");
    assert_eq!(ask.lines[1], "Or let “books” make it again.");
    assert_eq!(ask.options[0].label, "Copy and remove");
    assert_eq!(ask.options[0].answer, CopyAnswer::Copy);
    assert_eq!(ask.options[1].label, "Let “books” make it again");
    assert_eq!(ask.options[1].answer, CopyAnswer::WithoutCopies);
    assert!(!ask.options[1].primary);
    assert_eq!(
        ask.work,
        CopyWork::Removal { purge: Vec::new(), shelves: going.clone() }
    );

    // A removal that is already taking the books out of the library has
    // nothing left to copy: no question.
    let purge = vec!["b1".to_string(), "b2".to_string()];
    assert!(ask_of_removal(&rows, &shelves, &folders, &purge, &going).is_none());
}

#[test]
fn the_ask_names_the_copies_the_level_s_next_free_names() {
    let mut other = reading_folder();
    other.id = "f2".into();
    other.root = "/more".into();
    other.placed = HashSet::new();
    other.shelf_map = BTreeMap::from([
        (String::new(), "r2".to_string()),
        ("Fiction".to_string(), "fic2".to_string()),
    ]);
    let folders = vec![reading_folder(), other];
    let mut tree = tree();
    tree[1].name = "Fiction".to_string();
    let mut shelves = vec![
        testkit::shelf("to", "To", &[], None),
        testkit::shelf("held", "Fiction", &[], Some("to")),
    ];
    shelves.extend(tree);
    let mut fic2 = testkit::folder_shelf("fic2", "fic2", "f2", Some("Fiction"), &[], None);
    fic2.name = "Fiction".to_string();
    shelves.push(fic2);
    let rows = tree_rows();

    let ask = ask_of_shelf(
        &rows,
        &shelves,
        &folders,
        vec!["fic".to_string(), "fic2".to_string()],
        Some("to".to_string()),
        None,
    )
    .expect("two departing shelves are a question");
    assert_eq!(ask.action, "Move shelves", "the count belongs to the subject, not the verb");
    assert_eq!(ask.subject, "2 shelves — 2 books read in place");
    assert!(
        ask.lines[0].contains("their 2 books"),
        "the two levels' books are counted together in the one row: {}",
        ask.lines[0]
    );
    assert!(
        ask.lines[0].contains("“Fiction_1”") && ask.lines[0].contains("“Fiction_2”"),
        "and the row promises the names the copies will wear: {}",
        ask.lines[0]
    );
    assert_eq!(ask.lines[1], UNTOUCHED);
    assert!(
        ask.options.iter().all(|one| one.answer != CopyAnswer::WithoutCopies),
        "a reader's own shelf is nobody's family, so the drop owes no way home"
    );
    match &ask.work {
        CopyWork::Shelf { ids, target, seam, returns } => {
            assert_eq!(ids, &vec!["fic".to_string(), "fic2".to_string()]);
            assert_eq!(target.as_deref(), Some("to"));
            assert!(seam.is_none());
            assert!(returns.is_empty());
        }
        work => panic!("the shelf's work rides the shelf's ask: {work:?}"),
    }
}

/// f1's tree with a displaced member: the shape a removed rung and a
/// subfolder imported on its own leave behind.
fn family_state() -> (Vec<Shelf>, Vec<WatchedFolder>) {
    let mut tree = reading_folder();
    tree.shelf_map.remove("Fiction/SciFi");
    let mut member = reading_folder();
    member.id = "f3".into();
    member.root = "/books/Fiction/SciFi".into();
    member.shelf_map = BTreeMap::from([(String::new(), "s3".to_string())]);
    let shelves = vec![
        testkit::folder_shelf("r", "r", "f1", None, &[], None),
        testkit::folder_shelf("fic", "fic", "f1", Some("Fiction"), &[], Some("r")),
        testkit::folder_shelf("s3", "s3", "f3", None, &["deep"], None),
        testkit::shelf("mine", "Mine", &[], None),
    ];
    (shelves, vec![tree, member])
}

#[test]
fn a_displaced_folder_s_root_shelf_goes_home_by_the_fold() {
    let (shelves, folders) = family_state();
    match return_path(&shelves, &folders, "s3") {
        Some(ReturnPath::Reclaim { tree, gone, rel }) => {
            assert_eq!(tree, "f1", "the family the ground belongs to");
            assert_eq!(gone, "f3", "the folder that was reading it on its own");
            assert_eq!(rel, "Fiction/SciFi", "the rung its directory names");
        }
        path => panic!("the fold is a displaced root shelf's way home: {path:?}"),
    }
    assert!(target_is_family(&shelves, &folders, Some("fic"), "/books/Fiction/SciFi"));
    assert!(target_is_family(&shelves, &folders, Some("r"), "/books/Fiction/SciFi"));
    assert!(!target_is_family(&shelves, &folders, Some("mine"), "/books/Fiction/SciFi"));
    assert!(!target_is_family(&shelves, &folders, None, "/books/Fiction/SciFi"));
    assert!(!target_is_family(&shelves, &folders, Some("gone"), "/books/Fiction/SciFi"));
}

#[test]
fn an_off_seat_rung_goes_home_by_the_reseat_and_a_seated_one_is_home() {
    let (shelves, folders) = family_state();
    assert!(return_path(&shelves, &folders, "fic").is_none());
    let mut off = shelves.clone();
    off.iter_mut().find(|s| s.id == "fic").unwrap().parent = Some("mine".to_string());
    match return_path(&off, &folders, "fic") {
        Some(ReturnPath::Reseat { seat }) => assert_eq!(seat.as_deref(), Some("r")),
        path => panic!("the reseat is an off-seat rung's way home: {path:?}"),
    }
    let mut lifted = shelves.clone();
    lifted.iter_mut().find(|s| s.id == "r").unwrap().parent = Some("mine".to_string());
    match return_path(&lifted, &folders, "r") {
        Some(ReturnPath::Reseat { seat }) => {
            assert_eq!(seat, None, "the root's seat is the library's own level")
        }
        path => panic!("the root's seat is the library's own level: {path:?}"),
    }
}
