//! The departure: a read-at-place book leaving the ground that made it
//! becomes the library's own stored copy on the way out — its bytes its own,
//! the ORIGINAL fingerprint left free for the folder's log to keep — and the
//! copy is a cost the reader agrees to. One sheet for every door in, so no
//! path stores a file in silence.
//!
//! This is the pure half the web service ran against its signals: the screen
//! that says which rows a move takes into the store, the ask that names the
//! cost, the moved-out log a departure writes and a return binds. The store
//! batch between the ask and the landing is the app's, and so is the queue
//! of doors — the shelf's, the rung's and the removal's arrive with the
//! systems that own them; the move's is here.
//!
//! Two halves the native app does not carry yet, both documented where the
//! web does them: the covers a departure prunes and backfills wait on the
//! engines, and the highlights that follow the address wait on the reader's
//! marks store.

use std::collections::HashSet;

use library_core::book::{book_rows, duplicate_title, find_row, Book, Origin, Row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone, WatchedFolder};
use library_core::ledger::tombstone;
use library_core::paths::dir_label;
use library_core::shelf::{self, Shelf, ALL_SHELF};
use library_core::text::plural;

/// The gesture a copy question interrupted, so the sheet's own answer can
/// finish it: the same rows land, and the copies land marked — a departure
/// is not a return, so a copied book binds no folder's moved-out log, and a
/// book the move already took off its seat stays off it.
#[derive(Clone, PartialEq, Debug)]
pub enum RowMove {
    /// A drag or a bulk filing: the rows land on `to`, lifted off `from`
    /// where the two differ.
    Seat { from: Option<String>, to: String, index: Option<usize> },
    /// One row's own move, the form the conflict sheet rides as well as a
    /// drag of a single book. The single-row door arrives with the conflict
    /// sheet; the gesture's resume already answers it.
    #[allow(dead_code)]
    Row { to: String, index: Option<usize> },
    /// Out of every shelf the row was on, to the library's own top level.
    Unfile { shelf: String },
    /// The replace answer's own seat: the row lands in the displaced row's
    /// slot and takes over every OTHER shelf the displaced one was on. The
    /// copy question a replace asks of its own arrival rides this hand.
    Replaced { to: String, index: Option<usize>, inherited: Vec<String> },
}

impl RowMove {
    /// Where the rows are going: the gate screens the gesture against it,
    /// and the answer screens again, because the sheet was up while the
    /// library went on living.
    pub fn to(&self) -> &str {
        match self {
            RowMove::Seat { to, .. } | RowMove::Row { to, .. } | RowMove::Replaced { to, .. } => to,
            RowMove::Unfile { .. } => ALL_SHELF,
        }
    }
}

/// The reader's answer: buy the copies, finish the gesture without them, or
/// leave everything as it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyAnswer {
    Copy,
    /// Finish the gesture but leave the books where they are: the shelf's
    /// door answers it by taking its read-at-place books home, the removal's
    /// by removing. The move's door has one way through, so these wait on
    /// the systems that own them — the sheet's handler already reads them.
    #[allow(dead_code)]
    WithoutCopies,
    #[allow(dead_code)]
    Cancel,
}

/// One answer's button. Built at the raise rather than at the click, because
/// the sheet's own wording is a fact about the gesture and not something the
/// view recomputes.
#[derive(Clone, PartialEq, Debug)]
pub struct CopyOption {
    pub label: String,
    pub title: String,
    pub answer: CopyAnswer,
    pub primary: bool,
}

/// What the reader is in the middle of, and everything the answer needs to
/// finish it. One enum rather than a flag per door: the sheet names the
/// action, and the answer resumes THAT gesture rather than a second one
/// built from the same facts.
#[derive(Clone, PartialEq, Debug)]
pub enum CopyWork {
    /// Books on the move, and the way back into the drag or filing that held
    /// them.
    Rows { ids: Vec<String>, hand: RowMove },
    /// Shelves off the seat their folder's tree names, the level they were
    /// dropped on, the seam a sibling drop named, and the ones with a way
    /// home instead of a copy.
    Shelf {
        ids: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
        returns: Vec<(String, ReturnPath)>,
    },
    /// A rung of a read-at-place tree coming apart: its own books become the
    /// library's first, and the level goes once they are safe.
    Rung { id: String },
    /// The removal sheet's own gesture: the books going out of the library
    /// and the shelves coming off the list. The web's third fact — the
    /// answer about the reader's marks — waits on the marks store; there is
    /// no reading data to keep or drop yet.
    Removal { purge: Vec<String>, shelves: Vec<String> },
}

