//! Who holds a folder's root: the claim a second run asks, the two doors that
//! take it, and the release that lets the ask queued behind it start.
use super::{Asked, FsRun, RootPlan, Stage};
use crate::app::Mareader;
use crate::app::copies::QueuedImport;
use crate::app::message::Message;
use crate::platform::{fs, progress};
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::paths;
use library_core::folder::FolderOpts;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Who holds a folder's root: the claim question a second run asks before
/// it starts.
pub(in crate::app) enum Claim {
    Free,
    /// A focus walk holds it; an ask queues behind the walk's release.
    HeldByWalk,
    /// An ask holds it; a second ask is told, a walk waits for the next
    /// focus.
    HeldByAsk,
}

impl Mareader {
    /// Kick off a folder walk: one run id, one channel, one subscription
    /// that lives exactly as long as the run. Two walks of one folder are
    /// two snapshots of the same ledger row and two writes back, so the
    /// root is claimed for the length of the run — an ask outranks a
    /// rescan and queues behind it, and a second ask is told the first is
    /// still running.
    pub(in crate::app) fn begin_folder_walk(
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
}
impl Mareader {
    /// Who holds a root right now, if anyone — the runs and the queued ask
    /// behind them.
    pub(in crate::app) fn root_claim(&self, root: &str) -> Claim {
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
}
impl Mareader {
    /// A run ended: the ask queued behind its root, if one waited, starts
    /// now — the release the queued ask was promised.
    pub(in crate::app) fn release_root(&mut self, root: &str) -> Task<Message> {
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
}
