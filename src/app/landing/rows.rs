//! Landing a file as a row: the store copy, the ledger's settle, and the sweep
//! that takes a row out.

use crate::app::Mareader;
use crate::app::copies::PendingCopy;
use crate::app::message::Message;
use crate::app::walk::{FilesPlan, FsRun, Stage};
use crate::library::conflicts::{self, ConflictAsk};
use crate::platform::{now_ms, progress, store};
use iced::Task;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::conflict::Placement;
use library_core::folder::{self as folder_ops};
use library_core::ledger;
use library_core::paths;
use library_core::scan::FoundFile;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::BookFileRequest;
use std::sync::Arc;

impl Mareader {
    /// Whether the row the question found reads the very file that is
    /// arriving.
    pub(in crate::app) fn is_the_same_file(&self, existing_id: &str, path: &str) -> bool {
        book::find_by_id(&self.library.books, existing_id).is_some_and(|each| each.path() == path)
    }

    /// Record the placement and spend the removal that was holding the
    /// file out — the two ledger writes a walk makes for its files, made
    /// here because an answer landed this one after a walk raised the
    /// question.
    pub(in crate::app) fn settle_ledger(&mut self, folder_id: Option<&str>, fp: Fingerprint) {
        let Some(folder_id) = folder_id else {
            return;
        };
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &fp);
            folder.mark_placed(fp);
        }
    }

    /// The row's own byte, once no row reads it — the twin rule's other
    /// half: two rows of one file share an address, and a sweep that forgot
    /// the twin would delete the file the survivor reads. A linked book's
    /// bytes are the reader's and are never touched.
    pub(in crate::app) fn sweep_row_bytes(&mut self, book: &Book) {
        let in_use = book::book_rows(&self.library.books)
            .any(|each| each.id != book.id && each.path() == book.path());
        if in_use {
            return;
        }
        if book.origin.is_stored()
            && let Err(error) = store::delete_stored(book.path())
        {
            // The row is gone either way; a byte the host will not release
            // is the log's business, not a second question.
            eprintln!("[library] could not sweep {}: {error}", book.path());
        }
    }

    /// The sheet already withholds *keep both* from a twin; this is the
    /// write side of the same rule, because apply-to-all can carry an
    /// answer to a question whose sheet never offered it.
    pub(in crate::app) fn withhold_keep_both_from_a_twin(
        &self,
        ask: &ConflictAsk,
        answer: Placement,
    ) -> Placement {
        if answer != Placement::KeepBoth {
            return answer;
        }
        if ask.kind.reads_in_place()
            && let Some(file) = ask.arrival.file.as_ref()
            && self.is_the_same_file(&ask.existing_id, &file.path)
        {
            Placement::Merge
        } else {
            answer
        }
    }

    /// Remove one row, everywhere it is filed, and sweep the byte only it
    /// read — and no tombstone: the conflict sheet's dissolving row is one
    /// whose content stays in the library through the row on the other side
    /// of the question, so a rescan that re-found the file would resolve to
    /// that row, and a tombstone for a fingerprint the library still holds
    /// is noise in the folder's restore menu until the next scan prunes it.
    pub(in crate::app) fn drop_row(&mut self, id: &str) -> bool {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return false;
        };
        let doomed = row.book().cloned();
        self.unlist_row(id);
        if let Some(book) = &doomed {
            self.sweep_row_bytes(book);
        }
        true
    }

    /// The answered file's landing: a read-at-place folder's answer links
    /// the file where the gesture meant it, and a copying folder's answer
    /// rides the single-file copy, settling the folder's ledger when that
    /// copy comes home.
    pub(in crate::app) fn land_answer_file(
        &mut self,
        ask: &ConflictAsk,
        file: FoundFile,
        name: Option<String>,
        index: Option<usize>,
    ) -> Task<Message> {
        let (mode, folder_id) = match &ask.kind {
            conflicts::AskKind::FolderMerge { mode, folder_id } => {
                (*mode, Some(folder_id.as_str()))
            }
            _ => return Task::none(),
        };
        let shelf_id = ask.arrival.shelf_id.clone();
        if mode.reads_in_place() {
            self.land_file(&file, name, &shelf_id, index);
            self.settle_ledger(folder_id, file.fp);
            // The web's cover backfill waits on the engines, documented
            // where the covers land.
            return self.persist_library();
        }
        let settle = folder_id.map(|id| (id.to_string(), file.fp));
        self.land_stored_copy(file, name, shelf_id, index, settle)
    }

    /// The whole of a removal that is NOT a sweep: no tombstone, no store
    /// byte. One spelling, because the half of it that is easy to forget is
    /// the expensive one.
    pub(in crate::app) fn unlist_row(&mut self, id: &str) {
        book::remove_row(&mut self.library.books, id);
        book::drop_dangling_links(&mut self.library.books);
        shelf::forget_everywhere(&mut self.library.shelves, id);
    }

    /// A linked row at the file's own address: what a read-at-place folder
    /// lands for an answered file, on the shelf and in the slot the gesture
    /// meant, wearing the name the answer minted when it minted one. The
    /// web's kept reading data riding a returning file waits on the marks
    /// store.
    pub(in crate::app) fn land_file(
        &mut self,
        file: &FoundFile,
        name: Option<String>,
        shelf_id: &str,
        index: Option<usize>,
    ) -> String {
        let stamp = now_ms();
        // A row already reading the source address makes this row its own
        // book: independent, with its own marks and place.
        let independent =
            book::book_rows(&self.library.books).any(|each| each.path() == file.path);
        let mut minted = Book::new(
            library_core::id::next_id(stamp),
            file.fp,
            file.admitted_format(),
            Origin::Linked { src: file.path.clone() },
            stamp,
        );
        minted.title = name;
        minted.independent = independent;
        let placed = minted.id.clone();
        self.library.books.push(book::Row::Book(minted));
        // The root has no member list, so the placement write is skipped.
        if shelf_id != ALL_SHELF
            && let Some(home) = shelf::find_mut(&mut self.library.shelves, shelf_id)
        {
            shelf::place(&mut home.books, &placed, index);
        }
        placed
    }

    /// One file's ride through the store: a copy, a measurement, a landing
    /// — the answers that promise a library copy of their own ride this, so
    /// a copy that fails leaves the shelf untouched and the ledger
    /// unmarked.
    pub(in crate::app) fn land_stored_copy(
        &mut self,
        file: FoundFile,
        name: Option<String>,
        shelf_id: String,
        index: Option<usize>,
        settle: Option<(String, Fingerprint)>,
    ) -> Task<Message> {
        let stamp = now_ms();
        let book_id = library_core::id::next_id(stamp);
        let label = paths::file_name(&file.path);
        let request = BookFileRequest { from: file.path.clone(), id: book_id.clone() };
        let plan = FilesPlan {
            target: (shelf_id != ALL_SHELF).then_some(shelf_id),
            pending: vec![PendingCopy { book_id, file, title: name }],
            restored: 0,
            asks: Vec::new(),
            // An answer's own landing represents nothing: the question was
            // about this one file.
            represented: Vec::new(),
            settle,
            index,
        };
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink: Arc::clone(&sink),
            rx,
            latest: None,
            stage: Stage::Copying { plan: Box::new(plan) },
        });
        let task_name = task.to_string();
        let requests = vec![request];
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::FilesCopied(task, results),
        )
    }

    /// Spend the removal that was holding this content out of any folder's
    /// walk, and answer with the name it remembered. The match is the
    /// fingerprint, not the address: a file removed from a folder, moved
    /// across the disk and dropped back into the library is the same file
    /// the log was written for.
    pub(in crate::app) fn lift_stone_for(&mut self, fp: &Fingerprint) -> Option<String> {
        let owner = self
            .library
            .folders
            .iter()
            .find(|folder| ledger::find_tombstone(folder, fp).is_some())
            .map(|folder| folder.id.clone())?;
        let folder = folder_ops::find_mut(&mut self.library.folders, &owner)?;
        ledger::restore_deleted(folder, fp).and_then(|stone| stone.title)
    }

    /// The files a folder's moved-out log already answers for: a log bound to
    /// a LIVING row says the file's book is still here — the row a shelf move
    /// or a link-making answer left standing for it — so an import of that
    /// file succeeds by lighting the row up rather than landing a second book
    /// beside it. `scope` narrows the search to one folder's own log; the
    /// loose run asks every folder at once.
    pub(in crate::app) fn take_represented(&self, scope: Option<&str>, found: &mut Vec<FoundFile>) -> Vec<String> {
        let mut represented = Vec::new();
        found.retain(|file| {
            let row_id = self
                .library
                .folders
                .iter()
                .filter(|folder| scope.is_none_or(|id| folder.id == id))
                .find_map(|folder| {
                    ledger::find_tombstone(folder, &file.fp)
                        .and_then(|entry| entry.returned_row.clone())
                });
            let Some(row_id) = row_id else {
                return true;
            };
            // A log bound to a row that went is a log whose file is free to
            // land again.
            if book::find_row(&self.library.books, &row_id).is_none() {
                return true;
            }
            represented.push(row_id);
            false
        });
        represented
    }
}