/// The question, ready for the sheet: what it is about, what it costs, and
/// the answers.
#[derive(Clone, PartialEq, Debug)]
pub struct CopyAsk {
    /// The action the reader is in the middle of, in their own words.
    pub action: String,
    /// The shelf or the books it is about.
    pub subject: String,
    /// The cost, in one or two lines.
    pub lines: Vec<String>,
    pub options: Vec<CopyOption>,
    pub work: CopyWork,
}

/// The promise every copy's sheet makes: the bytes come IN, nothing goes
/// out.
pub const UNTOUCHED: &str = "The folder on disk is untouched.";

fn copying(label: &str) -> CopyOption {
    CopyOption {
        label: label.to_string(),
        title: "Copy the books read in place into the library, then finish".to_string(),
        answer: CopyAnswer::Copy,
        primary: true,
    }
}

/// The action's own wording: the count belongs to the subject, and
/// "1 Take shelf apart" is not a sentence. `text::plural` counts because a
/// count is what a subject is for; this one refuses it for the same reason,
/// and the two are not one helper.
fn doing(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// The rows a move to `to` takes into the store, in the order the gesture
/// held them.
pub fn converting_rows(
    rows: &[Row],
    folders: &[WatchedFolder],
    ids: &[String],
    to: &str,
) -> Vec<String> {
    ids.iter().filter(|id| converts_on_move(rows, folders, id, to)).cloned().collect()
}

/// The three negatives are as load-bearing as the positive: a STORED book is
/// already the library's own and simply moves, and a book no in-place folder
/// placed is nobody's departure.
pub fn converts_on_move(rows: &[Row], folders: &[WatchedFolder], row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = find_row(rows, row_id)
        .and_then(|row| row.book())
        .filter(|book| matches!(book.origin, Origin::Linked { .. }))
        .map(|book| (book.fp, book.path().to_string()))
    else {
        return false;
    };
    // Only a folder that placed this content owes a departure; a deleted
    // rung answers `None` and so never matches the destination.
    let placing: Vec<&WatchedFolder> = folders
        .iter()
        .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
        .collect();
    if placing.is_empty() {
        return false;
    }
    to == ALL_SHELF || !placing.iter().any(|f| f.rungs_for(&path).0 == Some(to))
}

/// A book move's ask: the rows the gate screened read in place, and the
/// ground they are leaving. `None` when nothing converts — the gesture then
/// runs as it always did.
pub fn ask_of_rows(
    rows: &[Row],
    folders: &[WatchedFolder],
    ids: &[String],
    hand: RowMove,
) -> Option<CopyAsk> {
    let books = ids.len();
    let folder = ground_of(rows, folders, ids)?;
    Some(CopyAsk {
        action: "Move books".to_string(),
        subject: format!("{} from “{folder}”", plural(books, "book", "books")),
        lines: vec![
            plural(
                books,
                "It is read in place; moving it out stores a copy.",
                "They are read in place; moving them out stores copies.",
            ),
            UNTOUCHED.to_string(),
        ],
        options: vec![copying("Copy and move")],
        work: CopyWork::Rows { ids: ids.to_vec(), hand },
    })
}

/// The folder whose ground these books leave: the first that placed one of
/// them, and the one the sheet names.
fn ground_of(rows: &[Row], folders: &[WatchedFolder], ids: &[String]) -> Option<String> {
    ids.iter().find_map(|id| {
        let fp = find_row(rows, id).and_then(|row| row.book()).map(|b| b.fp)?;
        let folder =
            folders.iter().find(|f| f.mode().reads_in_place() && f.placed.contains(&fp))?;
        Some(dir_label(&folder.root))
    })
}

/// The subtree is every shelf below the one the hand named — it rides with
/// the copy the way a directory's tree rides with the directory — and the
/// rungs are the folder's OWN shelves inside that subtree, which go free of
/// the folder's map.
pub fn departing_sets(
    shelves: &[Shelf],
    folder_id: &str,
    top_id: &str,
) -> (HashSet<String>, HashSet<String>) {
    let root = [top_id.to_string()];
    let subtree: HashSet<String> =
        std::iter::once(top_id.to_string()).chain(shelf::subtree_ids(shelves, &root)).collect();
    let rungs: HashSet<String> = subtree
        .iter()
        .filter(|id| {
            shelf::find(shelves, id).is_some_and(|s| s.kind.folder_id() == Some(folder_id))
        })
        .cloned()
        .collect();
    (subtree, rungs)
}

/// The linked books the folder placed whose own rung is one of the departing
/// ones, and who are members of the departing subtree. The two conditions
/// each rule out a real shape: a book whose rung stands OUTSIDE the subtree,
/// and a book the folder placed but no longer holds.
pub fn departing_book_ids(
    rows: &[Row],
    shelves: &[Shelf],
    folder: &WatchedFolder,
    rungs: &HashSet<String>,
    subtree: &HashSet<String>,
) -> Vec<String> {
    book_rows(rows)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && folder.placed.contains(&b.fp))
        .filter(|b| folder.rungs_for(b.path()).0.is_some_and(|rung| rungs.contains(rung)))
        .filter(|b| shelf::containing(shelves, &b.id).iter().any(|s| subtree.contains(&s.id)))
        .map(|b| b.id.clone())
        .collect()
}

