//! The scan decisions' own cases.

use super::*;
use std::collections::HashSet;
use crate::book::{Book, Fingerprint, Origin, Row};
use crate::ledger::kit::{file, folder, fp, registry};
use crate::ledger::registry::{KnownBook, Registry, registry_of};
use reader_core::format::Format;

#[test]
fn an_unknown_fingerprint_is_added() {
    let f = folder(&[], &[]);
    assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Add(file(1, "/books/a.pdf")));
}

/// The tracking tree's quiet half: a rung turned off under a watched root
/// adds nothing on a rescan, while an explicit import of the same ground
/// still adds what it finds.
#[test]
fn a_rung_nobody_watches_adds_nothing_on_a_rescan() {
    let mut f = folder(&[], &[]);
    f.set_tracking("Fiction", false);
    let reg = registry(&[]);
    assert_eq!(
        decide(&f, &reg, &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf"))
    );
    assert_eq!(
        decide(&f, &reg, &file(4, "/books/Poetry/d.pdf")),
        ScanAction::Add(file(4, "/books/Poetry/d.pdf"))
    );
    assert_eq!(decide(&f, &reg, &file(2, "/books/Fiction/b.pdf")), ScanAction::Skip);
    assert_eq!(decide(&f, &reg, &file(3, "/books/Fiction/SciFi/c.pdf")), ScanAction::Skip);
    assert_eq!(
        decide_import(&f, &reg, &file(2, "/books/Fiction/b.pdf")),
        ScanAction::Add(file(2, "/books/Fiction/b.pdf"))
    );
}

#[test]
fn a_known_book_at_its_own_address_is_skipped() {
    let f = folder(&[1], &[]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
    assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
}

#[test]
fn a_known_book_at_a_new_address_is_relinked() {
    let f = folder(&[1], &[]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
    assert_eq!(
        decide(&f, &r, &file(1, "/books/moved/a.pdf")),
        ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/moved/a.pdf".into()
        }
    );
}

#[test]
fn a_missing_book_is_relinked_by_a_folder_that_never_placed_it() {
    let f = folder(&[], &[]);
    let r = registry(&[(1, "b1", "/gone/a.pdf", true)]);
    assert!(matches!(
        decide(&f, &r, &file(1, "/books/a.pdf")),
        ScanAction::Relink { .. }
    ));
}

#[test]
fn a_second_folder_never_relinks_a_book_it_did_not_place() {
    let f = folder(&[], &[]);
    let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
    assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
}

/// The rule the whole ledger exists for: a book the reader moved off this
/// folder's shelf is still in the library, and a rescan must not put it
/// back.
#[test]
fn a_book_moved_off_the_folder_shelf_is_never_re_added() {
    let f = folder(&[1], &[]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
    assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    let f = folder(&[1], &[]);
    assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
}

#[test]
fn a_removed_book_stays_removed() {
    let f = folder(&[], &[1]);
    assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    let f = folder(&[1], &[1]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
    assert_eq!(decide(&f, &r, &file(1, "/books/elsewhere.pdf")), ScanAction::Skip);
}

/// A removal answers "should this come back on its own", not "the reader
/// is asking for it again".
#[test]
fn an_explicit_import_overrides_the_removals_that_wrote_the_tombstones() {
    let f = folder(&[], &[1]);
    assert_eq!(
        decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf"))
    );
    let f = folder(&[1], &[]);
    assert_eq!(
        decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf"))
    );
}

/// The import table keeps the rows about this folder: held at this
/// address is still a Skip, held at another address this folder placed is
/// still a Relink. It drops only the row that made a second folder's
/// identical copy invisible.
#[test]
fn an_explicit_import_duplicates_nothing_this_folder_placed() {
    let f = folder(&[1], &[]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
    assert_eq!(decide_import(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    assert_eq!(
        decide_import(&f, &r, &file(1, "/books/moved/a.pdf")),
        ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/moved/a.pdf".into()
        }
    );
    let f = folder(&[], &[]);
    let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
    assert_eq!(
        decide_import(&f, &r, &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf"))
    );
    assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    let placed = folder(&[1], &[]);
    let relink = ScanAction::Relink {
        book_id: "b1".into(),
        to: "/books/a.pdf".into(),
    };
    assert_eq!(decide_import(&placed, &r, &file(1, "/books/a.pdf")), relink);
}

/// The exception both tables carry: the registry named the library's own
/// copy of the file the walk stands on. Two addresses, not a move — the
/// copy is in the store, its source is here.
#[test]
fn a_copy_never_relinks_its_own_source() {
    let f = folder(&[1], &[]);
    let r = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/a.pdf")]);
    assert_eq!(
        decide(&f, &r, &file(1, "/books/a.pdf")),
        ScanAction::Skip,
        "a rescan of the copy's own source is quiet, however the folder placed it"
    );
    let stranger = folder(&[], &[]);
    assert_eq!(decide(&stranger, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    let other = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/elsewhere.pdf")]);
    assert_eq!(
        decide(&f, &other, &file(1, "/books/moved.pdf")),
        ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/moved.pdf".into()
        }
    );
    let mut dead = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/gone.pdf")]);
    dead.get_mut(&fp(1)).expect("a row").missing = true;
    assert_eq!(
        decide(&stranger, &dead, &file(1, "/books/a.pdf")),
        ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/a.pdf".into()
        }
    );
}

/// The one case where the two tables part company: an import is the
/// reader asking for THIS file, and the library's copy is not it, so the
/// quiet answer becomes an `Add`.
#[test]
fn an_explicit_import_of_a_copy_source_asks_for_the_files_own_book() {
    let f = folder(&[1], &[]);
    let r = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/a.pdf")]);
    assert_eq!(
        decide_import(&f, &r, &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf")),
        "the file is not the copy, so the import owes it a book of its own"
    );
    let stranger = folder(&[], &[]);
    assert_eq!(
        decide_import(&stranger, &r, &file(1, "/books/a.pdf")),
        ScanAction::Add(file(1, "/books/a.pdf"))
    );
}

// A linked book's source IS its path, so the copy's exception can never
// swallow a move heal.

#[test]
fn a_linked_book_is_never_a_copy_of_itself() {
    let rows = vec![Row::Book(Book::new(
        "b1".into(),
        fp(1),
        Format::Markdown,
        Origin::Linked {
            src: "/books/a.pdf".into(),
        },
        0,
    ))];
    let r = registry_of(&rows);
    assert_eq!(r.get(&fp(1)).and_then(|k| k.source.clone()), None);
    let f = folder(&[1], &[]);
    assert_eq!(
        decide(&f, &r, &file(1, "/books/moved/a.pdf")),
        ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/moved/a.pdf".into()
        },
        "the file moved inside the tree, and the heal is what a rescan owes it"
    );
}

#[test]
fn a_whole_walk_answers_in_order_and_only_changes_are_changes() {
    let f = folder(&[2], &[3]);
    let r = registry(&[(2, "b2", "/books/b.pdf", false)]);
    let walk = vec![
        file(1, "/books/a.pdf"),
        file(2, "/books/b.pdf"),
        file(3, "/books/c.pdf"),
        file(4, "/books/sub/d.pdf"),
    ];
    let actions = diff_folder(&f, &r, &walk);
    assert_eq!(actions.len(), walk.len());
    assert!(changes(&actions[0]));
    assert!(!changes(&actions[1]));
    assert!(!changes(&actions[2]));
    assert!(changes(&actions[3]));
    assert_eq!(actions.iter().filter(|a| changes(a)).count(), 2);
}

#[test]
fn an_unchanged_folder_costs_one_state_write_nothing() {
    let f = folder(&[1, 2], &[]);
    let r = registry(&[(1, "b1", "/books/a.pdf", false), (2, "b2", "/books/b.pdf", false)]);
    let walk = vec![file(1, "/books/a.pdf"), file(2, "/books/b.pdf")];
    assert!(diff_folder(&f, &r, &walk).iter().all(|a| !changes(a)));
}

#[test]
fn the_replace_list_is_the_linked_rows_the_ledger_answers_for() {
    let rows = vec![
        Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Linked {
                src: "/one/a.md".into(),
            },
            0,
        )),
        Row::Book(Book::new(
            "b2".into(),
            fp(2),
            Format::Markdown,
            Origin::Stored {
                src: Some("/one/b.md".into()),
                store: "/store/b2.md".into(),
            },
            0,
        )),
        Row::Book(Book::new(
            "b3".into(),
            fp(3),
            Format::Markdown,
            Origin::Linked {
                src: "/one/c.md".into(),
            },
            0,
        )),
    ];
    let placed: HashSet<Fingerprint> = [fp(1), fp(2), fp(3)].into_iter().collect();
    assert_eq!(
        linked_rows_of(&rows, &placed),
        vec!["b1".to_string(), "b3".to_string()],
        "a stored book is already the library's own and never converts"
    );
}

#[test]
fn a_relink_onto_an_address_a_row_already_reads_is_dropped() {
    // Two rows, one fingerprint: a walk of the other folder would
    // rewrite b1's address out from under it.
    let rows = vec![
        Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Linked {
                src: "/one/a.md".into(),
            },
            0,
        )),
        Row::Book(Book::new(
            "b2".into(),
            fp(2),
            Format::Markdown,
            Origin::Linked {
                src: "/one/gone.md".into(),
            },
            0,
        )),
    ];
    let mut relinks = vec![
        ("b1".to_string(), "/one/a.md".to_string()),
        ("b2".to_string(), "/one/moved.md".to_string()),
    ];
    keep_healable_relinks(&mut relinks, &rows);
    assert_eq!(
        relinks,
        vec![("b2".to_string(), "/one/moved.md".to_string())],
        "the heal that moves nobody stays, and the one that would steal an address goes"
    );
}

#[test]
fn placing_a_file_is_what_makes_the_next_scan_skip_it() {
    let mut f = folder(&[], &[]);
    f.mark_placed(fp(1));
    assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
}

/// A test-local helper rather than a method: nothing in the app asks
/// whether an action changes anything.
fn changes(action: &ScanAction) -> bool {
    !matches!(action, ScanAction::Skip)
}

/// A registry whose rows are the library's own copies: each entry carries
/// the address its bytes were made from.
fn copied_registry(rows: &[(u32, &str, &str, &str)]) -> Registry {
    rows.iter()
        .map(|(n, id, store, source)| {
            (
                fp(*n),
                KnownBook {
                    id: (*id).to_string(),
                    path: (*store).to_string(),
                    missing: false,
                    source: Some((*source).to_string()),
                },
            )
        })
        .collect()
}
