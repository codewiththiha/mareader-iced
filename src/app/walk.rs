//! The folder walk: claiming a folder's root, planning a run against its
//! ledger, and the rungs its files land on.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Fingerprint};
use library_core::folder::{FolderOpts, Tombstone, WatchedFolder};
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self, Shelf};
use library_core::wire::{BookFileRequest, ImportProgress, PathCheck};
use library_core::{paths, text as lib_text};
use reader_core::format::is_supported_path;

use crate::library::conflicts::{self, ConflictAsk};
use crate::platform::{dialogs, fs, now_ms, progress, store};
use crate::ui::toast::Tone;
use super::Mareader;
use super::copies::{CopiesDest, CopiesWork, DupWork, PendingCopy, QueuedImport};
use super::message::Message;
use super::moves::DepartWork;
use super::sheets::{Asked, LitNote, Sheet};

/// How long a rest over a book must last before the fold is offered: long
/// enough that a reorder crossing the card never brews a shelf, short
/// enough that the reader is not left waiting on the answer (the web
/// session's own 650ms). The crumb sink's 420ms rides with the crumb
/// targets, which arrive with the bar's drag geometry.
pub(super) const FOLD_DWELL_MS: u128 = 650;

/// Shorter than the fold's dwell, and for one reason only: the crumb is
/// the one target smaller than the ghost hovering it, and a full-size
/// ghost hides the very name being aimed at — so a rest this long parks
/// the ghost on the crumb, shrunk (the web session's own 420ms).
pub(super) const SINK_DWELL_MS: u128 = 420;

/// The fold panel's close waits this long behind the pointer's leave — the
/// web intent's own grace, so a diagonal crossing of the panel's corner
/// does not blink it shut.
pub(super) const ELLIPSIS_GRACE_MS: u64 = 220;

/// The centre pill's usable floor: what the fold reserves for the search
/// before it starts hiding levels, so a crammed bar never squeezes the
/// search box past use.
pub(super) const CENTER_FLOOR: f32 = 400.0;

/// The sunk ghost's scale, straight off drag.css: a third of its size is
/// what keeps the crumb readable, because the chrome's lane sits below the
/// drag layer and no z-step can put the ghost behind a crumb without
/// putting it behind everything.
pub(super) const SUNK_SCALE: f32 = 0.38;

/// What a picked ground belongs to: the tree that governs it, the rung the
/// ground names inside the tree, the watch answer that rung carries, and
/// the tree's own options with the rung's shape answer. The import sheet
/// opens seeded from these — a folder inside a governed tree asks nothing
/// the tree already answered — and its Import writes the watch answer back
/// onto the rung, never onto the tree's root.
#[derive(Debug, Clone)]
pub(super) struct GroundWatch {
    pub(super) tree_id: String,
    pub(super) rung: String,
    pub(super) on: bool,
    pub(super) opts: FolderOpts,
}

/// The watch seat a folder shelf answers for: which rung of which tree,
/// what that rung watches now, and the labels the toggle row speaks. The
/// row the folder context menu has and no other menu does, because no
/// other shelf has a rung to answer for.
pub(super) struct ShelfWatch {
    pub(super) folder_id: String,
    pub(super) rung: String,
    pub(super) on: bool,
    pub(super) label: String,
    pub(super) rung_label: Option<String>,
}

/// One filesystem run and its channel: the id the beats carry, the label
/// the dock pill names it by, the parked receiver the subscription takes
/// on first build, the sender the run's later stages reuse, and the latest
/// beat — the pill's whole input.
pub(super) struct FsRun {
    pub(super) task: u64,
    pub(super) label: String,
    pub(super) sink: progress::ProgressSink,
    pub(super) rx: progress::SharedProgress,
    pub(super) latest: Option<ImportProgress>,
    pub(super) stage: Stage,
}