/// A filing has no seam and appends; a sibling drop names the row and the
/// side of it. A value rather than a boolean at the ask: the seam rides the
/// question and the landing commits the very reorder the gesture meant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShelfSeam {
    pub anchor_id: String,
    pub after: bool,
}

/// Where a mover's way home leads, when it has one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ReturnPath {
    /// A displaced root shelf whose ground a family tree covers at a free
    /// rung goes home by the fold, which hangs it on the rung its directory
    /// names and folds the folder that was reading it into the tree's
    /// ledger.
    Reclaim { tree: String, gone: String, rel: String },
    /// Any other off-seat rung goes home by the reseat: back under the shelf
    /// its directory names.
    Reseat { seat: Option<String> },
}

/// The seam's anchor answers with the level that holds IT, a filing answers
/// with its target, and the root is the level that is not a shelf.
pub fn landing_level(
    shelves: &[Shelf],
    target: &Option<String>,
    seam: Option<&ShelfSeam>,
) -> Option<String> {
    match seam {
        Some(seam) => shelf::find(shelves, &seam.anchor_id).and_then(|s| s.parent.clone()),
        None => target.clone(),
    }
}

/// A rung of an in-place tree whose root covers the mover's ground directory
/// — the mover's own tree included, whose rungs are its first family. A
/// read-at-place shelf lives on the seat its directory stands on, so a move
/// inside the tree it belongs to can answer with the seat instead of a copy.
pub fn target_is_family(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    target: Option<&str>,
    ground: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Some(one) = shelf::find(shelves, target) else {
        return false;
    };
    let Some(folder_id) = one.kind.folder_id() else {
        return false;
    };
    folders.iter().any(|f| {
        f.id == folder_id
            && f.mode().reads_in_place()
            && folder_ops::rel_under(ground, &f.root).is_some()
    })
}

/// Two shapes, and which one a shelf owes is a fact about where its folder
/// stands. The folder's ROOT shelf whose ground a family tree covers at a
/// free rung goes home by the fold: `reclaim_rung`, which hangs the shelf on
/// the rung its directory names and folds the folder that was reading it
/// into the tree's ledger.
pub fn return_path(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Option<ReturnPath> {
    let one = shelf::find(shelves, shelf_id)?;
    let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
        return None;
    };
    let folder = folders
        .iter()
        .find(|f| &f.id == folder_id && f.mode().reads_in_place())?;
    let key = rel.clone().unwrap_or_default();
    if key.is_empty()
        && let Some((tree, tree_rel)) = shelf::family_for(folders, shelves, &folder.root)
    {
        return Some(ReturnPath::Reclaim {
            tree,
            gone: folder.id.clone(),
            rel: tree_rel,
        });
    }
    let seat = folder_ops::parent_key(&key)
        .and_then(|rung| folder.shelf_map.get(rung))
        .cloned();
    (one.parent != seat).then_some(ReturnPath::Reseat { seat })
}

