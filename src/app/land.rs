//! Landing a walk: what the run checked, what it copied, and how the files
//! that were already here take their seats.
use std::sync::Arc;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Origin};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops, rel_under};
use library_core::ledger::{self};
use library_core::scan::FoundFile;
use library_core::shelf::{self, ALL_SHELF};
use library_core::text as lib_text;
use library_core::wire::{BookFileRequest, PathCheck, StoreResult};

use crate::library::conflicts::{self, ConflictAsk};
use crate::platform::{now_ms, store};
use crate::ui::toast::Tone;
use super::Mareader;
use super::copies::{partition_store_results, CopyMap, PendingCopy};
use super::message::Message;
use super::sheets::{Asked, LitNote, Sheet};
use super::walk::{chain_for, found_from_check, write_folder_row, FilesPlan, Stage, WalkPlan};

impl Mareader {
    /// The diff's answer, written to the live lists: relinks first, then
    /// the mints — each wearing the shelf its rung names, minting the whole
    /// chain between the folder's root and the file's own subfolder — then
    /// the rehang the fresh map owes, the row written back whole, and the
    /// news the reader is told.
    #[allow(clippy::too_many_lines)]
    pub(super) fn land_folder_walk(
        &mut self,
        plan: WalkPlan,
        copies: Option<CopyMap>,
    ) -> Task<Message> {
        let WalkPlan {
            mut folder,
            asked,
            continuation,
            root_name,
            planned_root,
            adds,
            relinks,
            asks,
            replacements,
            copy_paths,
            represented,
        } = plan;
        let stamp = now_ms();
        let mut placed = 0usize;
        let mut relinked = 0usize;
        let mut healed = 0usize;
        let mut refused = 0usize;

        for (book_id, to) in relinks {
            if ledger::relink(&mut self.library.books, &book_id, &to) {
                relinked += 1;
            }
        }

        let mut new_shelves: Vec<shelf::Shelf> = Vec::new();
        let mut placements: Vec<(String, String)> = Vec::new();
        let root = folder.root.clone();
        let folder_id = folder.id.clone();

        for (book_id, file) in adds {
            // A file this run owes a copy of is the library's second instance
            // beside the row that already reads the address: never a heal.
            let own_copy = copy_paths.contains(&file.path);
            // A file at an address the library already reads is that book,
            // whatever the two fingerprints say: a migrated row's
            // placeholder identity is healed by the ground it stands on.
            if !own_copy
                && let Some(existing) =
                    book::book_rows_mut(&mut self.library.books).find(|b| b.path() == file.path)
            {
                existing.heal(file.fp);
                folder.mark_placed(file.fp);
                healed += 1;
                continue;
            }
            let origin = match &copies {
                None => Origin::Linked { src: file.path.clone() },
                Some(map) => match map.get(&book_id) {
                    Some((store_path, _)) => Origin::Stored {
                        src: Some(file.path.clone()),
                        store: store_path.clone(),
                    },
                    // The store refused this file; the rest of the batch
                    // still lands, and the failure is counted for the toast.
                    None => {
                        refused += 1;
                        continue;
                    }
                },
            };
            // A file that came back is the file that left: the name the
            // removal logged rides home with it.
            let title = ledger::find_tombstone(&folder, &file.fp).and_then(|s| s.title.clone());
            let mut minted =
                Book::new(book_id.clone(), file.fp, file.admitted_format(), origin, stamp);
            minted.title = title;
            if let Some(map) = &copies {
                // The copy's own measurement becomes the row's identity, so
                // the source's fingerprint stays free for the folder that
                // reads it.
                minted.adopt_measurement(map.get(&book_id).and_then(|(_, m)| *m));
            }
            // A copy-list file becomes a book of its own beside the linked
            // book the tree keeps, and so does an in-place file whose address
            // another row already reads as a STORED copy: `add_book`'s
            // one-row-per-fingerprint rule is right for a walk and wrong for
            // the second instance the library just asked for.
            if own_copy {
                minted.independent = true;
            }
            let beside_its_own_copy = copies.is_none()
                && book::book_rows(&self.library.books).any(|b| {
                    !b.independent && b.fp == file.fp && b.origin.is_store_copy_of(&file.path)
                });
            let placed_id = if own_copy || beside_its_own_copy {
                let id = minted.id.clone();
                self.library.books.push(library_core::book::Row::Book(minted));
                id
            } else {
                book::add_book(&mut self.library.books, minted)
            };
            // The whole chain, not the leaf: importing "1" containing "2"
            // and four books has to produce "1" at the root with them
            // inside.
            let key = folder.shelf_key(&file);
            let shelf_id =
                chain_for(&mut folder, &key, stamp, planned_root.as_deref(), &root, &mut new_shelves);
            placements.push((placed_id, shelf_id));
            folder.mark_placed(file.fp);
            // The landing spends the removal that was holding the file out.
            ledger::restore_deleted(&mut folder, &file.fp);
            placed += 1;
        }

        // The rows a planned tree re-seats: the row follows its rung onto the
        // tree the answer named, which is what makes *merge into it* a move
        // rather than a second copy of every book the folder already had.
        for (row_id, file) in replacements {
            let key = folder.shelf_key(&file);
            let shelf_id = chain_for(
                &mut folder,
                &key,
                stamp,
                planned_root.as_deref(),
                &root,
                &mut new_shelves,
            );
            placements.push((row_id, shelf_id));
        }

        // The shelves the mints reported, added unless already standing: a
        // second walk can name a rung the first minted, and the id is what
        // makes a rung the same rung.
        for minted in new_shelves {
            if !self.library.shelves.iter().any(|each| each.id == minted.id) {
                self.library.shelves.push(minted);
            }
        }
        for (book_id, shelf_id) in &placements {
            if let Some(home) = shelf::find_mut(&mut self.library.shelves, shelf_id) {
                shelf::shelf_add(home, book_id);
            }
        }
        // The disk's shape wins over the tree's own hanging, for every rung
        // no hand has moved.
        for (id, want) in shelf::rehang_moves(&self.library.shelves, &folder_id) {
            if let Some(moved) = shelf::find_mut(&mut self.library.shelves, &id) {
                moved.parent = want;
            }
        }

        folder.scanned_ms = stamp;
        write_folder_row(&mut self.library.folders, folder);
        let persist = self.persist_library();

        // The represented rows are books the reader got back without a copy:
        // they belong in the count the import reports.
        let total = placed + relinked + healed + represented.len();
        match asked {
            Asked::Explicitly if total > 0 => self.toasts.show(
                Tone::Info,
                format!("Imported {} from “{root_name}”", lib_text::plural(total, "book", "books")),
                Instant::now(),
            ),
            Asked::OnFocus if placed > 0 => self.toasts.show(
                Tone::Info,
                format!("{} in “{root_name}”", lib_text::plural(placed, "new book", "new books")),
                Instant::now(),
            ),
            _ => {}
        }
        if refused > 0 {
            self.toasts.show(
                Tone::Error,
                format!(
                    "Could not copy {}; the rest of the folder landed",
                    lib_text::plural(refused, "file", "files")
                ),
                Instant::now(),
            );
        }
        // The merge's questions are raised over the landed tree, never
        // beside ghosts. A covered walk's nothing-new note waits on them:
        // a question on screen is answered before the note its ground saw.
        let asks_empty = asks.is_empty();
        if !asks.is_empty() {
            self.raise_conflict(asks);
        }
        // The covered walk's answer to its own pick: nothing new is the
        // note, its close the shelf's light; something new goes to where it
        // stands, beside it.
        if asked == Asked::Explicitly
            && let Some(continuation) = continuation
        {
            if total == 0 && asks_empty {
                self.sheet = Some(Sheet::AlreadyImported {
                    note: LitNote {
                        shelf_id: continuation.shelf_id.clone(),
                        name: continuation.name.clone(),
                        kind: conflicts::NoteKind::NothingNew,
                    },
                });
            } else if total > 0 {
                return Task::batch([persist, self.reveal_shelf(&continuation.shelf_id)]);
            }
        }
        persist
    }

