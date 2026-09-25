//! The schema's own cases.

use super::*;
use crate::book::check::{add_book, apply_check};
use crate::book::kit::{at, check, fp, linked, private, rows};
use crate::book::query::{find_book_mut, find_by_id};
use crate::book::read::{ReadPoint, record_read};
use crate::book::sanitize::sanitize;
use reader_core::format::Format;

#[test]
fn a_stored_book_opens_from_the_store_and_remembers_its_source() {
    let b = Book {
        origin: Origin::Stored {
            src: Some("/downloads/dune.pdf".into()),
            store: "/app/Library/pdf/dune_1a2b3c4d.pdf".into(),
        },
        ..linked("a", "/ignored")
    };
    assert_eq!(b.path(), "/app/Library/pdf/dune_1a2b3c4d.pdf");
    assert_eq!(b.origin.source(), Some("/downloads/dune.pdf"));
    assert!(b.origin.is_stored());
}

#[test]
fn the_title_falls_back_to_the_stem_and_never_to_nothing() {
    let mut b = linked("a", "/books/Dune.pdf");
    assert_eq!(b.title(), "Dune");
    b.title = Some("  ".into());
    assert_eq!(b.title(), "Dune", "a blank title is no title");
    b.origin = Origin::Linked { src: "/".into() };
    assert_eq!(b.title(), "/", "the address is the last resort");
}

#[test]
fn two_rows_of_one_file_share_its_reading_truth() {
    // A kept duplicate is a second row, not a second file: reads, checks
    // and heals reach every row at the address, or a twin's placeholder
    // would hold a folder's rescan off forever.
    let mut books = rows([
        Book {
            fp_pending: true,
            ..linked("a", "/books/dune.pdf")
        },
        Book {
            id: "b".into(),
            title: Some("dune_1".into()),
            fp_pending: true,
            ..linked("a", "/books/dune.pdf")
        },
    ]);
    assert!(record_read(
        &mut books,
        "/books/dune.pdf",
        Some("Dune".into()),
        None,
        ReadPoint { page: 90, num_pages: 400, fraction: None },
        700,
    )
    .is_none());
    for book in book_rows(&books) {
        assert_eq!(book.page, 90);
        assert_eq!(book.last_read_ms, 700);
    }
    assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
    assert_eq!(at(&books, 1).title.as_deref(), Some("dune_1"));
    assert_eq!(
        apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
        2,
        "one measurement heals every row at the address"
    );
    assert!(book_rows(&books).all(|b| !b.fp_pending && b.fp == fp(20, 2, 8)));
}

#[test]
fn an_import_resolves_to_a_shared_row_and_never_to_a_private_one() {
    // An import must not resolve to a private book.
    let mut books = rows([private("a", "/one/dune.pdf")]);
    let arrival = Book {
        origin: Origin::Linked { src: "/two/dune.pdf".into() },
        ..linked("new", "/two/dune.pdf")
    };
    assert_eq!(add_book(&mut books, arrival), "new", "a private row holds nothing back");
    assert_eq!(books.len(), 2);
    let again = linked("new2", "/three/dune.pdf");
    assert_eq!(add_book(&mut books, again), "new");
    assert_eq!(books.len(), 2);
}

#[test]
fn a_relink_heals_a_missing_book() {
    let mut books = rows([Book {
        missing: true,
        ..linked("a", "/gone/one.pdf")
    }]);
    assert!(crate::ledger::relink(&mut books, "a", "/books/one.pdf"));
    assert!(!at(&books, 0).missing);
    assert_eq!(at(&books, 0).path(), "/books/one.pdf");
}

#[test]
fn the_same_content_is_the_same_book_whatever_its_address() {
    let mut books = rows([linked("a", "/books/one.pdf")]);
    let twin = Book {
        id: "twin".into(),
        origin: Origin::Linked {
            src: "/other/one.pdf".into(),
        },
        ..linked("a", "/books/one.pdf")
    };
    assert_eq!(add_book(&mut books, twin), "a");
    assert_eq!(books.len(), 1, "a second address never makes a second book");
}

#[test]
fn an_import_files_in_the_order_the_walk_produced() {
    // Inserting each file at the front would reverse a whole folder.
    let mut books: Vec<Row> = Vec::new();
    for name in ["a", "b", "c", "d"] {
        let book = Book {
            fp: fp(name.as_bytes()[0] as u64, 1, 1),
            ..linked(name, &format!("/books/{name}.pdf"))
        };
        add_book(&mut books, book);
    }
    let ids: Vec<&str> = books.iter().map(Row::id).collect();
    assert_eq!(ids, vec!["a", "b", "c", "d"]);
}

