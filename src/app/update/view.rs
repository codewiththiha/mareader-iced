//! The shelf view's settings: layout, covers, sorting, columns, and the
//! appearance cycle. Each writes the library's view and the settings file.
use iced::Task;

use library_core::sort::SortKey;
use library_core::view::{CoverFit, LibraryLayout};
use reader_core::appearance::BaseMode;

use crate::app::message::Message;
use crate::app::Mareader;

impl Mareader {
    pub(super) fn set_layout(&mut self, layout: LibraryLayout) -> Task<Message> {
        if self.library.view.layout == layout {
            self.menu = None;
            return Task::none();
        }
        self.library.view.layout = layout;
        self.view_changed()
    }

    pub(super) fn set_cover(&mut self, cover: CoverFit) -> Task<Message> {
        if self.library.view.cover == cover {
            self.menu = None;
            return Task::none();
        }
        self.library.view.cover = cover;
        self.view_changed()
    }

    pub(super) fn set_sort(&mut self, key: SortKey) -> Task<Message> {
        if self.library.view.sort == key {
            self.menu = None;
            return Task::none();
        }
        self.library.view.sort = key;
        self.view_changed()
    }

    pub(super) fn set_sort_asc(&mut self, ascending: bool) -> Task<Message> {
        if self.library.view.sort_asc == ascending {
            self.menu = None;
            return Task::none();
        }
        self.library.view.sort_asc = ascending;
        self.view_changed()
    }

    pub(super) fn step_columns(&mut self, delta: i32) -> Task<Message> {
        self.library.view.step_columns(delta);
        self.view_changed()
    }

    pub(super) fn auto_columns(&mut self) -> Task<Message> {
        if self.library.view.columns.is_none() {
            self.menu = None;
            return Task::none();
        }
        self.library.view.auto_columns();
        self.view_changed()
    }

    pub(super) fn cycle_appearance(&mut self) -> Task<Message> {
        self.settings.appearance.base = match self.settings.appearance.base {
            BaseMode::Light => BaseMode::Dark,
            BaseMode::Dark => BaseMode::Dim,
            BaseMode::Dim => BaseMode::Light,
        };
        // The live look moved, so no saved preset matches it any more.
        self.settings.active_preset = None;
        self.apply_appearance();
        self.persist_settings()
    }
}
