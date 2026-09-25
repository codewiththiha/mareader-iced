//! The sheets' own messages: the drafts they hold, the answers they collect,
//! and the import sheet's options.
use std::path::PathBuf;

use iced::Task;

use library_core::conflict::Placement;
use library_core::folder::FolderMode;
use reader_core::format::Format;

use crate::app::message::Message;
use crate::app::sheets::Sheet;
use crate::app::walk::{Asked, RootPlan};
use crate::app::Mareader;
use crate::library::conflicts;
use crate::library::departure::{CopyAnswer, CopyWork};

impl Mareader {
    pub(super) fn sheet_draft(&mut self, text: String) -> Task<Message> {
        if let Some(Sheet::Rename { draft, .. }) = &mut self.sheet {
            *draft = text;
        }
        Task::none()
    }

    pub(super) fn sheet_cancel(&mut self) -> Task<Message> {
        if let Some(Sheet::AlreadyImported { note }) = self.sheet.take() {
            self.advance_conflict();
            return self.reveal_shelf(&note.shelf_id);
        }
        self.dismiss_sheet();
        self.advance_conflict();
        Task::none()
    }

    pub(super) fn close_already_imported(&mut self) -> Task<Message> {
        let Some(Sheet::AlreadyImported { note }) = self.sheet.take() else {
            return Task::none();
        };
        self.advance_conflict();
        self.reveal_shelf(&note.shelf_id)
    }

    pub(super) fn answer_copy(&mut self, answer: CopyAnswer) -> Task<Message> {
        let Some(Sheet::Copy { ask }) = self.sheet.take() else {
            return Task::none();
        };
        match answer {
            // Nothing moves and nothing copies; the queue behind shows.
            CopyAnswer::Cancel => {
                self.advance_conflict();
                Task::none()
            }
            // No copies bought: a shelf with a way home takes it, and a
            // removal runs as its button promised.
            CopyAnswer::WithoutCopies => {
                let task = match ask.work {
                    CopyWork::Shelf { returns, .. } => {
                        if self.take_them_home(&returns) {
                            self.persist_library()
                        } else {
                            Task::none()
                        }
                    }
                    CopyWork::Removal { purge, shelves } => {
                        if self.remove(purge, shelves) {
                            self.persist_library()
                        } else {
                            Task::none()
                        }
                    }
                    CopyWork::Rows { .. } | CopyWork::Rung { .. } => Task::none(),
                };
                self.advance_conflict();
                task
            }
            CopyAnswer::Copy => self.copy_and_finish(ask),
        }
    }

    pub(super) fn answer_placement(&mut self, choice: Placement, all: bool) -> Task<Message> {
        let Some(Sheet::Conflict { mut ask }) = self.sheet.take() else {
            return Task::none();
        };
        let mut tasks: Vec<Task<Message>> = Vec::new();
        loop {
            // An arrival naming no row has nothing to write.
            if ask.arrival.moving.is_some() || ask.arrival.is_import() {
                let task = match &ask.kind {
                    conflicts::AskKind::FolderMerge { .. } => {
                        self.apply_folder_merge(&ask, choice)
                    }
                    _ => self.apply_placement(&ask, choice),
                };
                tasks.push(task);
            }
            self.advance_conflict();
            if !all {
                break;
            }
            // Apply-to-all answers every waiting question of this kind;
            // another kind keeps its own sheet.
            let Some(Sheet::Conflict { ask: next }) = self.sheet.take() else {
                break;
            };
            let same = (ask.kind.is_two_answer() && next.kind.is_two_answer())
                || (ask.kind.is_folder_merge() && next.kind.is_folder_merge());
            if !same {
                self.sheet = Some(Sheet::Conflict { ask: next });
                break;
            }
            ask = next;
        }
        match tasks.len() {
            0 => Task::none(),
            1 => tasks.pop().unwrap_or_else(Task::none),
            _ => Task::batch(tasks),
        }
    }

    pub(super) fn answer_shelf(&mut self, placement: Placement) -> Task<Message> {
        let Some(Sheet::ShelfConflict { ask }) = self.sheet.take() else {
            return Task::none();
        };
        if !conflicts::shelf_offers(&ask).contains(&placement) {
            return Task::none();
        }
        match placement {
            Placement::Open => self.reveal_shelf(&ask.existing_id),
            Placement::LinkOnly => self.link_to_existing(&ask),
            Placement::Merge => self.begin_folder_walk(
                PathBuf::from(&ask.root),
                ask.opts.clone(),
                Asked::Explicitly,
                RootPlan {
                    rename: None,
                    into: Some(ask.existing_id.clone()),
                    ..RootPlan::default()
                },
            ),
            Placement::KeepBoth => self.copies_beside_tree_run(ask),
            Placement::Replace => self.replace_with_tree(ask),
        }
    }

    pub(super) fn toggle_apply_all(&mut self) -> Task<Message> {
        self.apply_all = !self.apply_all;
        Task::none()
    }

    pub(super) fn sheet_import_format(&mut self, format: Format) -> Task<Message> {
        if !self.import_opts.formats.remove(&format) {
            self.import_opts.formats.insert(format);
        }
        Task::none()
    }

    pub(super) fn sheet_import_include(&mut self, include: bool) -> Task<Message> {
        self.import_opts.include_selected = include;
        Task::none()
    }

    pub(super) fn sheet_import_size(&mut self, delta: i32) -> Task<Message> {
        self.import_opts.step_min_size(delta);
        Task::none()
    }

    pub(super) fn sheet_import_mode(&mut self, mode: FolderMode) -> Task<Message> {
        // Both switches are written from the mode: no watching copy is
        // offerable.
        self.import_opts.in_place = mode.reads_in_place();
        self.import_opts.watch = mode.tracks_new_files();
        Task::none()
    }

    pub(super) fn sheet_import_groups(&mut self, groups: bool) -> Task<Message> {
        self.import_opts.groups = groups;
        Task::none()
    }
}
