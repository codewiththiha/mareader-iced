//! The moves: the gesture's asks, the gate every answer passes, the
//! departure run and the landing that finishes it.
use std::collections::HashSet;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self};
use library_core::folder::{self as folder_ops};
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::text as lib_text;
use library_core::wire::{BookFileRequest, StoreResult};

use crate::library::conflicts::{self};
use crate::library::departure::{
    self, CopyAsk, CopyWork, ReturnPath, RowMove, ShelfDeparture, ShelfSeam,
};
use crate::library::{self};
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use super::Mareader;
use super::copies::partition_store_results;
use super::message::Message;
use super::sheets::Sheet;
use super::walk::{chain_for, flatten_rungs, member_rungs, page_into, rel_of, Claim, Stage};

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

    /// The gate every hand-move rides: a row that reads in place and is
    /// leaving the ground that made it becomes the library's own stored
    /// copy, and a copy is a question. True means the move waits on the
    /// sheet.
    fn ask_move_copy(&mut self, ids: &[String], to: &str, hand: RowMove) -> bool {
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

    /// The sheet's own "Copy" answer, per door: the copies ride a store
    /// batch of their own — one card for the whole gesture, the way fifty
    /// books leaving their ground are one thing the reader asked for — and
    /// the gesture finishes when they come home.
    pub(super) fn copy_and_finish(&mut self, ask: CopyAsk) -> Task<Message> {
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

    /// The move door's copies: the answer screens again — the sheet was up
    /// while the library went on living — and the interrupted move resumes
    /// when they come home.
    fn copy_rows(&mut self, ids: Vec<String>, hand: RowMove) -> Task<Message> {
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

    /// The copies a door bought, as one store batch: the books to convert,
    /// and the landing that finishes the gesture when they come home. A door
    /// whose books all went while the sheet was up lands at once — a rung
    /// with nothing left to copy is still a take-apart, and a removal still
    /// removes.
    pub(super) fn begin_depart_run(
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

    /// The landing the reader bought: the rungs leave the tree with the copy
    /// they paid for, the other folders' shelves that rode along take the
    /// hand's mark, the folder lets the departed zone go, and the copies
    /// wear the level's next free names — then the gesture's own seam or
    /// nest seats what landed. A shelf whose books the store refused entire
    /// stays where it was, with its own sentence.
    fn land_shelf_moves(
        &mut self,
        deps: Vec<ShelfDeparture>,
        level: Option<String>,
        seam: Option<ShelfSeam>,
        departed: &[String],
    ) -> bool {
        let mut landed: Vec<String> = Vec::new();
        for dep in &deps {
            if dep.books.is_empty() || dep.books.iter().any(|id| departed.contains(id)) {
                landed.push(dep.id.clone());
            } else {
                self.toasts.show(
                    Tone::Info,
                    format!(
                        "“{}” stayed where it was — the library could not copy its books.",
                        dep.name
                    ),
                    Instant::now(),
                );
            }
        }
        if landed.is_empty() {
            return false;
        }
        let going: Vec<&ShelfDeparture> =
            deps.iter().filter(|dep| landed.contains(&dep.id)).collect();
        let mut promised: HashSet<String> =
            shelf::children_of(&self.library.shelves, level.as_deref())
                .into_iter()
                .map(|s| s.name.clone())
                .collect();
        for dep in &going {
            for rung in &dep.rungs {
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, rung) {
                    one.kind = library_core::shelf::ShelfKind::Departed;
                    one.manual_parent = false;
                }
            }
            for id in &dep.subtree {
                if dep.rungs.contains(id) {
                    continue;
                }
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, id)
                    && one.is_folder()
                {
                    one.manual_parent = true;
                }
            }
            if let Some(one) = shelf::find_mut(&mut self.library.shelves, &dep.id) {
                one.name = departure::free_name(&dep.name, &mut promised);
            }
        }
        for dep in &going {
            let Some(folder) = folder_ops::find_mut(&mut self.library.folders, &dep.folder_id)
            else {
                continue;
            };
            folder.shelf_map.retain(|key, shelf_id| {
                !folder_ops::key_in_zone(key, &dep.rel) && !dep.rungs.contains(shelf_id)
            });
        }
        let ids: Vec<String> = going.iter().map(|dep| dep.id.clone()).collect();
        // The seats ride the very gestures the screen wraps — clean by
        // construction now, because a Departed rung owes no second copy.
        if let Some(seam) = &seam {
            library::arrange::reorder_shelves_to_anchor(
                &mut self.library.shelves,
                &ids,
                &seam.anchor_id,
                seam.after,
            );
        } else {
            for id in &ids {
                library::arrange::nest_shelf(&mut self.library.shelves, id, level.as_deref());
            }
        }
        true
    }

    /// No copies: every mover that has a way home takes it, and a mover that
    /// has none stays where the tree put it. The fold is the import's own
    /// `reclaim_rung`, and the reseat rides the very `nest_shelf` the
    /// gesture did. The reveal that lights the first seated shelf waits on
    /// the reveal's own light.
    pub(super) fn take_them_home(&mut self, returns: &[(String, ReturnPath)]) -> bool {
        let mut moved = false;
        for (shelf_id, path) in returns {
            moved |= match path {
                ReturnPath::Reclaim { tree, gone, rel } => {
                    self.reclaim_rung(tree, gone, rel, shelf_id).is_some()
                }
                ReturnPath::Reseat { seat } => library::arrange::nest_shelf(
                    &mut self.library.shelves,
                    shelf_id,
                    seat.as_deref(),
                ),
            };
        }
        moved
    }

    /// Put a displaced member back on the rung its directory names and fold
    /// the folder that was reading it into the tree that contains it: one
    /// ground, one reader from here on. Three writes, in the order that
    /// keeps them honest; the answer is the shelf the member sat on.
    ///
    /// A tree a walk holds is the one fold to refuse, because that walk's
    /// clone of the ledger lands after this write and drops it. Persists
    /// nothing itself: the caller ends its own transaction.
    fn reclaim_rung(
        &mut self,
        tree_id: &str,
        gone_id: &str,
        rel: &str,
        shelf_id: &str,
    ) -> Option<String> {
        let now = now_ms();
        let mut minted: Vec<Shelf> = Vec::new();
        let mut tree = folder_ops::find(&self.library.folders, tree_id)?.clone();
        let gone = folder_ops::find(&self.library.folders, gone_id)?.clone();
        // Read before anything is written: the answer is about the shelves
        // standing, not the ones this move mints.
        let rungs = member_rungs(&self.library.shelves, gone_id, rel);
        // A shelf that went while the sheet was up is an answer with nothing
        // to move; so is a tree a walk holds, whose ledger clone lands after
        // this write.
        let foreign_walk = !matches!(self.root_claim(&tree.root), Claim::Free);
        if !rungs.iter().any(|(_, id)| id == shelf_id) || foreign_walk {
            return None;
        }
        let root = tree.root.clone();
        // Ground the shape keeps on one shelf has no rung for the member's
        // directory to become: its books come onto the rung the ground
        // answers for and its own shelves go, so an adoption cannot cut a
        // nested rung into an import that asked for none.
        let seat = if tree.cuts(rel) {
            let parent = chain_for(
                &mut tree,
                folder_ops::parent_key(rel).unwrap_or(""),
                now,
                None,
                &root,
                &mut minted,
            );
            for (key, id) in &rungs {
                tree.shelf_map.insert(key.clone(), id.clone());
            }
            page_into(&mut self.library.shelves, minted);
            let nestable = shelf::can_nest(&self.library.shelves, shelf_id, &parent);
            for (key, id) in &rungs {
                let Some(one) = shelf::find_mut(&mut self.library.shelves, id) else {
                    continue;
                };
                one.kind = library_core::shelf::ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(key),
                };
                if id != shelf_id {
                    continue;
                }
                if nestable {
                    one.parent = Some(parent.clone());
                    one.manual_parent = false;
                } else {
                    one.manual_parent = true;
                }
            }
            shelf_id.to_string()
        } else {
            // A pointer to a shelf that went is no seat, and this tree's map
            // is not the one the run pruned: the mint is asked for the key
            // the map has no standing rung under.
            let standing = tree
                .shelf_map
                .get("")
                .filter(|id| shelf::find(&self.library.shelves, id).is_some())
                .cloned();
            let seat = match standing {
                Some(seat) => seat,
                None => {
                    tree.shelf_map.remove("");
                    chain_for(&mut tree, "", now, None, &root, &mut minted)
                }
            };
            page_into(&mut self.library.shelves, minted);
            flatten_rungs(&mut self.library.shelves, gone_id, &seat);
            seat
        };
        // The answer the folded row carried for its own root becomes the
        // rung it becomes: the row that answer was written on is the one
        // this fold retires. A tree that cuts no rungs has one answer for
        // the whole of its ground — the reader's own about its root — and
        // the adoption does not second-guess it.
        if tree.cuts(rel) {
            tree.set_tracking(rel, gone.opts.watch);
        }
        tree.placed.extend(gone.placed.iter().copied());
        for stone in gone.ignored.iter() {
            if !tree.is_ignored(&stone.fp) {
                tree.ignored.push(stone.clone());
            }
        }
        tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);
        self.library.folders.retain(|f| f.id != gone_id);
        match self.library.folders.iter().position(|f| f.id == tree_id) {
            Some(at) => self.library.folders[at] = tree,
            None => self.library.folders.push(tree),
        }
        Some(seat)
    }

    /// The store's answer for a departure: the copies become the library's
    /// own — the moved-out log first, because the tombstone wears the book's
    /// ORIGINAL fingerprint — and the interrupted gesture resumes with what
    /// came home.
    pub(super) fn departing_done(&mut self, work: DepartWork, results: Vec<StoreResult>) -> Task<Message> {
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
    fn depart_landing(
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
    fn resume_move(&mut self, hand: RowMove, ids: Vec<String>, departed: Vec<String>) -> bool {
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