/// The books read in place that a rung takes with it when its ground goes:
/// the folder placed them, their own file answers to this rung, and they
/// stand inside it. A book answering to a level that stays — a seat the tree
/// still names — is not one of them, and neither is a book the library
/// already stores.
pub fn books_the_rung_takes(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Vec<String> {
    let Some(rung) = shelf::find(shelves, shelf_id) else {
        return Vec::new();
    };
    let Some(folder) = rung.kind.folder_id().and_then(|id| folders.iter().find(|f| f.id == id))
    else {
        return Vec::new();
    };
    if !folder.mode().reads_in_place() {
        return Vec::new();
    }
    let (subtree, _) = departing_sets(shelves, &folder.id, shelf_id);
    let going: HashSet<String> = std::iter::once(shelf_id.to_string()).collect();
    departing_book_ids(rows, shelves, folder, &going, &subtree)
}

/// Every book the named shelves take with them, and none the removal is
/// taking anyway: a book going out of the library is not a book to copy
/// first.
pub fn shelf_books(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    going: &[String],
    purge: &[String],
) -> Vec<String> {
    let mut taking: Vec<String> = Vec::new();
    for id in going {
        for book in books_the_rung_takes(rows, shelves, folders, id) {
            if !purge.contains(&book) && !taking.contains(&book) {
                taking.push(book);
            }
        }
    }
    taking
}

/// One departing shelf's whole fact, read the moment the answer lands: the
/// rung it is, the books it owes, the rungs and the subtree that go free of
/// the folder with it.
#[derive(Clone, Debug)]
pub struct ShelfDeparture {
    pub id: String,
    pub name: String,
    pub folder_id: String,
    pub rel: String,
    pub rungs: HashSet<String>,
    pub subtree: HashSet<String>,
    pub books: Vec<String>,
}

/// The departures a shelf move owes, in the gesture's own order. The rule is
/// asked again per shelf, because the sheet was up while the library went on
/// living: a shelf that no longer owes a departure — or that cannot nest
/// where the gesture was going — is skipped silently.
pub fn shelf_departures(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    departing: &[String],
    level: Option<&str>,
) -> Vec<ShelfDeparture> {
    let mut out: Vec<ShelfDeparture> = Vec::new();
    for id in departing {
        let Some(one) = shelf::find(shelves, id) else {
            continue;
        };
        let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
            continue;
        };
        let Some(folder) = folders.iter().find(|f| &f.id == folder_id) else {
            continue;
        };
        if !shelf::departs_on_move(shelves, folders, id, level) {
            continue;
        }
        if let Some(parent) = level
            && !shelf::can_nest(shelves, id, parent)
        {
            continue;
        }
        let (subtree, rungs) = departing_sets(shelves, folder_id, id);
        let books = departing_book_ids(rows, shelves, folder, &rungs, &subtree);
        out.push(ShelfDeparture {
            id: id.clone(),
            name: one.name.clone(),
            folder_id: folder_id.clone(),
            rel: rel.clone().unwrap_or_default(),
            rungs,
            subtree,
            books,
        });
    }
    out
}

/// A copy takes the level's next free name, so the folder's own name stays
/// free for the original.
pub fn free_name(name: &str, promised: &mut HashSet<String>) -> String {
    let free = duplicate_title(name, promised);
    promised.insert(free.clone());
    free
}

/// The copy names as the sheet lists them.
fn listed(names: &[String]) -> String {
    names.iter().map(|name| format!("“{name}”")).collect::<Vec<_>>().join(", ")
}

