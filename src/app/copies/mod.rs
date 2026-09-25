//! The copy runs: an unbound folder import, the duplicate queue, and the store
//! batch each one rides.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::walk::{FsRun, RootPlan, Stage};
use crate::library::duplicate::{BookCopy, TreePlan};
use crate::platform::{progress, store};
use iced::Task;
use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::scan::FoundFile;
use library_core::wire::{BookFileRequest, StoreResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// A duplicate's landing, waiting on its copies: one book filed beside the
/// row the reader pointed at, or a whole fresh subtree spliced in behind the
/// original.
pub(super) enum DupWork {
    Book(Box<BookCopy>),
    Tree(TreePlan),
}

/// The unbound copy run's seat: the run ground the tree's family reads, but
/// whose copies are the library's own rather than a second read of the
/// ground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CopiesDest {
    /// Spliced right behind the shelf whose name the arrival collided with:
    /// a copy appended to the end of the level is a shelf the reader has to
    /// go and find.
    NewShelf { name: String, after: Option<String> },
    /// The *replace*'s target, whose books the sweep has just taken out.
    Into { shelf_id: String },
}

/// A copy run's plan, queued past the scan: the files still owed copies,
/// each wearing the id it lands as, and the run's own answers.
pub(super) struct CopiesWork {
    /// The ground the run walked: its claim, its label.
    pub(super) root: String,
    pub(super) dest: CopiesDest,
    pub(super) opts: FolderOpts,
    pub(super) pending: Vec<PendingCopy>,
}

/// What waits behind a held root: everything the deferred run owes to start
/// again — the picked ground, the sheet answers it walked, and how the
/// reader meant the books held. A re-pick waits whole where a focus walk
/// waits politely.
#[derive(Clone)]
pub(super) enum QueuedImport {
    Walk(PathBuf, FolderOpts, RootPlan),
    Copies { dir: PathBuf, opts: FolderOpts, dest: CopiesDest },
}

/// One loose file queued for the store: the id it lands as, the finding, and
/// the name a spent tombstone remembered.
pub(super) struct PendingCopy {
    pub(super) book_id: String,
    pub(super) file: FoundFile,
    pub(super) title: Option<String>,
}

/// One copy's landing: the address it stored at and the measurement of its
/// own bytes.
pub(super) type Stored = (String, Option<Fingerprint>);

/// The store batch's answer, keyed: the book id each copy was requested
/// under to the address it landed at and the measurement of its own bytes.
pub(super) type CopyMap = HashMap<String, Stored>;

/// The store batch's answer, split: the copies that came home with their
/// measurements, and the first refusal's own sentence for the toast.
pub(super) fn partition_store_results(results: Vec<StoreResult>) -> (CopyMap, Option<String>) {
    let mut copies: CopyMap = HashMap::new();
    let mut failure: Option<String> = None;
    for result in results {
        if result.is_ok() {
            copies.insert(result.id.clone(), (result.store.clone(), result.measured));
        } else if failure.is_none() {
            failure = result.error;
        }
    }
    (copies, failure)
}

mod duplicates;
mod unbound;

impl Mareader {
    /// One store batch of the reader's own asking: one card for the whole
    /// gesture, and the stage that lands when the copies come home.
    pub(super) fn begin_store_run(
        &mut self,
        label: String,
        requests: Vec<BookFileRequest>,
        stage: Stage,
    ) -> Task<Message> {
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun { task, label, sink, rx, latest: None, stage });
        let emit = Arc::clone(&self.runs[self.runs.len() - 1].sink);
        let task_name = task.to_string();
        Task::perform(
            async move { store::store_books(&task_name, &requests, &emit) },
            move |results| Message::CopiesDone(task, results),
        )
    }
}
