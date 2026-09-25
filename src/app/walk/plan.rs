//! What a walk answers: the rows it adds, the moves the ledger healed, and the
//! questions its landing owes — planned against the library it just read.
use super::{Stage, write_folder_row};
use crate::app::Mareader;
use crate::app::copies::PendingCopy;
use crate::app::message::Message;
use crate::app::sheets::{Asked, LitNote, Sheet};
use crate::library::conflicts::{self, ConflictAsk};
use crate::platform::{now_ms, store};
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::paths;
use library_core::book::{self, Fingerprint};
use library_core::folder::{FolderOpts, WatchedFolder};
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::wire::BookFileRequest;
use std::collections::HashSet;
use std::sync::Arc;

/// The folder walk's answer, planned against the ledger and waiting for its
/// copies (or landing straight away when the books stay at place).
pub(in crate::app) struct WalkPlan {
    /// The ledger row, resolved against the library as the walk ended —
    /// placed marks, spent tombstones and the shelf map are written onto it
    /// as the landing runs, and it goes back whole at the end.
    pub(in crate::app) folder: WatchedFolder,
    pub(in crate::app) asked: Asked,
    /// The covered walk's light — the shelf its own pick names, kept with
    /// the plan until the landing answers for it.
    pub(in crate::app) continuation: Option<Continuation>,
    /// The folder's display name, for the toasts.
    pub(in crate::app) root_name: String,
    /// The deduped name a fresh root rung wears; `None` when the map already
    /// holds the root shelf.
    pub(in crate::app) planned_root: Option<String>,
    /// The additions, each wearing the book id it will land as.
    pub(in crate::app) adds: Vec<(String, FoundFile)>,
    /// The moves the ledger healed: book id to its new address.
    pub(in crate::app) relinks: Vec<(String, String)>,
    /// The per-file questions a merge owes, asked only once the landing is
    /// done — a file asked about must not stand among the landed rows.
    pub(in crate::app) asks: Vec<ConflictAsk>,
    /// The rows a planned tree re-seats: the row, and the file whose rung
    /// decides where it lands.
    pub(in crate::app) replacements: Vec<(String, FoundFile)>,
    /// The addresses whose file the library already reads in place, where
    /// this run lands its own copy beside the linked row that reads it.
    pub(in crate::app) copy_paths: HashSet<String>,
    /// The rows a moved-out log already answers for: the reader has these
    /// books, so the walk lights them up rather than landing neighbours.
    pub(in crate::app) represented: Vec<String>,
}

/// The walk's own root answer: *as new* names the tree something else and
/// continues from a fresh row, so the old row keeps answering its shelf;
/// *merge* files the new scans into the shelf that already holds the name.
#[derive(Clone, Debug, Default)]
pub(in crate::app) struct RootPlan {
    pub(in crate::app) rename: Option<String>,
    pub(in crate::app) into: Option<String>,
    /// The covered walk's light: the shelf its pick stood on, and the name
    /// its ledger row wears — the re-import's own run answers to them both.
    pub(in crate::app) continuation: Option<Continuation>,
}

/// What the covered walk's own light knows: where the pick's own shelf
/// stands, and which name the session opened with. The landing owes it an
/// answer even when nothing new was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct Continuation {
    pub(in crate::app) shelf_id: String,
    pub(in crate::app) name: String,
}

/// The loose-file run's answer, waiting for its copies.
pub(in crate::app) struct FilesPlan {
    /// The level the pick lands its books on, when one is standing.
    pub(in crate::app) target: Option<String>,
    pub(in crate::app) pending: Vec<PendingCopy>,
    /// Covered files restored as their folder's linked books, counted for
    /// the toast.
    pub(in crate::app) restored: usize,
    /// The questions the screens raised, riding along so they are asked
    /// only once the copies have landed: a sheet answered while its own
    /// copy is still in flight would land beside a ghost.
    pub(in crate::app) asks: Vec<ConflictAsk>,
    /// The books the drop found the reader already had, by a folder's own
    /// moved-out log: the light lands on the first of them.
    pub(in crate::app) represented: Vec<String>,
    /// The folder ledger an answered file settles when its copy comes
    /// home — the answer's own placement record, absent for a pick's run.
    pub(in crate::app) settle: Option<(String, Fingerprint)>,
    /// The slot the answered file takes, when an answer named one.
    pub(in crate::app) index: Option<usize>,
}

impl Mareader {
    /// The diff stage: everything between the walk's raw findings and the
    /// landing. Decides against the ledger the crate owns — tombstones
    /// first, then the registry, then the rung's tracking answer — and
    /// writes nothing but the folder's own row.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn plan_folder_walk(
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