/// Where a run is in its life, and the plan its next stage lands. The plans
/// ride in boxes: a run is a small thing the state tree holds many of, and
/// a walk's plan — a whole ledger row among its fields — would otherwise
/// decide the size of every stage.
pub(super) enum Stage {
    /// A folder walk is in flight.
    Walking { root: String, opts: FolderOpts, asked: Asked, plan: RootPlan },
    /// The scan half of an unbound copy run: the ground walked once more,
    /// with no ledger behind it.
    CopiesScan { root: String, opts: FolderOpts, dest: CopiesDest },
    /// The store half of an unbound copy run: the copes the landing files
    /// beside the ground's standing tree.
    Copies { work: Box<CopiesWork> },
    /// The store is copying a folder import's additions; the walk's answer
    /// waits in the plan.
    Storing { plan: Box<WalkPlan> },
    /// The picker's files are being measured.
    Measuring { target: Option<String> },
    /// The store is copying the picker's files; the landings wait in the
    /// plan.
    Copying { plan: Box<FilesPlan> },
    /// A restore is measuring the one file its log remembered.
    Restoring { folder_id: String, stone: Box<Tombstone>, opts: FolderOpts },
    /// The store is copying a restore's file; the landing waits in the run.
    RestoreCopying {
        folder_id: String,
        stone: Box<Tombstone>,
        opts: FolderOpts,
        found: Box<FoundFile>,
        book_id: String,
    },
    /// The store is copying a duplicate's bytes; the landing waits in the
    /// work.
    Duplicating { work: Box<DupWork> },
    /// The store is copying a departure's books; the interrupted gesture
    /// waits in the work.
    Departing { work: Box<DepartWork> },
}

/// The folder walk's answer, planned against the ledger and waiting for its
/// copies (or landing straight away when the books stay at place).
pub(super) struct WalkPlan {
    /// The ledger row, resolved against the library as the walk ended —
    /// placed marks, spent tombstones and the shelf map are written onto it
    /// as the landing runs, and it goes back whole at the end.
    pub(super) folder: WatchedFolder,
    pub(super) asked: Asked,
    /// The covered walk's light — the shelf its own pick names, kept with
    /// the plan until the landing answers for it.
    pub(super) continuation: Option<Continuation>,
    /// The folder's display name, for the toasts.
    pub(super) root_name: String,
    /// The deduped name a fresh root rung wears; `None` when the map already
    /// holds the root shelf.
    pub(super) planned_root: Option<String>,
    /// The additions, each wearing the book id it will land as.
    pub(super) adds: Vec<(String, FoundFile)>,
    /// The moves the ledger healed: book id to its new address.
    pub(super) relinks: Vec<(String, String)>,
    /// The per-file questions a merge owes, asked only once the landing is
    /// done — a file asked about must not stand among the landed rows.
    pub(super) asks: Vec<ConflictAsk>,
    /// The rows a planned tree re-seats: the row, and the file whose rung
    /// decides where it lands.
    pub(super) replacements: Vec<(String, FoundFile)>,
    /// The addresses whose file the library already reads in place, where
    /// this run lands its own copy beside the linked row that reads it.
    pub(super) copy_paths: HashSet<String>,
    /// The rows a moved-out log already answers for: the reader has these
    /// books, so the walk lights them up rather than landing neighbours.
    pub(super) represented: Vec<String>,
}

/// The walk's own root answer: *as new* names the tree something else and
/// continues from a fresh row, so the old row keeps answering its shelf;
/// *merge* files the new scans into the shelf that already holds the name.
#[derive(Clone, Debug, Default)]
pub(super) struct RootPlan {
    pub(super) rename: Option<String>,
    pub(super) into: Option<String>,
    /// The covered walk's light: the shelf its pick stood on, and the name
    /// its ledger row wears — the re-import's own run answers to them both.
    pub(super) continuation: Option<Continuation>,
}

/// What the covered walk's own light knows: where the pick's own shelf
/// stands, and which name the session opened with. The landing owes it an
/// answer even when nothing new was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Continuation {
    pub(super) shelf_id: String,
    pub(super) name: String,
}

