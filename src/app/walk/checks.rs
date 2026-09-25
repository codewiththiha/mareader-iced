//! The library's two automatic measurements: the pass over every address it
//! holds, and the walk of every watched folder that follows it.
use crate::app::Mareader;
use crate::app::message::Message;
use crate::platform::fs;
use iced::Task;
use library_core::paths;
use library_core::book::{self};
use library_core::scan::FoundFile;
use library_core::wire::PathCheck;
use reader_core::format::is_supported_path;

/// The found file a path check describes, when the check found a document:
/// the loose-file run's translation from measurement to ledger row.
pub(in crate::app) fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
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

impl Mareader {
    /// The library's two automatic measurements, in the one order they owe:
    /// first a pass over every address the library holds — marking books
    /// missing when their file was deleted or moved, and healing the
    /// fingerprints a migration left pending — then the walk of every
    /// watched folder. The walk only ever sees what the measure pass made
    /// legible.
    pub(in crate::app) fn start_measure_pass(&mut self) -> Task<Message> {
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
}
impl Mareader {
    pub(in crate::app) fn checks_done(&mut self, checks: Vec<PathCheck>) -> Task<Message> {
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
}
