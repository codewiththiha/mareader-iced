//! Bringing files in: the picker's answer, and the sheet that asks where they
//! belong when the standing tree already reads their ground.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::sheets::Sheet;
use crate::app::walk::{FsRun, Stage};
use crate::platform::{fs, progress};
use iced::Task;
use library_core::folder::FolderOpts;
use library_core::paths;
use library_core::shelf::ALL_SHELF;
use std::path::PathBuf;

impl Mareader {
    /// The pickers' and the drop's answer: measure the files, then land
    /// them the way the library holds loose arrivals — as its own stored
    /// copies, except the ones a read-at-place tree answers for, which come
    /// back as that tree's linked books.
    pub(in crate::app) fn import_files(&mut self, picked: Option<Vec<PathBuf>>) -> Task<Message> {
        self.menu = None;
        let Some(paths) = picked else { return Task::none() };
        if paths.is_empty() {
            return Task::none();
        }
        let addresses: Vec<String> =
            paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        let label = if addresses.len() == 1 {
            paths::file_name(&addresses[0])
        } else {
            format!("{} files", addresses.len())
        };
        // The level the pick lands its books on, captured at the ask: a
        // reader who steps away during the copy does not move the landing.
        let target = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink,
            rx,
            latest: None,
            stage: Stage::Measuring { target },
        });
        Task::perform(
            async move { fs::check_paths(&addresses) },
            move |checks| Message::FilesChecked(task, checks),
        )
    }

    /// Open the import sheet for a picked or dropped folder, seeded from
    /// the ground: a folder a tree governs opens with the tree's own
    /// answers and the rung's watch, so the sheet asks nothing the tree
    /// already answered. A ground no tree governs keeps the last answers,
    /// the way the open floor does.
    pub(in crate::app) fn open_import_sheet(&mut self, dir: PathBuf) {
        let ground = self.ground_tracking(&dir.to_string_lossy());
        if let Some(watch) = &ground {
            self.import_opts = FolderOpts { watch: watch.on, ..watch.opts.clone() };
        }
        self.sheet = Some(Sheet::Import { root: dir, ground });
    }
}
