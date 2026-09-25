//! The moves a row makes: the gates a gesture passes before it may write,
//! and the hand-move that finishes in place.

use crate::app::Mareader;
use crate::library::conflicts;
use crate::library::departure::{self, RowMove, ShelfDeparture, ShelfSeam};
use crate::library;
use library_core::shelf::{self, ALL_SHELF};

/// A departure run's landing: the rows the copies were asked for, and the
/// gesture that finishes when they come home.
pub(super) struct DepartWork {
    pub(super) converting: Vec<String>,
    pub(super) landing: DepartLand,
}

/// The gesture a copy run finishes: a move resumes with what landed, a rung
/// comes apart once its books are safe, and a removal runs whatever the
/// copies did.
pub(super) enum DepartLand {
    /// A hand-move: the gesture's whole id list and the hand that resumes.
    Move { ids: Vec<String>, hand: RowMove },
    /// The shelf move's landing: the departures read at the answer, the
    /// level they were going to, and the seam a sibling drop named.
    ShelfMove {
        deps: Vec<ShelfDeparture>,
        level: Option<String>,
        seam: Option<ShelfSeam>,
    },
    /// The rung the take-apart question named.
    Rung { id: String },
    /// The removal sheet's own gesture.
    Removal { purge: Vec<String>, shelves: Vec<String> },
}

mod copy;
mod run;
mod shelves;

impl Mareader {
    /// The book half of a move, behind both of its gates: the departure's
    /// copy question first — a read-at-place book leaving the ground that
    /// made it becomes the library's own stored copy, and a copy is a cost
    /// the reader agrees to before anything moves — and then the level's
    /// name screen, where what collides waits on the sheet and what does
    /// not lands now. `departed` names the rows a copy of this very gesture
    /// made: a departure is not a return, so they bind no moved-out log.
    /// True when anything wrote; a gated move writes nothing and waits.
    pub(super) fn gated_seat(
        &mut self,
        books: &[String],
        from: Option<String>,
        to: String,
        index: Option<usize>,
        departed: &[String],
    ) -> bool {
        // A re-order leaves nothing behind — the rows are arriving where they
        // already are — and every other hand-move is screened by the one
        // departure rule, which reads the book's own rung and not the shelf
        // it happens to stand on.
        let arriving_elsewhere = match from.as_deref() {
            Some(from) => from != to,
            None => to != ALL_SHELF,
        };
        if arriving_elsewhere {
            let hand = RowMove::Seat { from: from.clone(), to: to.clone(), index };
            if self.ask_move_copy(books, &to, hand) {
                return false;
            }
        }
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, &to, index, from.as_deref()),
        );
        let ids = conflicts::clean_move_ids(clean);
        let mut wrote = false;
        if !ids.is_empty() {
            wrote |= library::arrange::move_many_to_shelf(
                &mut self.library.shelves,
                &mut self.library.books,
                &ids,
                from.as_deref(),
                &to,
                index,
            );
            // Every stored book the move lands can bind a folder's moved-out
            // log as a return: the address they share is the bind.
            for id in &ids {
                if !departed.contains(id) {
                    wrote |= departure::bind_returned(
                        &self.library.books,
                        &self.library.shelves,
                        &mut self.library.folders,
                        id,
                        &to,
                    );
                }
            }
        }
        self.raise_conflict(asks);
        wrote
    }

    /// The lift out of one shelf, behind the same two gates: to the
    /// library's own floor is a departure for every read-at-place book a
    /// folder placed, and the floor has names of its own to collide with.
    /// No bind on this door — a lift out arrives nowhere a log could name.
    pub(super) fn gated_unfile(&mut self, books: &[String], shelf: &str) -> bool {
        let hand = RowMove::Unfile { shelf: shelf.to_string() };
        if self.ask_move_copy(books, ALL_SHELF, hand) {
            return false;
        }
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, ALL_SHELF, None, Some(shelf)),
        );
        let ids = conflicts::clean_move_ids(clean);
        let wrote = if ids.is_empty() {
            false
        } else {
            library::arrange::unfile_books(&mut self.library.shelves, &ids, shelf)
        };
        self.raise_conflict(asks);
        wrote
    }

    /// A second membership — a filing or an "also show": no copy and no
    /// departure, but the level's name screen rides the arrival all the
    /// same, and a stored book landing on a folder's shelf can be the
    /// file's return.
    pub(super) fn gated_file(&mut self, books: &[String], shelf_id: &str) -> bool {
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, shelf_id, None, None),
        );
        let ids = conflicts::clean_move_ids(clean);
        let mut wrote = false;
        if !ids.is_empty() {
            wrote |= library::arrange::file_many(&mut self.library.shelves, &ids, shelf_id);
            for id in &ids {
                wrote |= departure::bind_returned(
                    &self.library.books,
                    &self.library.shelves,
                    &mut self.library.folders,
                    id,
                    shelf_id,
                );
            }
        }
        self.raise_conflict(asks);
        wrote
    }

    /// One row's own move: off every shelf it was on and onto the one named,
    /// at the slot the drop pointed at. The root has no member list, so a
    /// move there is the lift out of every shelf. The single row rides the
    /// same departure gate as every hand-move — by the time a conflict
    /// answer reaches here the gate's screen is empty by construction, but
    /// an "as new" renames a row a filing converted, and that seat asks its
    /// own question. `departed` is the copy's own answer: a departure binds
    /// no moved-out log.
    pub(super) fn move_row(&mut self, row_id: &str, to: &str, index: Option<usize>, departed: bool) -> bool {
        let one = [row_id.to_string()];
        let hand = RowMove::Row { to: to.to_string(), index };
        if self.ask_move_copy(&one, to, hand) {
            return false;
        }
        shelf::forget_everywhere(&mut self.library.shelves, row_id);
        if to == ALL_SHELF {
            if index.is_some() {
                library::arrange::reorder_root(&mut self.library.books, &one, index);
            }
        } else {
            if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, to) {
                shelf::place(&mut shelf.books, row_id, index);
            }
            if !departed {
                departure::bind_returned(
                    &self.library.books,
                    &self.library.shelves,
                    &mut self.library.folders,
                    row_id,
                    to,
                );
            }
        }
        true
    }
}
