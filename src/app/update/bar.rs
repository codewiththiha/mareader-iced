//! The bar: its panels, the search pill, the breadcrumb's rename field, the
//! Escape key's order, and the shelf's own rename and removal.
use iced::time::Instant;
use iced::widget::operation;
use iced::Task;

use library_core::shelf::{self, ALL_SHELF};

use crate::app::message::{MenuKind, Message};
use crate::app::Mareader;
use crate::library::bar;
use crate::storage;
use crate::ui::toast::Tone;

impl Mareader {
    pub(super) fn query_changed(&mut self, terms: String) -> Task<Message> {
        self.menu = None;
        self.query = terms;
        Task::none()
    }

    pub(super) fn toggle_menu(&mut self, kind: MenuKind) -> Task<Message> {
        self.menu = if self.menu == Some(kind) { None } else { Some(kind) };
        self.renaming = false;
        self.menu_confirm = None;
        // The add panel's Restore section promises files: check them.
        if self.menu == Some(MenuKind::Add) {
            return self.check_restore_paths();
        }
        Task::none()
    }

    pub(super) fn close_menu(&mut self) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        self.renaming = false;
        Task::none()
    }

    pub(super) fn escape_pressed(&mut self) -> Task<Message> {
        // Escape closes the topmost thing, a drag first: cancelling one
        // lands its payload nowhere and leaves the choice that lifted it.
        if self.drag.is_some() {
            self.drag = None;
            self.close_ellipsis();
            return Task::none();
        }
        if self.sheet.is_some() {
            self.dismiss_sheet();
            self.advance_conflict();
            return Task::none();
        }
        if self.context.is_some() {
            self.context = None;
            return Task::none();
        }
        if self.ellipsis_open {
            self.close_ellipsis();
            return Task::none();
        }
        if self.renaming {
            self.renaming = false;
            return Task::none();
        }
        if self.selecting {
            self.exit_selection();
            return Task::none();
        }
        self.menu = None;
        self.menu_confirm = None;
        Task::none()
    }

    pub(super) fn start_rename(&mut self) -> Task<Message> {
        self.menu = None;
        self.rename_draft = shelf::find(&self.library.shelves, &self.shelf)
            .map(|shelf| shelf.name.clone())
            .unwrap_or_default();
        self.renaming = true;
        // The field appears in the next view; focus lands with it.
        operation::focus(bar::RENAME_INPUT)
    }

    pub(super) fn rename_draft(&mut self, text: String) -> Task<Message> {
        self.rename_draft = text;
        Task::none()
    }

    pub(super) fn commit_rename(&mut self) -> Task<Message> {
        self.renaming = false;
        let name = self.rename_draft.trim().to_string();
        if name.is_empty() {
            return Task::none();
        }
        if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &self.shelf) {
            shelf.name = name;
        }
        self.persist_library()
    }

    pub(super) fn take_apart(&mut self, id: String) -> Task<Message> {
        self.context = None;
        self.menu = None;
        self.ask_shelf_apart(&id)
    }

    pub(super) fn reload_library(&mut self, now: Instant) -> Task<Message> {
        self.menu = None;
        self.exit_selection();
        self.library = storage::load_library();
        if self.shelf != ALL_SHELF
            && shelf::find(&self.library.shelves, &self.shelf).is_none()
        {
            self.shelf = ALL_SHELF.to_string();
        }
        self.toasts.show(Tone::Info, "Reloaded the library from disk", now);
        Task::none()
    }
}