    // ── The loose-file run ──────────────────────────────────────────────

    /// The measurement arrived: heal and mark what it measured, answer each
    /// file by the ground it stands on, and queue the rest for the store —
    /// loose files land as the library's own copies, because no folder
    /// rescans them and a linked row no ledger answers for is a row no rule
    /// can keep honest.
    #[allow(clippy::too_many_lines)]
    pub(super) fn files_checked(&mut self, task: u64, checks: Vec<PathCheck>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let Stage::Measuring { target } = &mut self.runs[ix].stage else {
            return Task::none();
        };
        let target = target.take();

        let mut changed = false;
        for check in &checks {
            if !book::apply_check(&mut self.library.books, check).is_empty() {
                changed = true;
            }
        }

        let mut found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
        if found.is_empty() {
            self.runs.remove(ix);
            self.toasts.show(
                Tone::Error,
                "None of those files could be opened.",
                Instant::now(),
            );
            return if changed { self.persist_library() } else { Task::none() };
        }
        // The files a folder's own log already answers for: the reader has
        // these books, so the drop lights them up rather than landing copies
        // beside them.
        let represented = self.take_represented(None, &mut found);

        let stamp = now_ms();
        let shelf_id = target.clone().unwrap_or_else(|| ALL_SHELF.to_string());
        let mut restored = 0usize;

        // A file a read-at-place tree holds is that folder's business
        // first: a row already reading the address raises the folder's
        // two-answer question, and a removal the tree logged comes back as
        // the tree's own linked book.
        let mut covered_asks: Vec<ConflictAsk> = Vec::new();
        found.retain(|file| {
            let covering = self
                .library
                .folders
                .iter()
                .filter(|folder| folder.mode().reads_in_place())
                .find(|folder| rel_under(&file.path, &folder.root).is_some())
                .map(|folder| folder.id.clone());
            let Some(folder_id) = covering else {
                return true;
            };
            let held_at_address = book::book_rows(&self.library.books)
                .find(|each| each.path() == file.path)
                .map(|each| each.id.clone());
            if let Some(row_id) = held_at_address {
                let existing_name = book::find_row(&self.library.books, &row_id)
                    .map(|row| row.display_name())
                    .unwrap_or_else(|| file.rel.clone());
                covered_asks.push(ConflictAsk::covered(
                    Arrival::import(file.clone(), shelf_id.clone(), None),
                    row_id,
                    existing_name,
                    folder_id,
                ));
                return false;
            }
            let stone = folder_ops::find(&self.library.folders, &folder_id)
                .and_then(|folder| ledger::find_tombstone(folder, &file.fp).cloned());
            let placed_by_folder = folder_ops::find(&self.library.folders, &folder_id)
                .is_some_and(|folder| folder.placed.contains(&file.fp));
            if stone.is_some() || placed_by_folder {
                self.restore_covered(file, &folder_id, stone.as_ref(), stamp);
                restored += 1;
                changed = true;
                return false;
            }
            true
        });

        // Content the library already holds is content the reader already
        // has, wherever it is filed: the two-answer question asks whether
        // the drop meant a second instance or the one they have. A row
        // whose address died is no answer — the copy lands beside it.
        let mut held_asks: Vec<ConflictAsk> = Vec::new();
        found.retain(|file| {
            let Some(held) = ledger::existing_for(&self.library.books, file.fp) else {
                return true;
            };
            if held.missing {
                return true;
            }
            let existing_name = book::find_row(&self.library.books, &held.row_id)
                .map(|row| row.display_name())
                .unwrap_or_else(|| file.rel.clone());
            held_asks.push(ConflictAsk::already_have(
                Arrival::import(file.clone(), shelf_id.clone(), None),
                held.row_id.clone(),
                existing_name,
            ));
            false
        });

        // Every file whose name the level already holds is a question
        // rather than a placement; a twin on another shelf is not one.
        let arrivals: Vec<Arrival> = found
            .iter()
            .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
            .collect();
        let (clean, name_asks) =
            conflicts::screen(&self.library.books, &self.library.shelves, arrivals);

        let mut pending: Vec<PendingCopy> = Vec::new();
        for arrival in &clean {
            let Some(file) = arrival.file.clone() else {
                continue;
            };
            // A removal any folder logged against this content is spent by
            // the explicit ask: the name it remembered rides onto the copy.
            let title = self.lift_stone_for(&file.fp);
            pending.push(PendingCopy {
                book_id: library_core::id::next_id(stamp),
                file,
                title,
            });
        }
        let mut asks = covered_asks;
        asks.extend(held_asks);
        asks.extend(name_asks);

        if pending.is_empty() {
            self.runs.remove(ix);
            if restored > 0 {
                self.toasts.show(
                    Tone::Info,
                    format!("{} came back", lib_text::plural(restored, "book", "books")),
                    Instant::now(),
                );
            }
            self.raise_conflict(asks);
            // A drop the library already had in every file it named is a light
            // on the first book a log answered for: the copy's own landing
            // never runs, so the light rides this door instead.
            let light = match represented.first() {
                Some(book_id) => self.reveal_book(book_id),
                None => Task::none(),
            };
            return if changed {
                Task::batch([self.persist_library(), light])
            } else {
                light
            };
        }

        let requests: Vec<BookFileRequest> = pending
            .iter()
            .map(|item| BookFileRequest { from: item.file.path.clone(), id: item.book_id.clone() })
            .collect();
        let sink = Arc::clone(&self.runs[ix].sink);
        let task_name = task.to_string();
        self.runs[ix].stage = Stage::Copying {
            plan: Box::new(FilesPlan {
                target,
                pending,
                restored,
                asks,
                represented,
                settle: None,
                index: None,
            }),
        };
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::FilesCopied(task, results),
        )
    }

    /// The store's answer for the loose files: land the copies that came
    /// home, each wearing its own measurement, on the level the pick named.
    pub(super) fn files_copied(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        let Stage::Copying { plan } = run.stage else {
            return Task::none();
        };
        let FilesPlan { target, pending, restored, asks, represented, settle, index } = *plan;
        let (copies, failure) = partition_store_results(results);
        let stamp = now_ms();

        let mut landed = 0usize;
        let mut placements: Vec<String> = Vec::new();
        for item in pending {
            let Some((store_path, measured)) = copies.get(&item.book_id) else {
                continue;
            };
            // A row already reading the source address makes this copy its
            // own book: independent, with its own marks and place.
            let independent =
                book::book_rows(&self.library.books).any(|each| each.path() == item.file.path);
            let mut minted = Book::new(
                item.book_id,
                item.file.fp,
                item.file.admitted_format(),
                Origin::Stored { src: Some(item.file.path.clone()), store: store_path.clone() },
                stamp,
            );
            minted.title = item.title;
            minted.independent = independent;
            minted.adopt_measurement(*measured);
            let placed_id = minted.id.clone();
            self.library.books.push(library_core::book::Row::Book(minted));
            placements.push(placed_id);
            landed += 1;
        }
        if let Some(target) = &target
            && let Some(home) = shelf::find_mut(&mut self.library.shelves, target)
        {
            for placed_id in &placements {
                shelf::place(&mut home.books, placed_id, index);
            }
        }
        // The answer's own ledger write, spent only when the copy it
        // waited for actually landed.
        if landed > 0
            && let Some((folder_id, fp)) = settle
        {
            self.settle_ledger(Some(&folder_id), fp);
        }

        let added = landed + restored;
        if added > 0 {
            let persist = self.persist_library();
            if let Some(error) = failure {
                self.toasts.show(Tone::Error, error, Instant::now());
            } else {
                let line = if target.is_some() {
                    format!("Added {} to this shelf", lib_text::plural(added, "book", "books"))
                } else {
                    format!("Added {}", lib_text::plural(added, "book", "books"))
                };
                self.toasts.show(Tone::Info, line, Instant::now());
            }
            self.raise_conflict(asks);
            let light = match represented.first() {
                Some(book_id) => self.reveal_book(book_id),
                None => Task::none(),
            };
            return Task::batch([persist, light]);
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        // A drop that landed nothing new but answered for a book the reader
        // already had is a light on that book rather than a receipt of zero:
        // the log remembered it, so the reader is taken to it.
        let light = match represented.first() {
            Some(book_id) => self.reveal_book(book_id),
            None => Task::none(),
        };
        self.raise_conflict(asks);
        light
    }

    /// The folder the level on screen belongs to, when it is a watched
    /// folder's shelf — the add menu's in-folder doors answer for it.
    pub(super) fn standing_folder_id(&self) -> Option<String> {
        if self.shelf == ALL_SHELF {
            return None;
        }
        shelf::find(&self.library.shelves, &self.shelf)
            .and_then(|shelf| shelf.kind.folder_id().map(str::to_string))
    }
}
