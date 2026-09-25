//! The choosing set: entering it, everything it can hold, the popovers that
//! file it, and the ways out of it.
use iced::Task;

use crate::app::message::Message;
use crate::app::sheets::Sheet;
use crate::app::Mareader;
use crate::library;
use crate::route::Route;

impl Mareader {
    pub(super) fn select_row(&mut self, id: String) -> Task<Message> {
        self.context = None;
        self.enter_selection(&id);
        Task::none()
    }

    pub(super) fn select_all(&mut self) -> Task<Message> {
        self.context = None;
        let folders = library::level_folders(&self.library, &self.shelf, &self.query);
        let rows = library::level_rows(&self.library, &self.shelf, &self.query);
        self.selecting = true;
        self.selected.extend(folders.into_iter().map(|shelf| shelf.id));
        self.selected.extend(rows.iter().map(|row| row.id().to_string()));
        Task::none()
    }

    pub(super) fn toggle_select_pop(&mut self) -> Task<Message> {
        self.select_pop = !self.select_pop;
        Task::none()
    }

    pub(super) fn ask_remove_selection(&mut self) -> Task<Message> {
        self.context = None;
        let (books, shelves) = self.split_selection();
        if books.is_empty() && shelves.is_empty() {
            return Task::none();
        }
        self.exit_selection();
        self.sheet = Some(Sheet::RemoveMany { books, shelves });
        Task::none()
    }

    pub(super) fn clear_selection(&mut self) -> Task<Message> {
        self.context = None;
        self.exit_selection();
        Task::none()
    }

    pub(super) fn enter_pressed(&mut self) -> Task<Message> {
        if self.route != Route::Library
            || self.sheet.is_some()
            || self.context.is_some()
            || self.menu.is_some()
        {
            return Task::none();
        }
        let Some(id) = self.hovered_card.clone() else { return Task::none() };
        if self.selecting {
            self.toggle_selected(&id);
            Task::none()
        } else {
            self.card_tap(&id)
        }
    }

    pub(super) fn shift_enter(&mut self) -> Task<Message> {
        if self.route != Route::Library
            || self.selecting
            || self.sheet.is_some()
            || self.context.is_some()
            || self.menu.is_some()
        {
            return Task::none();
        }
        let Some(id) = self.hovered_card.clone() else { return Task::none() };
        self.enter_selection(&id);
        Task::none()
    }
}
