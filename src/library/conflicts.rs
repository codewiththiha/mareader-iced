//! The name question: a level already holds a book of the name that is
//! arriving, and the sheet asks which of three things the reader meant.
//!
//! The RULE is not here — it is `library_core::conflict`, pure and
//! host-tested: what collides, what the next free name is, which answers a
//! shape of question offers. This module is the wiring between that rule and
//! the things a reader can do about it: the screen every placing surface
//! hands its arrivals through, the queue that holds the questions a busy
//! sheet cannot take yet, and the sheet's own words.
//!
//! The move side is the whole of it today. The web app asks three more
//! questions on the same chrome — a merged folder's per-file ask, a covered
//! file's ground, and a folder's own name collision — and all three are the
//! import run's business; they arrive with the import's own screen. The
//! import half of an answer (`Arrival::file`) is ported where it is pure and
//! documented where it lands, so the offers cannot drift when it arrives.
//!
//! Two spellings the native app does not carry yet, both where the web has
//! them: the replace note counts highlights the reader has no store for
//! until the engines land, and the "already imported" answer navigates
//! without the web's scroll-and-flash, which waits on the grid's scroll-to.

use library_core::book::{find_by_id, find_row, Book, Row};
use library_core::conflict::{collide, next_name, Arrival, Placement};
use library_core::folder::WatchedFolder;
use library_core::shelf::{self, Shelf, ALL_SHELF};

