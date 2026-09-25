//! The departure run: the store work asked for, the answers that come back,
//! and the landing each gesture gets.

use super::{DepartLand, DepartWork};
use crate::app::Mareader;
use crate::app::copies::partition_store_results;
use crate::app::message::Message;
use crate::app::walk::Stage;
use crate::library::departure::{self, RowMove};
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::book;
use library_core::wire::{BookFileRequest, StoreResult};

impl Mareader {
    /// The copies a door bought, as one store batch: the books to convert,
    /// and the landing that finishes the gesture when they come home. A door
    /// whose books all went while the sheet was up lands at once — a rung
    /// with nothing left to copy is still a take-apart, and a removal still
    /// removes.
    pub(in crate::app) fn begin_depart_run(
        &mut self,
        label: String,
        converting: Vec<String>,
        landing: DepartLand,
    ) -> Task<Message> {
        let requests: Vec<BookFileRequest> = converting
            .iter()
            .filter_map(|id| {
                let book = book::find_row(&self.library.books, id)?.book()?;
                Some(BookFileRequest { from: book.path().to_string(), id: id.clone() })
            })
            .collect();
        if requests.is_empty() {
            let failed = if converting.is_empty() { Vec::new() } else { converting };
            let wrote = self.depart_landing(landing, Vec::new(), failed);
            self.advance_conflict();
            return if wrote { self.persist_library() } else { Task::none() };
        }
        let work = DepartWork { converting, landing };
        self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) })
    }

    /// The store's answer for a departure: the copies become the library's
    /// own — the moved-out log first, because the tombstone wears the book's
    /// ORIGINAL fingerprint — and the interrupted gesture resumes with what
    /// came home.
    pub(in crate::app) fn departing_done(&mut self, work: DepartWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let now = now_ms();
        let mut departed: Vec<String> = Vec::new();
        for id in &work.converting {
            let Some((store, measured)) = copies.get(id) else { continue };
            let Some(book) = book::find_by_id(&self.library.books, id) else { continue };
            let path = book.path().to_string();
            departure::write_moved_stones(
                &mut self.library.folders,
                &self.library.shelves,
                book,
                None,
                now,
            );
            if let Some(book) = book::find_book_mut(&mut self.library.books, id) {
                book.become_stored(&path, store.clone(), *measured);
            }
            departed.push(id.clone());
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        let failed: Vec<String> = work
            .converting
            .iter()
            .filter(|id| !departed.contains(*id))
            .cloned()
            .collect();
        let any_copies = !departed.is_empty();
        let moved = self.depart_landing(work.landing, departed, failed);
        self.advance_conflict();
        if moved || any_copies {
            return self.persist_library();
        }
        Task::none()
    }

    /// The gesture a copy run finishes. A move resumes with what landed — a
    /// copy that failed costs that book its move and nothing else: it stays
    /// where it was, and every screen downstream sees the books as what they
    /// are about to be — except a replace, which seats its row whatever the
    /// copy did: a failed copy re-asks at the seat's own gate. A rung comes
    /// apart only once every book it owed is safe: a book the store refused
    /// leaves the shelf standing, so the reader can ask again rather than
    /// lose the ground the rest of the rung answers to. Shelves land as the
    /// take-out wrote them: a shelf whose books the store refused entire
    /// stays where it was. A removal runs whatever the copies did: the sheet
    /// promised the removal, and the copies were the books' own way out of
    /// it.
    pub(in crate::app) fn depart_landing(
        &mut self,
        landing: DepartLand,
        departed: Vec<String>,
        failed: Vec<String>,
    ) -> bool {
        match landing {
            DepartLand::Move { ids, hand } => {
                let resume_ids: Vec<String> = match &hand {
                    RowMove::Replaced { .. } => ids.clone(),
                    _ => ids.iter().filter(|id| !failed.contains(*id)).cloned().collect(),
                };
                if resume_ids.is_empty() {
                    return false;
                }
                self.resume_move(hand, resume_ids, departed)
            }
            DepartLand::ShelfMove { deps, level, seam } => {
                self.land_shelf_moves(deps, level, seam, &departed)
            }
            DepartLand::Rung { id } => {
                if !failed.is_empty() {
                    return false;
                }
                self.dismantle_shelf(&id)
            }
            DepartLand::Removal { purge, shelves } => self.remove(purge, shelves),
        }
    }

    /// The gesture a copy question interrupted, finished: the same rows land
    /// through the same doors — a seat re-runs its own gate and screen,
    /// which the copies that came home pass by construction — and the copies
    /// land marked: a departure is not a return, so a copied book binds no
    /// folder's moved-out log.
    pub(in crate::app) fn resume_move(&mut self, hand: RowMove, ids: Vec<String>, departed: Vec<String>) -> bool {
        match hand {
            RowMove::Seat { from, to, index } => self.gated_seat(&ids, from, to, index, &departed),
            RowMove::Row { to, index } => {
                let gone = !departed.is_empty();
                let mut wrote = false;
                for id in &ids {
                    wrote |= self.move_row(id, &to, index, gone);
                }
                wrote
            }
            RowMove::Replaced { to, index, inherited } => {
                let gone = !departed.is_empty();
                let mut wrote = false;
                for id in &ids {
                    self.seat_replace(id, &to, index, &inherited, gone);
                    wrote = true;
                }
                wrote
            }
            RowMove::Unfile { shelf } => self.gated_unfile(&ids, &shelf),
        }
    }
}