/// The loose-file run's answer, waiting for its copies.
pub(super) struct FilesPlan {
    /// The level the pick lands its books on, when one is standing.
    pub(super) target: Option<String>,
    pub(super) pending: Vec<PendingCopy>,
    /// Covered files restored as their folder's linked books, counted for
    /// the toast.
    pub(super) restored: usize,
    /// The questions the screens raised, riding along so they are asked
    /// only once the copies have landed: a sheet answered while its own
    /// copy is still in flight would land beside a ghost.
    pub(super) asks: Vec<ConflictAsk>,
    /// The books the drop found the reader already had, by a folder's own
    /// moved-out log: the light lands on the first of them.
    pub(super) represented: Vec<String>,
    /// The folder ledger an answered file settles when its copy comes
    /// home — the answer's own placement record, absent for a pick's run.
    pub(super) settle: Option<(String, Fingerprint)>,
    /// The slot the answered file takes, when an answer named one.
    pub(super) index: Option<usize>,
}

/// Who holds a folder's root: the claim question a second run asks before
/// it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Claim {
    Free,
    /// A focus walk holds it; an ask queues behind the walk's release.
    HeldByWalk,
    /// An ask holds it; a second ask is told, a walk waits for the next
    /// focus.
    HeldByAsk,
}

/// The found file a path check describes, when the check found a document:
/// the loose-file run's translation from measurement to ledger row.
pub(super) fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
    if !is_supported_path(&check.path) {
        return None;
    }
    let fp = check.fingerprint()?;
    Some(FoundFile {
        rel: paths::file_name(&check.path),
        path: check.path.clone(),
        ext: paths::extension(&check.path),
        size: check.size,
        fp,
    })
}

/// The removed row's second line: when the reader took the book out, and —
/// when the log remembers the file's own bytes — how big the promise is.
pub(super) fn removed_sublabel(entry: &Tombstone, stamp: u64) -> String {
    let age = lib_text::human_age(entry.removed_ms, stamp);
    match entry.fp.mtime_ms {
        0 => format!("removed {age}"),
        _ => format!("removed {age} · {}", lib_text::human_size(entry.fp.size)),
    }
}

/// One spelling for the shelf chain a folder walk mints through — the books
/// it adds and the rungs between them — so a rung cannot be minted twice
/// under two spellings of its own name. Rungs already in the map are
/// reused, which is what makes a rescan continue the tree instead of
/// growing a twin beside it.
pub(super) fn chain_for(
    folder: &mut WatchedFolder,
    key: &str,
    stamp: u64,
    planned_root: Option<&str>,
    root: &str,
    new_shelves: &mut Vec<shelf::Shelf>,
) -> String {
    let folder_id = folder.id.clone();
    let root = root.to_string();
    let planned = planned_root.map(str::to_string);
    folder.shelf_chain_for(
        key,
        |_| library_core::id::next_shelf_id(stamp),
        move |rung| {
            if rung.is_empty() {
                planned.clone().unwrap_or_else(|| paths::dir_label(&root))
            } else {
                rung.rsplit('/').next().unwrap_or(rung).to_string()
            }
        },
        |rung, id, name, parent| {
            let rel = (!rung.is_empty()).then(|| rung.to_string());
            new_shelves.push(shelf::Shelf::folder_shelf(id, name, &folder_id, rel, parent));
        },
    )
}

/// The rungs a folded member brings: every shelf the folded folder owns,
/// keyed the way the receiving tree keys its own — the rung the member's
/// directory names, and the ones below it.
pub(super) fn member_rungs(shelves: &[Shelf], gone_id: &str, rel: &str) -> Vec<(String, String)> {
    shelves
        .iter()
        .filter(|s| s.kind.folder_id() == Some(gone_id))
        .filter_map(|s| {
            let library_core::shelf::ShelfKind::Folder { rel: own, .. } = &s.kind else {
                return None;
            };
            let own = own.as_deref().unwrap_or("");
            let key =
                if own.is_empty() { rel.to_string() } else { format!("{rel}/{own}") };
            Some((key, s.id.clone()))
        })
        .collect()
}

/// The map's key as the shelf's own rel: the root's empty key is no rel.
pub(super) fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// The minted rungs join the list: one push per shelf no id already holds.
pub(super) fn page_into(shelves: &mut Vec<Shelf>, minted: Vec<Shelf>) {
    for made in minted {
        if !shelves.iter().any(|s| s.id == made.id) {
            shelves.push(made);
        }
    }
}