use super::departure;

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
                asks.push(ConflictAsk { arrival, existing_id, existing_name });
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
pub fn offers_for(rows: &[Row], folders: &[WatchedFolder], ask: &ConflictAsk) -> &'static [Placement] {
    if ask.arrival.is_import() {
        Placement::FILE
    } else if link_shape(rows, folders, ask) {
        Placement::MOVE_KEEPING_BOTH
    } else {
        Placement::MOVE
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

/// One answer row on the sheet: its name, the line that promises what
/// choosing it does, and the answer it carries.
#[derive(Clone, PartialEq, Debug)]
pub struct ChoiceSpec {
    pub label: &'static str,
    pub note: String,
    pub placement: Placement,
}

/// The question described rather than drawn: every sentence built off one
/// snapshot of the library, because a row that counted one way and answered
/// another is a receipt for something else.
pub struct SheetSpec {
    pub heading: String,
    pub subtitle: String,
    pub question: String,
    pub choices: Vec<ChoiceSpec>,
}

/// One spelling for every question that names the level: two sheets wording
/// the same shelf differently would read as two places. An empty name means
/// the shelf went while the sheet was up.
fn where_line(shelves: &[Shelf], shelf_id: &str) -> String {
    if shelf_id == ALL_SHELF {
        "in your library".to_string()
    } else {
        match shelf::find(shelves, shelf_id).map(|s| s.name.as_str()).unwrap_or("") {
            name if !name.is_empty() => format!("on “{name}”"),
            _ => "on this shelf".to_string(),
        }
    }
}

fn more_waiting(subtitle: String, waiting: usize) -> String {
    if waiting > 0 {
        format!("{subtitle} · {waiting} more waiting")
    } else {
        subtitle
    }
}

/// The name question's own words. `waiting` is the count of questions behind
/// this one — only the questions this sheet answers, because a count that
/// included another shape would promise a batch these answers cannot
/// consume.
///
/// The replace note's highlight count is the one clause the native app
/// cannot keep yet: the reader has no marks store until the engines land, so
/// the count is always zero and the note wears its zero-mark spelling.
pub fn describe(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    ask: &ConflictAsk,
    waiting: usize,
) -> SheetSpec {
    let where_line = where_line(shelves, &ask.arrival.shelf_id);
    let import = ask.arrival.is_import();
    // The apply's own list rather than a re-derived condition, so a row the
    // sheet renders is a row the answer will take.
    let offers = offers_for(rows, folders, ask);
    let existing_name = ask.existing_name.clone();
    let new_name = next_name(rows, shelves, &ask.arrival.shelf_id, &ask.arrival.name);

    let question = if import {
        format!(
            "“{}” is already {where_line}. Add a second book of its own, put a link here \
             instead, or go to the one you have.",
            ask.arrival.name
        )
    } else if offers.contains(&Placement::LinkOnly) {
        format!(
            "A book called “{existing_name}” is already {where_line}, and it is one of the \
             library's own copies. Keep one book, reach the copy from here, or keep both under \
             a new name."
        )
    } else {
        format!(
            "A book called “{existing_name}” is already {where_line}. Keep one book, keep this \
             one instead, or keep both under a new name."
        )
    };
    let go_to_note = format!("Add nothing — go to “{existing_name}” where it already is");
    let new_note = format!("Keeps both, under the next free name — “{new_name}”");
    const LINK_NOTE: &str = "A pointer row, not a copy: tapping it goes to the book where it lives";
    let merge_note = format!(
        "One book — “{existing_name}” stays, and takes this one's shelves, its highlights, \
         and the further place in it"
    );
    let replace_note = format!(
        "“{existing_name}” leaves the library — this one takes its place on every shelf it \
         was on"
    );
    let move_new_note = format!("Keeps both — this one becomes “{new_name}”");
    let link_note = format!(
        "The book you dragged becomes a pointer here — “{existing_name}” stays, the file on \
         disk stays, and nothing is destroyed"
    );

    let choices = offers
        .iter()
        .map(|choice| match choice {
            Placement::Open => ChoiceSpec {
                label: "Already imported",
                note: go_to_note.clone(),
                placement: Placement::Open,
            },
            Placement::KeepBoth if import => ChoiceSpec {
                label: "Add as new",
                note: new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::KeepBoth => ChoiceSpec {
                label: "As new",
                note: move_new_note.clone(),
                placement: Placement::KeepBoth,
            },
            Placement::LinkOnly if import => ChoiceSpec {
                label: "Make link",
                note: LINK_NOTE.to_string(),
                placement: Placement::LinkOnly,
            },
            Placement::LinkOnly => ChoiceSpec {
                label: "Make link",
                note: link_note.clone(),
                placement: Placement::LinkOnly,
            },
            Placement::Merge => ChoiceSpec {
                label: "Merge",
                note: merge_note.clone(),
                placement: Placement::Merge,
            },
            Placement::Replace => ChoiceSpec {
                label: "Replace",
                note: replace_note.clone(),
                placement: Placement::Replace,
            },
        })
        .collect();

    SheetSpec {
        heading: ask.arrival.name.clone(),
        subtitle: more_waiting(format!("Already {where_line}"), waiting),
        question,
        choices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::Origin;
    use library_core::testkit;
    use reader_core::format::Format;

    fn linked_row(id: &str, name: &str, path: &str, n: u32) -> Row {
        let mut book = Book::new(
            id.to_string(),
            testkit::fp_n(n),
            Format::Markdown,
            Origin::Linked { src: path.to_string() },
            0,
        );
        book.title = Some(name.to_string());
        book.title_locked = true;
        Row::Book(book)
    }

    fn stored_row(id: &str, name: &str, src: &str, store: &str, n: u32) -> Row {
        let mut book = Book::new(
            id.to_string(),
            testkit::fp_n(n),
            Format::Markdown,
            Origin::Stored { src: Some(src.to_string()), store: store.to_string() },
            0,
        );
        book.title = Some(name.to_string());
        book.title_locked = true;
        Row::Book(book)
    }

    fn plain_shelf(id: &str, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    fn placing_folder(n: u32) -> WatchedFolder {
        WatchedFolder {
            placed: std::collections::HashSet::from([testkit::fp_n(n)]),
            ..testkit::watched_folder("f1", "/books")
        }
    }

    fn ask(moving: Option<&str>, name: &str, to: &str) -> ConflictAsk {
        let arrival = match moving {
            Some(id) => Arrival::moved(id, name, to, None),
            None => Arrival::folder(name, to),
        };
        ConflictAsk {
            arrival,
            existing_id: "e1".to_string(),
            existing_name: name.to_string(),
        }
    }

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
        };
        let spec = describe(&rows, &shelves, &folders, &ask, 2);
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
        };
        let spec = describe(&rows, &shelves, &folders, &root_ask, 0);
        assert_eq!(spec.subtitle, "Already in your library");
        let gone_ask = ConflictAsk {
            arrival: Arrival::moved("m1", "Dune", "gone-shelf", None),
            existing_id: "e1".to_string(),
            existing_name: "Dune".to_string(),
        };
        let spec = describe(&rows, &shelves, &folders, &gone_ask, 0);
        assert_eq!(spec.subtitle, "Already on this shelf");
    }

    #[test]
    fn the_link_shape_sheet_words_its_own_question() {
        let rows = vec![
            stored_row("e1", "Dune", "/books/dune.md", "/store/e1.md", 9),
            linked_row("m1", "Dune", "/books/dune.md", 7),
        ];
        let shelves = vec![plain_shelf("s1", &["e1"])];
        let folders = vec![placing_folder(7)];
        let ask = ask(Some("m1"), "Dune", "s1");
        let spec = describe(&rows, &shelves, &folders, &ask, 0);
        assert!(spec.question.contains("one of the library's own copies"), "the shape says why replace is withheld");
        assert_eq!(
            spec.choices.iter().map(|c| c.placement).collect::<Vec<_>>(),
            Placement::MOVE_KEEPING_BOTH
        );
        let link = spec.choices.iter().find(|c| c.placement == Placement::LinkOnly).unwrap();
        assert!(link.note.contains("nothing is destroyed"));
    }
}
