//! Taking a row away: what leaves the ledger, what leaves the disk, and which of
//! the two the reader asked for.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::sheets::Sheet;
use crate::library::departure;
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::book;
use library_core::folder::Tombstone;
use library_core::ledger;
use library_core::shelf;
use library_core::{text as lib_text};

impl Mareader {
    /// The removal, and everything the library holds about the row going
    /// out: the memberships, the links that pointed at it, the tombstone
    /// that keeps a watched folder's rescan from putting the book straight
    /// back, and — for a copy the library owns — the byte in the store.
    pub(in crate::app) fn remove_row(&mut self, id: &str) -> Task<Message> {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return Task::none();
        };
        let name = row.display_name();
        let was_book = row.book().is_some();
        if !self.purge_row(id) {
            return Task::none();
        }
        let line = if was_book {
            format!("Removed “{name}” from the library")
        } else {
            format!("Removed the link “{name}”")
        };
        self.toasts.show(Tone::Info, line, Instant::now());
        self.persist_library()
    }

    /// The removal sheet's own door: a shelf coming off the list that reads
    /// books in place buys their copies first, and a removal with nothing to
    /// copy runs at once.
    pub(in crate::app) fn remove_entries(&mut self, purge: Vec<String>, shelves: Vec<String>) -> Task<Message> {
        let ask = departure::ask_of_removal(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            &purge,
            &shelves,
        );
        match ask {
            Some(ask) => {
                self.sheet = Some(Sheet::Copy { ask });
                Task::none()
            }
            None => {
                if self.remove(purge, shelves) {
                    self.persist_library()
                } else {
                    Task::none()
                }
            }
        }
    }

    /// The removal, whole: the books go out of the library the way one does —
    /// the ledger's tombstone, the swept copy, the memberships, the links —
    /// and the shelves come off the list deepest first, because a shelf
    /// dissolved first is a shelf no sweep reaches. One receipt; the persist
    /// is the caller's, so a removal that rides a copy run is one write with
    /// it.
    pub(in crate::app) fn remove(&mut self, purge: Vec<String>, shelves: Vec<String>) -> bool {
        let removed_books = purge.iter().filter(|id| self.purge_row(id)).count();
        let mut going: Vec<(usize, String)> = shelves
            .into_iter()
            .map(|id| (shelf::ancestors(&self.library.shelves, &id).len(), id))
            .collect();
        going.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        let removed_shelves = going.into_iter().filter(|(_, id)| self.dismantle_shelf(id)).count();
        if removed_books == 0 && removed_shelves == 0 {
            return false;
        }
        let mut parts: Vec<String> = Vec::new();
        if removed_books > 0 {
            parts.push(lib_text::plural(removed_books, "book", "books"));
        }
        if removed_shelves > 0 {
            parts.push(lib_text::plural(removed_shelves, "shelf", "shelves"));
        }
        let line = format!("Removed {} from the library", parts.join(" and "));
        self.toasts.show(Tone::Info, line, Instant::now());
        true
    }

    /// What the menus call: a rung holding books read in place asks first,
    /// and every other shelf comes apart at once, because nothing about it
    /// is a question.
    pub(in crate::app) fn ask_shelf_apart(&mut self, id: &str) -> Task<Message> {
        let ask = departure::ask_of_rung(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            id,
        );
        match ask {
            Some(ask) => {
                self.sheet = Some(Sheet::Copy { ask });
                Task::none()
            }
            None => self.remove_shelf_with(id),
        }
    }

    /// A row leaving the library, receipt aside: the tombstone the ledger
    /// needs, the copy swept when no twin reads the byte, the memberships,
    /// the links that pointed at it. True when the id named a row.
    pub(in crate::app) fn purge_row(&mut self, id: &str) -> bool {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return false;
        };
        let doomed = row.book().cloned();
        if let Some(book) = &doomed {
            // Read the world before writing any of it: the tombstone needs
            // the folder that placed this book and the shelf it was filed
            // on.
            let placed_by = self
                .library
                .folders
                .iter()
                .find(|folder| folder.placed.contains(&book.fp))
                .map(|folder| folder.id.clone());
            let home = placed_by
                .as_deref()
                .and_then(|folder_id| {
                    departure::folder_shelf_of(&self.library.shelves, folder_id, &book.id)
                });
            let entry = Tombstone::of(book, home, now_ms());
            ledger::tombstone(&mut self.library.folders, &entry);
        }
        self.unlist_row(id);
        if let Some(book) = &doomed {
            self.sweep_row_bytes(book);
        }
        true
    }
}
