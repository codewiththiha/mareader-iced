//! The departure: a read-at-place book leaving the ground that made it
//! becomes the library's own stored copy on the way out — its bytes its own,
//! the ORIGINAL fingerprint left free for the folder's log to keep.
//!
//! This is the pure half the web service ran against its signals: the screen
//! that says which rows a move takes into the store, the ask that names the
//! cost, the moved-out log a departure writes and a return binds. The store
//! batch between the ask and the landing is the app's, and so is the queue of
//! doors.
//!
//! Two halves the native app does not carry yet, both documented where the web
//! does them: the covers a departure prunes and backfills wait on the engines,
//! and the highlights that follow the address wait on the reader's marks store.

pub mod leaving;
pub mod log;
pub mod returning;
pub mod shelves;

pub use leaving::{ask_of_rows, converting_rows, converts_on_move};
pub use log::{bind_returned, folder_shelf_of, write_moved_stones};
pub use returning::{books_the_rung_takes, landing_level, shelf_books};
pub use shelves::{
    ask_of_removal, ask_of_rung, ask_of_shelf, free_name, shelf_departures, ShelfDeparture,
};

use library_core::shelf::ALL_SHELF;

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
    #[allow(dead_code)] // ported ahead: the conflict sheet raises it
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
    #[allow(dead_code)] // ported ahead: the move's door reads it
    WithoutCopies,
    #[allow(dead_code)] // ported ahead: the web's third door
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashSet};
    use crate::library::departure::kit::{family_state, reading_folder, tree, tree_rows};
    use crate::library::departure::returning::return_path;
    use crate::library::departure::shelves::{ask_of_removal, ask_of_rung, ask_of_shelf};
    use library_core::book::Row;
    use library_core::folder::WatchedFolder;
    use library_core::shelf::Shelf;
    use library_core::testkit;

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
}

#[cfg(test)]
mod kit;
