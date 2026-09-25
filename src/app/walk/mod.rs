//! The folder walk: claiming a folder's root, planning a run against its ledger,
//! and the rungs its files land on.
use std::path::PathBuf;

use iced::Task;

use library_core::folder::{FolderOpts, Tombstone};
use library_core::scan::FoundFile;

use crate::app::copies::{CopiesDest, CopiesWork, DupWork};
use crate::app::message::Message;
use crate::app::moves::DepartWork;
use crate::platform::dialogs;
use super::Mareader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Which question a folder run answers; a boolean at the signature could
/// not say. An ask is the reader's own import; a walk is the app keeping a
/// watched folder's promise to itself.
pub(in crate::app) enum Asked {
    Explicitly,
    OnFocus,
}

mod checks;
mod claim;
mod plan;
mod run;
mod rungs;
mod watch;

pub(in crate::app) use checks::{found_from_check};
pub(in crate::app) use claim::{Claim};
pub(in crate::app) use plan::{Continuation, FilesPlan, RootPlan, WalkPlan};
pub(in crate::app) use run::{FsRun};
pub(in crate::app) use rungs::{chain_for, flatten_rungs, member_rungs, page_into, rel_of, write_folder_row};
pub(in crate::app) use watch::{GroundWatch, ShelfWatch};

/// Where a run is in its life, and the plan its next stage lands. The plans
/// ride in boxes: a run is a small thing the state tree holds many of, and
/// a walk's plan — a whole ledger row among its fields — would otherwise
/// decide the size of every stage.
pub(in crate::app) enum Stage {
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

impl Mareader {
    /// The walk of every folder that owes one. Held back while a migrated
    /// book still wears a placeholder fingerprint — scanning against
    /// unmeasured identities would re-add every one of them — and while the
    /// focus is the app's own picker closing.
    pub(super) fn run_watched(&mut self) -> Task<Message> {
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
}