/// A shelf move's ask: one spelling for the three hand-moves, because a
/// drag, a bulk filing and a sibling reorder are one rule and one question.
/// The copies wear the level's next free names, and a drop inside the
/// mover's own family offers the way home instead of a copy. `None` when no
/// departing shelf is a folder's rung at all — the gesture then is nobody's
/// departure and lands clean.
pub fn ask_of_shelf(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) -> Option<CopyAsk> {
    let level = landing_level(shelves, &target, seam.as_ref());
    let mut promised: HashSet<String> = shelf::children_of(shelves, level.as_deref())
        .into_iter()
        .map(|s| s.name.clone())
        .collect();
    let mut names: Vec<String> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    let mut returns: Vec<(String, ReturnPath)> = Vec::new();
    // The first folder that placed one of the books names the sheet.
    let mut folder = String::new();
    let mut count = 0usize;
    for id in &departing {
        let Some(one) = shelf::find(shelves, id) else {
            continue;
        };
        let Some(folder_id) = one.kind.folder_id() else {
            continue;
        };
        let Some(placing) = folders.iter().find(|f| f.id == folder_id) else {
            continue;
        };
        let (subtree, rungs) = departing_sets(shelves, folder_id, id);
        count += departing_book_ids(rows, shelves, placing, &rungs, &subtree).len();
        if folder.is_empty() {
            folder = dir_label(&placing.root);
        }
        // Offered only for a drop inside the mover's FAMILY: anywhere else
        // the copy is the only honest answer, because there is no tree to
        // put the shelf back into.
        let ground = folder_ops::dir_of_rung(&placing.root, one.kind.rung());
        if target_is_family(shelves, folders, level.as_deref(), &ground)
            && let Some(path) = return_path(shelves, folders, id)
        {
            returns.push((id.clone(), path));
        }
        copies.push(free_name(&one.name, &mut promised));
        names.push(one.name.clone());
    }
    if names.is_empty() {
        return None;
    }
    let one = names.len() == 1;
    let cost = plural(count, "book", "books");
    let subject = match (one, count) {
        (true, 0) => format!("“{}” — nothing read in place", names[0]),
        (true, _) => format!("“{}” — {cost} from “{folder}”", names[0]),
        (false, 0) => plural(names.len(), "shelf", "shelves"),
        (false, _) => format!(
            "{} — {cost} read in place",
            plural(names.len(), "shelf", "shelves")
        ),
    };
    let lines = match (one, count) {
        (true, 0) => vec![
            "Nothing on it is read in place, so no file is copied.".to_string(),
            UNTOUCHED.to_string(),
        ],
        (true, _) => vec![
            format!("Moving it out stores its {cost}; the copy lands as “{}”.", copies[0]),
            UNTOUCHED.to_string(),
        ],
        (false, 0) => vec![
            "Nothing on them is read in place, so no file is copied.".to_string(),
            UNTOUCHED.to_string(),
        ],
        (false, _) => vec![
            format!(
                "Moving them out stores their {cost}; the copies land as {}.",
                listed(&copies)
            ),
            UNTOUCHED.to_string(),
        ],
    };
    let mut options = vec![copying("Copy and move")];
    if !returns.is_empty() {
        options.push(CopyOption {
            label: if returns.len() == 1 {
                "Put it back in its place".to_string()
            } else {
                "Put them back in their places".to_string()
            },
            title: "Return each shelf to the place its folder names; nothing is copied"
                .to_string(),
            answer: CopyAnswer::WithoutCopies,
            primary: false,
        });
    }
    Some(CopyAsk {
        action: doing(names.len(), "Move shelf", "Move shelves"),
        subject,
        lines,
        options,
        work: CopyWork::Shelf { ids: departing, target, seam, returns },
    })
}

/// A rung of a read-at-place tree coming apart: the level's own books leave
/// the ground that made them, so they become the library's own before it
/// goes. `None` when nothing on it is a question — an empty rung, a reader's
/// own shelf, a level the library already stores.
pub fn ask_of_rung(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Option<CopyAsk> {
    let books = books_the_rung_takes(rows, shelves, folders, shelf_id);
    if books.is_empty() {
        return None;
    }
    let folder = ground_of(rows, folders, &books)?;
    let rung = shelf::find(shelves, shelf_id)?;
    let home = match rung
        .kind
        .folder_id()
        .and_then(|folder_id| shelf::rung_above(shelves, folder_id, rung.kind.rung()))
        .and_then(|seat| shelf::find(shelves, &seat))
    {
        Some(seat) => format!("“{}”", seat.name),
        None => "the library's top level".to_string(),
    };
    let cost = plural(books.len(), "book", "books");
    let line = if books.len() == 1 {
        format!("The book read in place here becomes a copy and comes up to {home}.")
    } else {
        format!(
            "The {} books read in place here become copies and come up to {home}.",
            books.len()
        )
    };
    Some(CopyAsk {
        action: "Take shelf apart".to_string(),
        subject: format!("“{}” — {cost} from “{folder}”", rung.name),
        lines: vec![line, UNTOUCHED.to_string()],
        options: vec![copying("Copy and take apart")],
        work: CopyWork::Rung { id: shelf_id.to_string() },
    })
}

/// A shelf coming off the list with books read in place on it: the same
/// copy, and the one removal that changes nothing — the folder's next
/// import makes the level again.
pub fn ask_of_removal(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    purge: &[String],
    going: &[String],
) -> Option<CopyAsk> {
    let taking = shelf_books(rows, shelves, folders, going, purge);
    if taking.is_empty() {
        return None;
    }
    let folder = ground_of(rows, folders, &taking)?;
    let names: Vec<String> =
        going.iter().filter_map(|id| shelf::find(shelves, id).map(|s| s.name.clone())).collect();
    let one = names.len() == 1;
    let again = if one {
        format!("Or let “{folder}” make it again.")
    } else {
        "Or let the folders make them again.".to_string()
    };
    let label = if one {
        format!("Let “{folder}” make it again")
    } else {
        "Let the folders make them again".to_string()
    };
    let kept = plural(
        taking.len(),
        "Read in place, so removing it stores a copy.",
        "Read in place, so removing them stores copies.",
    );
    Some(CopyAsk {
        action: doing(names.len(), "Remove shelf", "Remove shelves"),
        subject: plural(names.len(), "shelf", "shelves"),
        lines: vec![kept, again],
        options: vec![
            copying("Copy and remove"),
            CopyOption {
                label,
                title: "Take the shelf off the list; its books read in place where they are"
                    .to_string(),
                answer: CopyAnswer::WithoutCopies,
                primary: false,
            },
        ],
        work: CopyWork::Removal { purge: purge.to_vec(), shelves: going.to_vec() },
    })
}

/// The shelf of a folder that holds a book: one answer rather than every
/// answer, because a removed book comes back to ONE shelf.
pub fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|s| s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}

