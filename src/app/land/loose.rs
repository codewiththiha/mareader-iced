//! The loose-file run: the files picked without a folder are measured, then
//! copied into the store and landed as the library's own rows.

use crate::app::Mareader;
use crate::app::copies::{PendingCopy, partition_store_results};
use crate::app::message::Message;
use crate::app::walk::{FilesPlan, Stage, found_from_check};
use crate::library::conflicts::{self, ConflictAsk};
use crate::platform::{now_ms, store};
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Origin};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops, rel_under};
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::{BookFileRequest, PathCheck, StoreResult};
use library_core::{text as lib_text};
use std::sync::Arc;

impl Mareader {
    /// The measurement arrived: heal and mark what it measured, answer each
    /// file by the ground it stands on, and queue the rest for the store —
    /// loose files land as the library's own copies, because no folder
    /// rescans them and a linked row no ledger answers for is a row no rule
    /// can keep honest.
    #[allow(clippy::too_many_lines)]
    pub(in crate::app) fn files_checked(&mut self, task: u64, checks: Vec<PathCheck>) -> Task<Message> {
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
    pub(in crate::app) fn files_copied(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
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
}