/// The whole of one tree's books onto `seat`, and the shelves the one-shelf
/// answer has no place for taken out: the flattening a re-import asks for,
/// and the one an adopting tree takes a member in by. Every rung of `from`
/// goes except the one that is `seat`. Books move, readers do not: a shelf
/// the reader made inside one comes up to `seat` with its books.
pub(super) fn flatten_rungs(shelves: &mut Vec<Shelf>, from: &str, seat: &str) {
    let own: Vec<String> =
        shelf::rungs_of(shelves, from).into_values().map(String::from).collect();
    let going: HashSet<String> = own.iter().filter(|id| id.as_str() != seat).cloned().collect();
    // A set rather than a growing list: the whole tree's books pass through
    // here, and membership was a scan per book.
    let mut held: HashSet<String> = HashSet::new();
    for rung in &own {
        let Some(one) = shelf::find(shelves, rung) else {
            continue;
        };
        held.extend(one.books.iter().cloned());
    }
    if let Some(one) = shelf::find_mut(shelves, seat) {
        for book_id in held {
            shelf::shelf_add(one, &book_id);
        }
    }
    for one in shelves.iter_mut() {
        if one.parent.as_deref().is_some_and(|parent| going.contains(parent)) {
            one.parent = Some(seat.to_string());
        }
    }
    shelves.retain(|one| !going.contains(&one.id));
}

/// Put the folder row back, by id. One place, because the ledger is the
/// part of the library that must never be written half-updated: a `placed`
/// set that lost an entry re-adds a book the reader already filed.
pub(super) fn write_folder_row(folders: &mut Vec<WatchedFolder>, folder: WatchedFolder) {
    match folders.iter().position(|each| each.id == folder.id) {
        Some(at) => folders[at] = folder,
        None => folders.push(folder),
    }
}

impl Mareader {
    /// The library's two automatic measurements, in the one order they owe:
    /// first a pass over every address the library holds — marking books
    /// missing when their file was deleted or moved, and healing the
    /// fingerprints a migration left pending — then the walk of every
    /// watched folder. The walk only ever sees what the measure pass made
    /// legible.
    pub(super) fn start_measure_pass(&mut self) -> Task<Message> {
        if self.verifying {
            return Task::none();
        }
        let addresses: Vec<String> =
            book::book_rows(&self.library.books).map(|b| b.path().to_string()).collect();
        if addresses.is_empty() {
            return self.run_watched();
        }
        self.verifying = true;
        Task::perform(async move { fs::check_paths(&addresses) }, Message::ChecksDone)
    }

    pub(super) fn checks_done(&mut self, checks: Vec<PathCheck>) -> Task<Message> {
        self.verifying = false;
        let mut changed = false;
        for check in &checks {
            if !book::apply_check(&mut self.library.books, check).is_empty() {
                changed = true;
            }
        }
        let walks = self.run_watched();
        if changed {
            Task::batch([self.persist_library(), walks])
        } else {
            walks
        }
    }

    /// The walk of every folder that owes one. Held back while a migrated
    /// book still wears a placeholder fingerprint — scanning against
    /// unmeasured identities would re-add every one of them — and while the
    /// focus is the app's own picker closing.
    fn run_watched(&mut self) -> Task<Message> {
        if self.library.awaiting_check() || dialogs::picker_focus() {
            return Task::none();
        }
        let watched: Vec<(String, FolderOpts)> = self
            .library
            .folders
            .iter()
            // Watched anywhere, not only at the root: a tree turned off at
            // the root with one subfolder still on owes the walk, and the
            // ledger's per-rung gate keeps the off rungs quiet inside it.
            .filter(|folder| folder.owes_walk())
            .map(|folder| (folder.root.clone(), folder.opts.clone()))
            .collect();
        let mut walks = Vec::with_capacity(watched.len());
        for (root, opts) in watched {
            walks.push(self.begin_folder_walk(
                PathBuf::from(root),
                opts,
                Asked::OnFocus,
                RootPlan::default(),
            ));
        }
        Task::batch(walks)
    }

    // ── The folder run ──────────────────────────────────────────────────