#[test]
fn sanitize_dedupes_by_id_and_clamps_the_resume() {
    let mut books = rows([
        Book {
            page: 0,
            ..linked("a", "/books/one.pdf")
        },
        Book {
            origin: Origin::Linked {
                src: "/copies/one.pdf".into(),
            },
            ..linked("a", "/books/one.pdf")
        },
        // Same content under two ids is a duplicate the reader chose to keep; a fingerprint dedupe would take it back.
        Book {
            id: "dup".into(),
            title: Some("one_1".into()),
            ..linked("dup", "/books/one.pdf")
        },
        Book {
            id: "  ".into(),
            ..linked("blank", "/books/three.pdf")
        },
        Book {
            fp: fp(99, 9, 9),
            fraction: Some(2.0),
            ..linked("c", "/books/four.md")
        },
    ]);
    sanitize(&mut books);
    let ids: Vec<&str> = books.iter().map(Row::id).collect();
    assert_eq!(ids, vec!["a", "dup", "c"]);
    assert_eq!(at(&books, 0).page, 1, "page 0 clamps to 1");
    assert_eq!(at(&books, 1).title.as_deref(), Some("one_1"), "a duplicate keeps the name it was minted with");
    assert_eq!(at(&books, 2).fraction, None, "an impossible fraction is dropped");
}

#[test]
fn a_title_the_reader_chose_survives_the_debris_rule() {
    // The title rule hunts document-supplied debris; a reader-typed name is locked.
    let mut books = rows([
        Book {
            title: Some("0321894073.pdf".into()),
            ..linked("doc", "/books/one.pdf")
        },
        Book {
            id: "ren".into(),
            title: Some("my_report_final.pdf".into()),
            title_locked: true,
            ..linked("ren", "/books/two.pdf")
        },
    ]);
    sanitize(&mut books);
    assert_eq!(
        at(&books, 0).title, None,
        "a document's download debris still goes"
    );
    assert_eq!(
        at(&books, 1).title.as_deref(),
        Some("my_report_final.pdf"),
        "the reader's own name stays, whatever it looks like"
    );
}

#[test]
fn a_stored_book_is_named_by_the_file_it_came_from() {
    // Store bytes are named `source.pdf`, so the store address's stem is a layout artifact.
    let stored = Book {
        origin: Origin::Stored {
            src: Some("/downloads/dune.pdf".into()),
            store: "/app/Library/items/b1/source.pdf".into(),
        },
        ..linked("b1", "/app/Library/items/b1/source.pdf")
    };
    assert_eq!(stored.title(), "dune");
    assert_eq!(stored.stem(), "dune");
    let orphan = Book {
        origin: Origin::Stored {
            src: None,
            store: "/app/Library/items/b2/source.pdf".into(),
        },
        ..linked("b2", "/app/Library/items/b2/source.pdf")
    };
    assert_eq!(orphan.title(), "source");
    let named = Book {
        title: Some("Dune".into()),
        ..stored
    };
    assert_eq!(named.title(), "Dune");
}

#[test]
fn a_store_stem_burnt_into_a_title_is_healed_and_a_readers_own_is_not() {
    // A stored book's open-address stem is the store's own "source": a
    // burn-in the load drops.
    let mut books = rows([
        Book {
            title: Some("source".into()),
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            ..linked("b1", "/app/Library/items/b1/source.pdf")
        },
        Book {
            title: Some("source".into()),
            title_locked: true,
            origin: Origin::Stored {
                src: Some("/downloads/foundation.pdf".into()),
                store: "/app/Library/items/b2/source.pdf".into(),
            },
            ..linked("b2", "/app/Library/items/b2/source.pdf")
        },
    ]);
    sanitize(&mut books);
    assert_eq!(at(&books, 0).title, None, "the burn-in goes");
    assert_eq!(at(&books, 0).title(), "dune", "and the source's stem shows");
    assert_eq!(
        at(&books, 1).title.as_deref(),
        Some("source"),
        "the reader's own name stays, however plain"
    );
}

#[test]
fn a_link_to_a_shelf_survives_the_row_sweep() {
    // A shelf id is never a book id; which shelves exist is the shelf sweep's question.
    let mut books = rows([linked("a", "/books/one.pdf")]);
    books.push(Row::link("l1".into(), "Books".into(), "s1".into(), 1));
    books.push(Row::link("l2".into(), "Gone".into(), "zz".into(), 1));
    sanitize(&mut books);
    let ids: Vec<&str> = books.iter().map(Row::id).collect();
    assert_eq!(ids, vec!["a", "l1"], "the book link at a gone book goes; the shelf link stays");
}