/// Every folder that placed a fingerprint records that the book left as the
/// library's own copy rather than died. `returned_row` names the row the
/// file is represented by, for the answers that dissolve a linked row into a
/// book the library already holds.
///
/// Persists nothing itself: every caller ends its own transaction with a
/// persist, and this log is one write inside it.
pub fn write_moved_stones(
    folders: &mut [WatchedFolder],
    shelves: &[Shelf],
    book: &Book,
    returned_row: Option<&str>,
    now: u64,
) {
    let home = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .and_then(|f| folder_shelf_of(shelves, &f.id, &book.id));
    // Spelling all eight fields here would be a second place a new
    // `Tombstone` field has to be remembered.
    let entry = Tombstone {
        moved: true,
        returned_row: returned_row.map(str::to_string),
        ..Tombstone::of(book, home, now)
    };
    tombstone(folders, &entry);
}

/// The bind is by ADDRESS, and the address is the one thing a copy cannot
/// change: the log's fp is the file's and the row's is its own copy's stamp,
/// so the fingerprints can never meet again — but the log remembers where the
/// file stood (`last_path`) and the row remembers where its bytes came from
/// (`origin.source()`), and two books called "Dune" in one folder left two
/// logs from two addresses. A row with no address to name — a legacy copy —
/// falls back to the name, and only while it is still wearing its pending
/// placeholder. True when the bind wrote.
pub fn bind_returned(
    rows: &[Row],
    shelves: &[Shelf],
    folders: &mut [WatchedFolder],
    row_id: &str,
    shelf_id: &str,
) -> bool {
    if shelf_id == ALL_SHELF {
        return false;
    }
    let Some((name, src, measured)) = find_row(rows, row_id).and_then(|row| {
        let book = row.book().filter(|b| b.origin.is_stored())?;
        Some((row.display_name(), book.origin.source().map(str::to_string), !book.fp_pending))
    }) else {
        return false;
    };
    let Some(folder_id) = shelf::find(shelves, shelf_id).and_then(|s| s.kind.folder_id()) else {
        return false;
    };
    let Some(folder) = folder_ops::find_mut(folders, folder_id) else {
        return false;
    };
    let is_the_one = |entry: &Tombstone| {
        if !entry.moved {
            return false;
        }
        match src.as_deref() {
            Some(src) => src == entry.last_path,
            None => !measured && same_name(&entry.label(), &name),
        }
    };
    if let Some(entry) = folder.ignored.iter_mut().find(|entry| is_the_one(entry))
        && entry.returned_row.as_deref() != Some(row_id)
    {
        entry.returned_row = Some(row_id.to_string());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::find_book_mut;
    use library_core::testkit;
    use reader_core::format::Format;
    use std::collections::{BTreeMap, HashSet};

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
}
