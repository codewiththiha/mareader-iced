//! The add door: the pickers, the walk's answer, the restore check, the
//! moved shelf's confirm face, and the progress beats of a run in flight.
use std::path::PathBuf;

use iced::Task;

use library_core::wire::{ImportProgress, PathCheck};

use crate::app::message::{MenuKind, Message, MovedAsk};
use crate::app::Mareader;
use crate::platform::dialogs;

impl Mareader {
    pub(super) fn pick_files(&mut self) -> Task<Message> {
        self.menu = None;
        dialogs::pick_files(Message::FilesPicked)
    }

    pub(super) fn pick_folder(&mut self) -> Task<Message> {
        self.menu = None;
        dialogs::pick_folder(Message::FolderPicked)
    }

    pub(super) fn folder_picked(&mut self, picked: Option<PathBuf>) -> Task<Message> {
        match picked {
        // The walk waits: the sheet answers what the import may be.
        Some(dir) => {
            self.open_import_sheet(dir);
            Task::none()
        }
        None => Task::none(),
    }
    }

    pub(super) fn pick_files_in_folder(&mut self, root: String) -> Task<Message> {
        self.menu = None;
        dialogs::pick_files_in(root, Message::FilesPicked)
    }

    pub(super) fn restore_checked(&mut self, checks: Vec<PathCheck>) -> Task<Message> {
        // The answer belongs to the panel that asked; a closed panel drops it.
        if self.menu == Some(MenuKind::Add) {
            self.restore_gone = checks
                .iter()
                .filter(|check| !check.exists)
                .map(|check| check.path.clone())
                .collect();
        }
        Task::none()
    }

    pub(super) fn confirm_moved(&mut self, ask: MovedAsk) -> Task<Message> {
        self.menu_confirm = Some(ask);
        Task::none()
    }

    pub(super) fn menu_back(&mut self) -> Task<Message> {
        self.menu_confirm = None;
        Task::none()
    }

    pub(super) fn import_progress(&mut self, beat: ImportProgress) -> Task<Message> {
        if let Some(run) =
            self.runs.iter_mut().find(|run| run.task.to_string() == beat.task)
        {
            run.latest = Some(beat);
        }
        Task::none()
    }
}
