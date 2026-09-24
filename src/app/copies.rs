//! The two batch runs — the store's copies and the duplicate pass — and the
//! questions their results raise.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::folder::{self as folder_ops, FolderOpts};
use library_core::ledger::{self};
use library_core::scan::FoundFile;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::{BookFileRequest, StoreResult};
use library_core::{paths, text as lib_text};

use crate::library::duplicate::{self, BookCopy, DupPlan, Duplicated, TreePlan};
use crate::platform::{fs, now_ms, progress, store};
use crate::ui::toast::Tone;
use super::Mareader;
use super::message::Message;
use super::walk::{Claim, FsRun, RootPlan, Stage};

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

impl Mareader {
    /// The scan start of an unbound copy run: one walk of the ground, one
    /// card, no row to answer to — the standing tree's claims are beside
    /// the copies, never under them.
    pub(super) fn begin_copies_run(&mut self, dir: PathBuf, opts: FolderOpts, dest: CopiesDest) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        match self.root_claim(&root) {
            Claim::Free => {}
            Claim::HeldByWalk => {
                self.queued_ask = Some(QueuedImport::Copies { dir, opts, dest });
                return Task::none();
            }
            Claim::HeldByAsk => {
                self.toasts.show(
                    Tone::Info,
                    format!("{} is already being imported.", paths::dir_label(&root)),
                    Instant::now(),
                );
                return Task::none();
            }
        }
        let label = paths::dir_label(&root);
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink: Arc::clone(&sink),
            rx,
            latest: None,
            stage: Stage::CopiesScan { root: root.clone(), opts: opts.clone(), dest },
        });
        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::CopiesScanned(task, result),
        )
    }

    /// The unbound walk's scan answer: heal the rows the ground reads
    /// beside the tree, then owe every quieter file a copy of its own — the
    /// ledger's table says which, and the batch rides the run's own card.
    pub(super) fn copies_scanned(
        &mut self,
        task: u64,
        result: Result<Vec<FoundFile>, String>,
        now: Instant,
    ) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let Stage::CopiesScan { root, opts, dest } = &self.runs[ix].stage else {
            return Task::none();
        };
        let (root, opts, dest) = (root.clone(), opts.clone(), dest.clone());
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.runs.remove(ix);
                self.toasts.show(Tone::Error, error, now);
                return self.release_root(&root);
            }
        };
        let stamp = now_ms();
        // Everything but the copy the library already made, one book per
        // fingerprint, off the ledger's own pure table; the heal reads the
        // copy paths again against the tree beside it.
        let registry = ledger::registry_of(&self.library.books);
        let copy_paths = ledger::copy_over_paths(&found, &registry, &self.library.books);
        let mut adds = ledger::unbound_copies(&found, &registry, &self.library.books);
        let mut healed = 0usize;
        // A file at an address the library already reads is that book,
        // whatever the two fingerprints say.
        adds.retain(|file| {
            if copy_paths.contains(&file.path) {
                return true;
            }
            match book::book_rows_mut(&mut self.library.books).find(|b| b.path() == file.path) {
                Some(existing) => {
                    existing.heal(file.fp);
                    healed += 1;
                    false
                }
                None => true,
            }
        });
        if adds.is_empty() {
            self.runs.remove(ix);
            self.toasts.show(
                Tone::Info,
                format!("Everything in “{}” is already in the library", paths::dir_label(&root)),
                now,
            );
            if healed > 0 {
                return Task::batch([self.persist_library(), self.release_root(&root)]);
            }
            return self.release_root(&root);
        }
        let pending: Vec<PendingCopy> = adds
            .into_iter()
            .map(|file| PendingCopy {
                book_id: library_core::id::next_id(stamp),
                file,
                title: None,
            })
            .collect();
        let requests: Vec<BookFileRequest> = pending
            .iter()
            .map(|each| BookFileRequest {
                from: each.file.path.clone(),
                id: each.book_id.clone(),
            })
            .collect();
        let sink = Arc::clone(&self.runs[ix].sink);
        let task_name = task.to_string();
        self.runs[ix].stage = Stage::Copies {
            work: Box::new(CopiesWork { root, dest, opts, pending }),
        };
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::CopiesDone(task, results),
        )
    }

    /// The unbound copies' landing: every file lands as the library's own —
    /// its own measurement, its source's fingerprint free — on the seat the
    /// run's answer minted, and the tree beside reads on untouched.
    fn copies_run_done(&mut self, work: CopiesWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let stamp = now_ms();
        let mut landed_ids: Vec<(String, String)> = Vec::new();
        let mut root_shelf: Option<String> = None;
        let mut rungs: HashMap<String, String> = HashMap::new();
        for item in &work.pending {
            let Some((store_path, measured)) = copies.get(&item.book_id) else {
                continue;
            };
            let independent =
                book::book_rows(&self.library.books).any(|each| each.path() == item.file.path);
            let mut minted = Book::new(
                item.book_id.clone(),
                item.file.fp,
                item.file.admitted_format(),
                Origin::Stored { src: Some(item.file.path.clone()), store: store_path.clone() },
                stamp,
            );
            minted.independent = independent;
            minted.adopt_measurement(*measured);
            let placed_id = minted.id.clone();
            self.library.books.push(book::Row::Book(minted));
            // The seat each copy lands on, resolved in batch order before
            // any write: the root shelf is minted once, and a grouped run
            // cuts each file's rung under it.
            let on_shelf =
                root_shelf.get_or_insert_with(|| self.copies_dest_shelf(&work.dest, stamp)).clone();
            let seat = if work.opts.groups {
                self.copies_rung_shelf(&work.root, &on_shelf, &mut rungs, item.file.subfolder(), stamp)
            } else {
                on_shelf
            };
            landed_ids.push((placed_id, seat));
        }
        let landed = landed_ids.len();
        for (book_id, seat) in &landed_ids {
            if let Some(home) = shelf::find_mut(&mut self.library.shelves, seat) {
                shelf::shelf_add(home, book_id);
            }
        }
        let persist = if landed > 0 { self.persist_library() } else { Task::none() };
        if landed > 0 {
            self.toasts.show(
                Tone::Info,
                format!(
                    "Imported {} from “{}”",
                    lib_text::plural(landed, "book", "books"),
                    paths::dir_label(&work.root)
                ),
                Instant::now(),
            );
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        // Navigate to the shelf the run minted or emptied — the web's light
        // waits on the grid's scroll-to, documented beside the reveal's
        // other half; the way there rides first.
        let reveal = match &work.dest {
            CopiesDest::Into { shelf_id } => Some(shelf_id.clone()),
            _ => root_shelf,
        };
        if let Some(seat) = reveal {
            self.shelf = shelf::find(&self.library.shelves, &seat)
                .and_then(|each| each.parent.clone())
                .unwrap_or_else(|| ALL_SHELF.to_string());
        }
        let release = self.release_root(&work.root);
        Task::batch([persist, release])
    }

    /// The unbound run's root shelf, minted on first use: spliced behind
    /// the shelf the collision named, or the *replace*'s emptied target.
    fn copies_dest_shelf(&mut self, dest: &CopiesDest, stamp: u64) -> String {
        match dest {
            CopiesDest::Into { shelf_id } => shelf_id.clone(),
            CopiesDest::NewShelf { name, after } => {
                let id = library_core::id::next_shelf_id(stamp);
                let at = after
                    .as_deref()
                    .and_then(|after| self.library.shelves.iter().position(|s| s.id == after))
                    .map_or(self.library.shelves.len(), |at| at + 1);
                self.library.shelves.insert(
                    at,
                    shelf::Shelf::virtual_shelf(id.clone(), name.clone(), None),
                );
                id
            }
        }
    }

    /// The bound walk's chain rule on a local map instead of a ledger's: an
    /// unbound run owes no row, so its rungs live only until the landing
    /// files them onto `shelves`. A folder that does not group has no
    /// rungs.
    fn copies_rung_shelf(
        &mut self,
        root: &str,
        dest_shelf: &str,
        rungs: &mut HashMap<String, String>,
        key: &str,
        stamp: u64,
    ) -> String {
        let dir = root.to_string();
        let mut parent = dest_shelf.to_string();
        for rung in folder_ops::key_chain(key) {
            if rung.is_empty() {
                continue;
            }
            if let Some(id) = rungs.get(rung) {
                parent = id.clone();
                continue;
            }
            let id = library_core::id::next_shelf_id(stamp);
            // The rung's name is its directory's own label, the way a bound
            // walk names the rungs its files mint.
            let name =
                rung.rsplit('/').next().filter(|label| !label.is_empty()).unwrap_or(&dir).to_string();
            self.library
                .shelves
                .push(shelf::Shelf::virtual_shelf(id.clone(), name, Some(parent.clone())));
            rungs.insert(rung.to_string(), id.clone());
            parent = id;
        }
        parent
    }

    /// The store's answer for a folder import: land what copied, count what
    /// the store refused, and let the rest of the batch stand.
    pub(super) fn copies_done(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        match run.stage {
            Stage::Storing { plan } => {
                let root = plan.folder.root.clone();
                let (copies, failure) = partition_store_results(results);
                let outcome = self.land_folder_walk(*plan, Some(copies));
                if let Some(error) = failure {
                    self.toasts.show(Tone::Error, error, Instant::now());
                }
                let release = self.release_root(&root);
                Task::batch([outcome, release])
            }
            Stage::Copies { work } => self.copies_run_done(*work, results),
            Stage::Duplicating { work } => self.duplicates_done(*work, results),
            Stage::Departing { work } => self.departing_done(*work, results),
            _ => Task::none(),
        }
    }

    /// The store's answer for one duplicate run: land what came home, count
    /// it for the report, toast what the store refused, and let the queue
    /// walk on.
    fn duplicates_done(&mut self, work: DupWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let now = now_ms();
        let recorded = match work {
            DupWork::Book(copy) => copies.get(&copy.new_id).map(|(store, measured)| {
                let title = duplicate::land_book(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    &self.shelf,
                    *copy,
                    store.clone(),
                    *measured,
                    now,
                );
                Duplicated { name: title, shelf: false }
            }),
            DupWork::Tree(plan) => {
                let name = duplicate::land_tree(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    plan,
                    &copies,
                    now,
                );
                Some(Duplicated { name, shelf: true })
            }
        };
        if let Some(one) = recorded {
            self.dup_landed.push(one);
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        self.pump_dup()
    }

    /// The duplicate queue's step: one entry at a time, because each counter
    /// name counts against the level as the last landing left it — the web's
    /// own sequential loop, walked by messages instead of an async fn. The
    /// entries that owe no bytes land on the spot and the walk continues;
    /// the ones that do ride a run, and the queue resumes when its copies
    /// come home.
    pub(super) fn pump_dup(&mut self) -> Task<Message> {
        while let Some(entry) = self.dup_queue.first().cloned() {
            self.dup_queue.remove(0);
            let now = now_ms();
            let plan =
                duplicate::plan_one(&self.library.books, &self.library.shelves, &entry, now);
            match plan {
                DupPlan::Skip => {}
                DupPlan::Dead(note) => {
                    self.toasts.show(Tone::Error, note, Instant::now());
                }
                DupPlan::ShelfLink { row_id, name, target } => {
                    let title = duplicate::land_shelf_link(
                        &mut self.library.books,
                        &mut self.library.shelves,
                        &self.shelf,
                        &row_id,
                        &name,
                        &target,
                        now,
                    );
                    self.dup_landed.push(Duplicated { name: title, shelf: false });
                }
                DupPlan::Book(copy) => {
                    let label = copy.shown.clone();
                    let requests = vec![BookFileRequest {
                        from: copy.book.path().to_string(),
                        id: copy.new_id.clone(),
                    }];
                    return self.begin_dup(label, requests, DupWork::Book(copy));
                }
                DupPlan::Tree(plan) => {
                    let requests = duplicate::tree_requests(&plan);
                    if requests.is_empty() {
                        // A tree of links and dead rows copies nothing: no
                        // card, and the landing is the whole run.
                        let name = duplicate::land_tree(
                            &mut self.library.books,
                            &mut self.library.shelves,
                            plan,
                            &HashMap::new(),
                            now,
                        );
                        self.dup_landed.push(Duplicated { name, shelf: true });
                        continue;
                    }
                    let label = plan.label.clone();
                    return self.begin_dup(label, requests, DupWork::Tree(plan));
                }
            }
        }
        // The queue ran out: the report the whole batch ends with, and the
        // one persist that covers it — a link-only batch, which never met a
        // run, lands here too.
        if self.dup_landed.is_empty() {
            return Task::none();
        }
        let report = duplicate::report(&self.dup_landed);
        self.dup_landed.clear();
        self.toasts.show(Tone::Info, report, Instant::now());
        self.persist_library()
    }

    /// A duplicate's store run: the card wears the name of what is being
    /// duplicated, the way a folder run wears its folder.
    fn begin_dup(
        &mut self,
        label: String,
        requests: Vec<BookFileRequest>,
        work: DupWork,
    ) -> Task<Message> {
        self.begin_store_run(label, requests, Stage::Duplicating { work: Box::new(work) })
    }

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
