//! A filesystem run in flight: its channel, its beats, and the answer that
//! turns into a plan.
use super::{Asked, Stage};
use crate::app::Mareader;
use crate::app::message::Message;
use crate::platform::progress;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::scan::FoundFile;
use library_core::wire::ImportProgress;

/// One filesystem run and its channel: the id the beats carry, the label
/// the dock pill names it by, the parked receiver the subscription takes
/// on first build, the sender the run's later stages reuse, and the latest
/// beat — the pill's whole input.
pub(in crate::app) struct FsRun {
    pub(in crate::app) task: u64,
    pub(in crate::app) label: String,
    pub(in crate::app) sink: progress::ProgressSink,
    pub(in crate::app) rx: progress::SharedProgress,
    pub(in crate::app) latest: Option<ImportProgress>,
    pub(in crate::app) stage: Stage,
}

impl Mareader {
    /// The walk's answer arrived: plan it against the ledger, and either
    /// land it (the books stay at place) or hand the additions to the store
    /// first (the books are copied).
    pub(in crate::app) fn scan_done(
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
}
