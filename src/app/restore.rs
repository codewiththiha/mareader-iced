//! The folders' remembered ground: the books a removal left behind, and the
//! restore that gives them back.
use std::sync::Arc;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::folder::{self as folder_ops, FolderOpts, Tombstone};
use library_core::ledger::{self, Recovered};
use library_core::scan::FoundFile;
use library_core::shelf::{self};
use library_core::wire::{BookFileRequest, PathCheck, StoreResult};

use crate::platform::{fs, now_ms, progress, store};
use crate::ui::toast::Tone;
use super::Mareader;
use super::copies::{partition_store_results, Stored};
use super::message::Message;
use super::walk::{found_from_check, FsRun, Stage};

impl Mareader {
    /// The folder's book comes back the way a folder walk brings it back —
    /// a linked book at the file's address, wearing the name the log
    /// remembered, on the log's shelf when it still stands — and the
    /// landing spends the log.
    pub(super) fn restore_covered(
        &mut self,
        file: &FoundFile,
        folder_id: &str,
        stone: Option<&Tombstone>,
        stamp: u64,
    ) {
        let mut minted = Book::new(
            library_core::id::next_id(stamp),
            file.fp,
            file.admitted_format(),
            Origin::Linked { src: file.path.clone() },
            stamp,
        );
        minted.title = stone.and_then(|entry| entry.title.clone());
        let placed_id = book::add_book(&mut self.library.books, minted);
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &file.fp);
            folder.mark_placed(file.fp);
        }
        // The log's shelf when it stands, else the folder's root rung, else
        // the file stays unfiled — the order the walk's own restore keeps.
        let home = stone
            .and_then(|entry| entry.shelf_id.clone())
            .or_else(|| {
                folder_ops::find(&self.library.folders, folder_id)
                    .and_then(|folder| folder.shelf_map.get("").cloned())
            })
            .filter(|id| shelf::find(&self.library.shelves, id).is_some());
        if let Some(home) = home
            && let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &home)
        {
            shelf::shelf_add(shelf, &placed_id);
        }
    }

    /// What this folder could give the reader back: the ledger's
    /// recoverables, pure and synchronous — the menu opens on a click and
    /// answers from the last walk's log, not from a directory tree.
    fn restore_candidates(&self) -> Vec<Recovered> {
        let Some(folder_id) = self.standing_folder_id() else {
            return Vec::new();
        };
        let Some(folder) = folder_ops::find(&self.library.folders, &folder_id) else {
            return Vec::new();
        };
        let index = ledger::index_by_fp(&self.library.books);
        ledger::recoverables(folder, &index, &self.library.shelves)
    }

    /// The gone check behind the add menu's Restore section: a removed
    /// book's file may have left the disk since the walk logged it, and a
    /// row that promises a file that is not there would land an error
    /// where the menu could have shown the truth. Dispatched when the
    /// panel opens; the rows answer disabled as the check arrives.
    pub(super) fn check_restore_paths(&mut self) -> Task<Message> {
        self.restore_gone.clear();
        let addresses: Vec<String> = self
            .restore_candidates()
            .into_iter()
            .filter_map(|item| match item {
                Recovered::Deleted(entry) => Some(entry.last_path),
                Recovered::Moved { .. } => None,
            })
            .collect();
        if addresses.is_empty() {
            return Task::none();
        }
        Task::perform(async move { fs::check_paths(&addresses) }, Message::RestoreChecked)
    }

    /// The Restore section's removed row: measure the one file the log
    /// remembered, then land it back the way the folder holds its books.
    /// A restore re-measures before it promises — the file on disk, not
    /// the log, has the last word.
    pub(super) fn restore_deleted(&mut self, folder_id: String, fp: Fingerprint) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        let Some((opts, stone)) = folder_ops::find(&self.library.folders, &folder_id).and_then(
            |folder| {
                ledger::find_tombstone(folder, &fp)
                    .cloned()
                    .map(|stone| (folder.opts.clone(), stone))
            },
        ) else {
            return Task::none();
        };
        let address = stone.last_path.clone();
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label: stone.label(),
            sink,
            rx,
            latest: None,
            stage: Stage::Restoring { folder_id, stone: Box::new(stone), opts },
        });
        Task::perform(
            async move { fs::check_paths(std::slice::from_ref(&address)) },
            move |checks| Message::RestoreMeasured(task, checks),
        )
    }

    /// The restore's measurement answered: a file that is not there any
    /// more is an error the reader hears; a file that is there lands
    /// linked when the tree reads at place, and goes through the store
    /// when it does not.
    pub(super) fn restore_measured(&mut self, task: u64, checks: Vec<PathCheck>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let mut run = self.runs.remove(ix);
        let Stage::Restoring { folder_id, stone, opts } = &run.stage else {
            return Task::none();
        };
        let (folder_id, stone, opts) = (folder_id.clone(), (**stone).clone(), opts.clone());
        let Some(found) = checks.first().and_then(found_from_check) else {
            self.toasts.show(
                Tone::Error,
                format!("{} is not there any more.", stone.label()),
                Instant::now(),
            );
            return Task::none();
        };
        let book_id = library_core::id::next_id(now_ms());
        if opts.mode().reads_in_place() {
            return self.land_restore(&folder_id, stone, &opts, found, book_id, None);
        }
        let requests = vec![BookFileRequest { from: found.path.clone(), id: book_id.clone() }];
        let sink = Arc::clone(&run.sink);
        let task_name = task.to_string();
        run.stage = Stage::RestoreCopying {
            folder_id,
            stone: Box::new(stone),
            opts,
            found: Box::new(found),
            book_id,
        };
        self.runs.push(run);
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::RestoreCopied(task, results),
        )
    }

    /// The restore's copy answered: land the stored book with its own
    /// measurement, or tell the reader the copy did not come home.
    pub(super) fn restore_copied(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        let Stage::RestoreCopying { folder_id, stone, opts, found, book_id } = run.stage else {
            return Task::none();
        };
        let (copies, failure) = partition_store_results(results);
        let measured = copies.get(&book_id).cloned();
        if let Some(error) =
            failure.or_else(|| measured.is_none().then(|| "The copy did not land.".to_string()))
        {
            self.toasts.show(Tone::Error, error, Instant::now());
            return Task::none();
        }
        self.land_restore(&folder_id, *stone, &opts, *found, book_id, measured)
    }

    /// The restore's landing: the book comes back wearing the name the log
    /// remembered and the measurement its own bytes answered with, the log
    /// is spent, the file is marked placed, and the book is filed on the
    /// shelf the log remembered when that shelf still stands — else on the
    /// folder's root rung, else nowhere. A file that changed since the
    /// removal spends the changed address's log too, so the next walk does
    /// not give the book back a second time.
    fn land_restore(
        &mut self,
        folder_id: &str,
        stone: Tombstone,
        opts: &FolderOpts,
        found: FoundFile,
        book_id: String,
        measured: Option<Stored>,
    ) -> Task<Message> {
        let stamp = now_ms();
        let origin = match &measured {
            Some((store_path, _)) => {
                Origin::Stored { src: Some(found.path.clone()), store: store_path.clone() }
            }
            None => Origin::Linked { src: found.path.clone() },
        };
        let mut minted = Book::new(book_id, found.fp, found.admitted_format(), origin, stamp);
        minted.title = stone.title.clone();
        if opts.mode().copies_files() {
            minted.adopt_measurement(measured.as_ref().and_then(|(_, measure)| *measure));
        }
        let label = stone.label();
        let placed_id = book::add_book(&mut self.library.books, minted);
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &stone.fp);
            folder.mark_placed(found.fp);
            if found.fp != stone.fp {
                folder.ignored.retain(|entry| entry.fp != found.fp);
            }
        }
        let home = stone
            .shelf_id
            .clone()
            .or_else(|| {
                folder_ops::find(&self.library.folders, folder_id)
                    .and_then(|folder| folder.shelf_map.get("").cloned())
            })
            .filter(|id| shelf::find(&self.library.shelves, id).is_some());
        if let Some(home) = home
            && let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &home)
        {
            shelf::shelf_add(shelf, &placed_id);
        }
        self.toasts.show(Tone::Info, format!("“{label}” came back"), Instant::now());
        self.persist_library()
    }
}