    /// Kick off a folder walk: one run id, one channel, one subscription
    /// that lives exactly as long as the run. Two walks of one folder are
    /// two snapshots of the same ledger row and two writes back, so the
    /// root is claimed for the length of the run — an ask outranks a
    /// rescan and queues behind it, and a second ask is told the first is
    /// still running.
    pub(super) fn begin_folder_walk(
        &mut self,
        dir: PathBuf,
        opts: FolderOpts,
        asked: Asked,
        plan: RootPlan,
    ) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        match self.root_claim(&root) {
            Claim::Free => {}
            Claim::HeldByWalk => {
                if asked == Asked::Explicitly {
                    self.queued_ask = Some(QueuedImport::Walk(dir, opts, plan));
                }
                return Task::none();
            }
            Claim::HeldByAsk => {
                if asked == Asked::Explicitly {
                    self.toasts.show(
                        Tone::Info,
                        format!("{} is already being imported.", paths::dir_label(&root)),
                        Instant::now(),
                    );
                }
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
            stage: Stage::Walking { root: root.clone(), opts: opts.clone(), asked, plan },
        });
        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::ScanDone(task, result),
        )
    }

    /// Who holds a root right now, if anyone — the runs and the queued ask
    /// behind them.
    pub(super) fn root_claim(&self, root: &str) -> Claim {
        if self.queued_ask.as_ref().is_some_and(|queued| {
            let held = match queued {
                QueuedImport::Walk(dir, _, _) | QueuedImport::Copies { dir, .. } => {
                    dir.to_string_lossy()
                }
            };
            held.as_ref() == root
        }) {
            return Claim::HeldByAsk;
        }
        for run in &self.runs {
            let holds = match &run.stage {
                Stage::Walking { root: held, .. }
                | Stage::CopiesScan { root: held, .. } => held == root,
                Stage::Storing { plan } => plan.folder.root == root,
                Stage::Copies { work } => work.root == root,
                Stage::Measuring { .. }
                | Stage::Copying { .. }
                | Stage::Restoring { .. }
                | Stage::RestoreCopying { .. }
                | Stage::Duplicating { .. }
                | Stage::Departing { .. } => false,
            };
            if !holds {
                continue;
            }
            let focus_walk = match &run.stage {
                Stage::Walking { asked, .. } => *asked == Asked::OnFocus,
                Stage::Storing { plan } => plan.asked == Asked::OnFocus,
                Stage::Measuring { .. }
                | Stage::Copying { .. }
                | Stage::Restoring { .. }
                | Stage::RestoreCopying { .. }
                | Stage::Duplicating { .. }
                | Stage::Departing { .. }
                | Stage::CopiesScan { .. }
                | Stage::Copies { .. } => false,
            };
            return if focus_walk { Claim::HeldByWalk } else { Claim::HeldByAsk };
        }
        Claim::Free
    }

    /// A run ended: the ask queued behind its root, if one waited, starts
    /// now — the release the queued ask was promised.
    pub(super) fn release_root(&mut self, root: &str) -> Task<Message> {
        match self.queued_ask.take() {
            Some(QueuedImport::Walk(dir, opts, plan)) if dir.to_string_lossy() == root => {
                self.begin_folder_walk(dir, opts, Asked::Explicitly, plan)
            }
            Some(QueuedImport::Copies { dir, opts, dest }) if dir.to_string_lossy() == root => {
                self.begin_copies_run(dir, opts, dest)
            }
            Some(other) => {
                self.queued_ask = Some(other);
                Task::none()
            }
            None => Task::none(),
        }
    }

    /// The walk's answer arrived: plan it against the ledger, and either
    /// land it (the books stay at place) or hand the additions to the store
    /// first (the books are copied).
    pub(super) fn scan_done(
        &mut self,
        task: u64,
        result: Result<Vec<FoundFile>, String>,
        now: Instant,
    ) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let (root, opts, asked, plan) = match &self.runs[ix].stage {
            Stage::Walking { root, opts, asked, plan } => {
                (root.clone(), opts.clone(), *asked, plan.clone())
            }
            _ => return Task::none(),
        };
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.runs.remove(ix);
                // A quiet walk that could not read its folder leaves no
                // trace; an ask answers with the advice.
                if asked == Asked::Explicitly {
                    self.toasts.show(Tone::Error, error, now);
                }
                return self.release_root(&root);
            }
        };
        self.plan_folder_walk(ix, task, &root, opts, asked, plan, found)
    }

    /// The diff stage: everything between the walk's raw findings and the
    /// landing. Decides against the ledger the crate owns — tombstones
    /// first, then the registry, then the rung's tracking answer — and
    /// writes nothing but the folder's own row.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn plan_folder_walk(
        &mut self,
        ix: usize,
        task: u64,
        root: &str,
        opts: FolderOpts,
        asked: Asked,
        mut plan: RootPlan,
        mut found: Vec<FoundFile>,
    ) -> Task<Message> {
        let stamp = now_ms();
        // Importing a folder the library already holds continues that row's
        // `placed` and `ignored` sets — the whole point of them: re-importing
        // is how a reader would otherwise get back every book they deleted
        // last week.
        let standing = self.library.folders.iter().find(|folder| folder.root == root).cloned();
        // The *as new* run owes a fresh row beside the old one: placements
        // and quiet logs key off the row id, so a second run of one ground
        // cannot spend the first run's answers.
        let fresh_row = plan.rename.is_some() || standing.is_none();
        let mut folder = if plan.rename.is_some() {
            WatchedFolder::new(library_core::id::next_folder_id(stamp), root, opts.clone())
        } else {
            standing.clone().unwrap_or_else(|| {
                WatchedFolder::new(library_core::id::next_folder_id(stamp), root, opts.clone())
            })
        };
        folder.opts = opts;
        if folder.mode().reads_in_place() {
            if fresh_row {
                // A fresh row's watch is the sheet's switch, at the root;
                // `set_tracking` keeps the tree and the legacy flag agreed.
                folder.set_tracking("", folder.opts.watch);
            } else {
                // A continuation keeps the tree's own answers.
                folder.opts.watch = folder.tracking.tracked();
            }
        }
        folder.prune_shelf_map(&self.library.shelves);
        // The level it joins is the rung it files under from now on: the
        // merge's root rung is that shelf, which makes the answer a promise
        // the next scan keeps.
        if let Some(into) = &plan.into {
            folder.shelf_map.insert(String::new(), into.clone());
        }
        // An *as new* answer owes a tree of its own: every rung mints fresh
        // under the counter-named root.
        if plan.rename.is_some() {
            folder.shelf_map.clear();
        }

        let registry = ledger::registry_of(&self.library.books);
        // The addresses whose file the library already reads in place, where
        // this run lands its own copy beside the linked row: a copies import
        // is the library's second instance, unrelated to the tree reading the
        // ground, and the tree keeps the rows it has.
        let copy_paths: HashSet<String> =
            if folder.mode().copies_files() && asked == Asked::Explicitly {
                ledger::copy_over_paths(&found, &registry, &self.library.books)
            } else {
                HashSet::new()
            };
        ledger::prune_tombstones(&mut folder, &registry);
        // Written on every scan, including one that changes nothing: the
        // restore menu's "moved out of this folder" answer is only as fresh
        // as the last walk.
        folder.record_seen(&found);
        // A file a moved-out log binds to a LIVING row is a book the reader
        // already has: the walk lights that row up rather than landing a
        // second book beside it. A quiet walk asks nothing — its light would
        // be a light the reader did not ask for.
        let represented = if asked == Asked::OnFocus {
            Vec::new()
        } else {
            self.take_represented(Some(folder.id.as_str()), &mut found)
        };

        let actions = match asked {
            Asked::OnFocus => ledger::diff_folder(&folder, &registry, &found),
            Asked::Explicitly => ledger::diff_import(&folder, &registry, &found),
        };
        let mut adds: Vec<FoundFile> = Vec::new();
        let mut relinks: Vec<(String, String)> = Vec::new();
        for action in actions {
            match action {
                ScanAction::Add(file) => adds.push(file),
                ScanAction::Relink { book_id, to } => relinks.push((book_id, to)),
                ScanAction::Skip => {}
            }
        }
        ledger::keep_healable_relinks(&mut relinks, &self.library.books);
        // One book per fingerprint per scan: two byte-identical files are
        // one book, and copying both would leave an orphan in the store
        // nothing can remove.
        let mut seen: HashSet<Fingerprint> = HashSet::new();
        adds.retain(|file| seen.insert(file.fp));
        // The ledger answered Skip for the copy run's own files — their
        // content is known — but the reader asked for a second instance of
        // each, so the run owes every one of them a book.
        for file in found.iter().filter(|file| copy_paths.contains(&file.path)) {
            if !adds.iter().any(|each| each.path == file.path) {
                adds.push(file.clone());
            }
        }

        // A merge's new arrivals land on a rung the level already held, and
        // the rows a rename or a merge re-seats follow their own rungs onto
        // the planned tree: one screen answers both, and a question rides the
        // arrival itself rather than a second pass.
        let screened = conflicts::screen_planned(
            &folder,
            &self.library.books,
            &self.library.shelves,
            &registry,
            &found,
            (plan.rename.is_some(), plan.into.as_deref()),
            &mut adds,
            &copy_paths,
        );
        let asks = screened.asks;

        let root_name = paths::dir_label(root);
        if adds.is_empty() && relinks.is_empty() && asks.is_empty() && represented.is_empty() {
            // Nothing to do. A quiet walk leaves no trace beyond the row's
            // stamp; an ask still owes the reader the news.
            folder.scanned_ms = stamp;
            let fresh = standing.is_none();
            write_folder_row(&mut self.library.folders, folder);
            let persist = if fresh || asked == Asked::Explicitly {
                self.persist_library()
            } else {
                Task::none()
            };
            self.runs.remove(ix);
            if asked == Asked::Explicitly {
                if let Some(continuation) = plan.continuation.take() {
                    // The covered walk's empty answer is the note, and the
                    // note's close answers it back to the shelf the pick
                    // meant.
                    self.sheet = Some(Sheet::AlreadyImported {
                        note: LitNote {
                            shelf_id: continuation.shelf_id,
                            name: continuation.name,
                            kind: conflicts::NoteKind::NothingNew,
                        },
                    });
                } else {
                    let line = if found.is_empty() {
                        format!("No documents found in “{root_name}”")
                    } else {
                        format!("Everything in “{root_name}” is already in the library")
                    };
                    self.toasts.show(Tone::Info, line, Instant::now());
                }
            }
            return Task::batch([persist, self.release_root(root)]);
        }

        // The root rung's name, deduped against the shelves standing — two
        // doors of one name are two doors a reader cannot tell apart. A
        // continuation keeps the shelf the map already names.
        let planned_root = match plan.rename.clone() {
            Some(name) => Some(name),
            None => (!folder.shelf_map.contains_key("")).then(|| {
                let names: HashSet<String> =
                    self.library.shelves.iter().map(|each| each.name.clone()).collect();
                if names.contains(&root_name) {
                    book::duplicate_title(&root_name, &names)
                } else {
                    root_name.clone()
                }
            }),
        };
        let adds: Vec<(String, FoundFile)> = adds
            .into_iter()
            .map(|file| (library_core::id::next_id(stamp), file))
            .collect();
        let plan = WalkPlan {
            folder,
            asked,
            continuation: plan.continuation.take(),
            root_name,
            planned_root,
            adds,
            relinks,
            asks,
            replacements: screened.replacements,
            copy_paths,
            represented,
        };

        // A run with nothing to copy skips the store altogether: its only
        // remaining work is the light and the rows a log already answered for.
        if plan.folder.mode().copies_files() && !plan.adds.is_empty() {
            // The store batch rides the run's own channel: the pill the
            // scan lit keeps counting, in its copy phase.
            let requests: Vec<BookFileRequest> = plan
                .adds
                .iter()
                .map(|(id, file)| BookFileRequest { from: file.path.clone(), id: id.clone() })
                .collect();
            let sink = Arc::clone(&self.runs[ix].sink);
            let task_name = task.to_string();
            // The scan's last beat is stale the moment the stage turns:
            // the pill waits on the copy phase's own first beat.
            self.runs[ix].latest = None;
            self.runs[ix].stage = Stage::Storing { plan: Box::new(plan) };
            Task::perform(
                async move { store::store_books(&task_name, &requests, &sink) },
                move |results| Message::CopiesDone(task, results),
            )
        } else {
            self.runs.remove(ix);
            let landed = self.land_folder_walk(plan, None);
            Task::batch([landed, self.release_root(root)])
        }
    }
}
