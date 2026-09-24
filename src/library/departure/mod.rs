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
mod tests;
