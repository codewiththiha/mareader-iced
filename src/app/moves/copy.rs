//! The copies a gesture buys: the ask, the run, and the landing that finishes
//! the move it was for.

use super::{DepartLand, DepartWork};
use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::sheets::Sheet;
use crate::app::walk::Stage;
use crate::library::departure::{self, CopyAsk, CopyWork, RowMove};
use iced::Task;
use library_core::book;
use library_core::shelf;
use library_core::wire::BookFileRequest;
use library_core::{text as lib_text};

impl Mareader {
    /// The gate every hand-move rides: a row that reads in place and is
    /// leaving the ground that made it becomes the library's own stored
    /// copy, and a copy is a question. True means the move waits on the
    /// sheet.
    pub(in crate::app) fn ask_move_copy(&mut self, ids: &[String], to: &str, hand: RowMove) -> bool {
        let converting =
            departure::converting_rows(&self.library.books, &self.library.folders, ids, to);
        let Some(ask) =
            departure::ask_of_rows(&self.library.books, &self.library.folders, &converting, hand)
        else {
            return false;
        };
        self.sheet = Some(Sheet::Copy { ask });
        true
    }

    /// The move door's copies: the answer screens again — the sheet was up
    /// while the library went on living — and the interrupted move resumes
    /// when they come home.
    pub(in crate::app) fn copy_rows(&mut self, ids: Vec<String>, hand: RowMove) -> Task<Message> {
        let converting =
            departure::converting_rows(&self.library.books, &self.library.folders, &ids, hand.to());
        let requests: Vec<BookFileRequest> = converting
            .iter()
            .filter_map(|id| {
                let book = book::find_row(&self.library.books, id)?.book()?;
                Some(BookFileRequest { from: book.path().to_string(), id: id.clone() })
            })
            .collect();
        if requests.is_empty() {
            // Nothing left to copy: the rows the gate named went while the
            // sheet was up, and the gesture resumes without them — a copy
            // that failed costs that book its move and nothing else.
            let rest: Vec<String> =
                ids.into_iter().filter(|id| !converting.contains(id)).collect();
            let moved =
                if rest.is_empty() { false } else { self.resume_move(hand, rest, Vec::new()) };
            self.advance_conflict();
            return if moved { self.persist_library() } else { Task::none() };
        }
        let label = match converting.len() {
            1 => book::find_row(&self.library.books, &converting[0])
                .map(|row| row.display_name())
                .unwrap_or_else(|| "1 book".to_string()),
            n => format!("{n} books"),
        };
        let work = DepartWork { converting, landing: DepartLand::Move { ids, hand } };
        self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) })
    }

    /// The sheet's own "Copy" answer, per door: the copies ride a store
    /// batch of their own — one card for the whole gesture, the way fifty
    /// books leaving their ground are one thing the reader asked for — and
    /// the gesture finishes when they come home.
    pub(in crate::app) fn copy_and_finish(&mut self, ask: CopyAsk) -> Task<Message> {
        match ask.work {
            CopyWork::Rows { ids, hand } => self.copy_rows(ids, hand),
            CopyWork::Shelf { ids, target, seam, .. } => self.copy_shelves(ids, target, seam),
            CopyWork::Rung { id } => {
                // The answer walks the rung again, because the sheet was up
                // while the library went on living: a book that went comes
                // back through the folder's own rescan, not this run.
                let books = departure::books_the_rung_takes(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    &id,
                );
                let label = shelf::find(&self.library.shelves, &id)
                    .map(|rung| rung.name.clone())
                    .unwrap_or_else(|| "shelf".to_string());
                self.begin_depart_run(label, books, DepartLand::Rung { id })
            }
            CopyWork::Removal { purge, shelves } => {
                let books = departure::shelf_books(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    &shelves,
                    &purge,
                );
                let label = lib_text::plural(books.len(), "book", "books");
                self.begin_depart_run(label, books, DepartLand::Removal { purge, shelves })
            }
        }
    }
}