#[test]
fn the_storage_cap_evicts_the_least_recently_read() {
    let mut books: Vec<Row> = (0..(BOOKS_CAP + 2))
        .map(|i| {
            Row::Book(Book {
                last_read_ms: i as u64,
                ..linked(&format!("b{i}"), &format!("/books/{i}.pdf"))
            })
        })
        .collect();
    sanitize(&mut books);
    assert_eq!(books.len(), BOOKS_CAP);
    assert!(
        book_rows(&books).all(|b| b.last_read_ms >= 2),
        "the two never-read books go first"
    );
}

#[test]
fn a_book_survives_a_round_trip_through_storage() {
    let book = Book {
        id: "b1".into(),
        fp: fp(1024, 1_700_000_000_000, 0xdead_beef),
        title: Some("Dune".into()),
        author: Some("Frank Herbert".into()),
        format: Format::Pdf,
        origin: Origin::Stored {
            src: Some("/downloads/dune.pdf".into()),
            store: "/app/store/pdf/dune_b1.pdf".into(),
        },
        added_ms: 5,
        last_read_ms: 9,
        page: 12,
        num_pages: 400,
        fraction: None,
        missing: true,
        fp_pending: false,
        independent: true,
        title_locked: false,
    };
    let json = serde_json::to_string(&book).unwrap();
    assert!(json.contains("\"kind\":\"stored\""), "{json}");
    assert!(json.contains("\"format\":\"pdf\""), "{json}");
    let back: Book = serde_json::from_str(&json).unwrap();
    assert_eq!(back, book);
}

#[test]
fn a_blob_from_before_a_field_existed_still_loads() {
    // Every field has a default, so a row from an older build loads rather than dropping the library.
    let b: Book = serde_json::from_str(
        r#"{"id":"b1","fp":{"size":1,"mtimeMs":2,"headHash":3},
            "format":"markdown","origin":{"kind":"linked","src":"/n.md"}}"#,
    )
    .unwrap();
    assert_eq!(b.page, 1);
    assert!(!b.missing);
    assert_eq!(b.title, None);
    assert_eq!(b.path(), "/n.md");
}

#[test]
fn the_mutable_book_lookup_steps_over_a_link() {
    let mut list = vec![
        Row::link("l1".into(), "Dune".into(), "b1".into(), 1),
        Row::Book(linked("b1", "/a.pdf")),
    ];
    // A writer of a book's facts must not reach a link row.
    assert!(find_row_mut(&mut list, "l1").is_some());
    assert!(find_book_mut(&mut list, "l1").is_none());
    find_book_mut(&mut list, "b1").unwrap().missing = true;
    assert!(find_by_id(&list, "b1").is_some_and(|b| b.missing));
    assert!(find_book_mut(&mut list, "gone").is_none());
}

#[test]
fn a_stored_book_knows_the_address_it_was_copied_from() {
    let copy = Book::new(
        "b1".into(),
        fp(1, 1, 1),
        Format::Pdf,
        Origin::Stored {
            src: Some("/src/a.pdf".into()),
            store: "/store/b1.pdf".into(),
        },
        1,
    );
    assert!(copy.origin.is_store_copy_of("/src/a.pdf"));
    assert!(!copy.origin.is_store_copy_of("/store/b1.pdf"), "the copy is not its own provenance");
    assert!(!copy.origin.is_store_copy_of("/src/other.pdf"));
    // A linked book is the address, and a copy whose source is gone has no provenance to match.
    assert!(!linked("b2", "/src/a.pdf").origin.is_store_copy_of("/src/a.pdf"));
    let orphan = Book::new(
        "b3".into(),
        fp(1, 1, 1),
        Format::Pdf,
        Origin::Stored {
            src: None,
            store: "/store/b3.pdf".into(),
        },
        1,
    );
    assert!(!orphan.origin.is_store_copy_of("/src/a.pdf"));
}

#[test]
fn a_departure_keeps_the_name_the_shelf_showed() {
    // The store file is named after the row's id, so an untitled row would
    // read as "b1c2d3" the moment it left.
    let mut b = linked("b1", "/src/Dune.pdf");
    b.become_stored("/src/Dune.pdf", "/store/b1.pdf".into(), Some(fp(9, 9, 9)));
    assert_eq!(b.title.as_deref(), Some("Dune"));
    assert_eq!(
        b.origin,
        Origin::Stored {
            src: Some("/src/Dune.pdf".into()),
            store: "/store/b1.pdf".into()
        }
    );
    assert_eq!(b.fp, fp(9, 9, 9), "the copy's own measurement is the identity");
    assert!(!b.fp_pending);
    assert!(!b.missing);
    // The opened address is the copy's now and the source is provenance,
    // leaving the original fingerprint free for the folder that reads it.
    assert_eq!(b.path(), "/store/b1.pdf");
    assert_eq!(b.origin.source(), Some("/src/Dune.pdf"));
}
