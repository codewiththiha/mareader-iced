//! One row of the level: its hover and press, the drag its hold lifts, the
//! right-click's menu, the reveal, the duplicate, and opening a book.
use std::path::PathBuf;

use iced::time::{Duration, Instant};
use iced::Task;

use library_core::book;

use crate::app::message::{ContextRequest, ContextTarget, Message};
use crate::app::selection::{Family, Hot, Press};
use crate::app::sheets::Sheet;
use crate::app::Mareader;
use crate::library::drag::Band;
use crate::platform::fs;
use crate::route::Route;
use crate::ui::toast::Tone;

/// The fold panel's close waits out a diagonal crossing of its corner.
const ELLIPSIS_GRACE_MS: u64 = 220;

impl Mareader {
    pub(super) fn duplicate(&mut self, id: String) -> Task<Message> {
        // The menus close with the ask; the run's card is watched next.
        self.menu = None;
        self.context = None;
        self.dup_queue.push(id);
        self.pump_dup()
    }

    pub(super) fn duplicate_selection(&mut self) -> Task<Message> {
        self.context = None;
        self.dup_queue.extend(self.selected.iter().cloned());
        self.pump_dup()
    }

    pub(super) fn context_menu(&mut self, target: ContextTarget) -> Task<Message> {
        // A drag in flight owns the pointer: the right-click is its cancel.
        if self.drag.is_some() {
            return Task::none();
        }
        self.menu = None;
        self.menu_confirm = None;
        self.context = Some(ContextRequest { target, at: self.cursor });
        Task::none()
    }

    pub(super) fn reveal_row(&mut self, id: String) -> Task<Message> {
        self.context = None;
        let path = book::find_row(&self.library.books, &id)
            .and_then(|row| match row {
                book::Row::Book(book) => Some(book.path().to_string()),
                book::Row::Link { .. } => None,
            });
        match path {
            Some(address) => {
                if let Err(error) = fs::reveal(&address) {
                    self.toasts.show(Tone::Error, error, Instant::now());
                }
            }
            None => self.toasts.show(
                Tone::Info,
                "A link has no place on disk to show",
                Instant::now(),
            ),
        }
        Task::none()
    }

    pub(super) fn ask_remove_row(&mut self, id: String) -> Task<Message> {
        self.context = None;
        let name = book::find_row(&self.library.books, &id)
            .map(|row| match row {
                book::Row::Book(book) => book.title(),
                book::Row::Link { name, .. } => name.clone(),
            })
            .unwrap_or_default();
        self.sheet = Some(Sheet::Remove { id, name });
        Task::none()
    }

    pub(super) fn open_library_book(&mut self, id: String) -> Task<Message> {
        self.menu = None;
        self.context = None;
        self.exit_selection();
        let Some(path) = book::find_by_id(&self.library.books, &id)
            .map(|book| PathBuf::from(book.path()))
        else {
            return Task::none();
        };
        // The id rides along: a book resumes where its own reader left off.
        self.open_row(Some(id), path)
    }

    pub(super) fn card_hover(&mut self, hovered: Option<String>, now: Instant) -> Task<Message> {
        if let Some(id) = &hovered {
            self.on_hot_change(Some(Hot::Card(id.clone())), now);
            self.hovered_crumb = None;
        } else {
            self.on_hot_exit(Family::Card, now);
        }
        self.hovered_card = hovered;
        Task::none()
    }

    pub(super) fn drag_band(&mut self, id: String, band: Band, now: Instant) -> Task<Message> {
        // The band arrives after the cell's own hover, in the same event.
        self.on_hot_change(Some(Hot::Card(id.clone())), now);
        self.hovered_crumb = None;
        self.hovered_card = Some(id);
        if let Some(drag) = &mut self.drag {
            drag.band = band;
        }
        Task::none()
    }

    pub(super) fn crumb_hover(&mut self, hovered: Option<String>, now: Instant) -> Task<Message> {
        if let Some(id) = &hovered {
            self.on_hot_change(Some(Hot::Crumb(id.clone())), now);
            self.hovered_card = None;
        } else {
            self.on_hot_exit(Family::Crumb, now);
        }
        self.hovered_crumb = hovered;
        Task::none()
    }

    pub(super) fn ellipsis_hover(&mut self, over: bool, now: Instant) -> Task<Message> {
        // The leave arms a grace; a return inside it clears the deadline.
        if over {
            self.ellipsis_open = true;
            self.ellipsis_close_at = None;
            self.hovered_crumb = None;
            self.hovered_card = None;
            self.on_hot_change(Some(Hot::Ellipsis), now);
        } else {
            // A leave inside the panel's box never left the intent.
            if !self.pointer_in_panel() {
                self.ellipsis_close_at =
                    Some(now + Duration::from_millis(ELLIPSIS_GRACE_MS));
            }
            self.on_hot_exit(Family::Ellipsis, now);
        }
        Task::none()
    }

    pub(super) fn ellipsis_pressed(&mut self) -> Task<Message> {
        self.ellipsis_open = true;
        self.ellipsis_close_at = None;
        Task::none()
    }

    pub(super) fn press_started(&mut self, now: Instant) -> Task<Message> {
        // Clearing the swallow here, not at the release, keeps the order
        // of queued messages out of it.
        self.tap_swallow = None;
        // The hold starts on the shelf alone, with nothing covering it.
        if self.route == Route::Library
            && self.sheet.is_none()
            && self.menu.is_none()
            && self.context.is_none()
            && let Some(id) = self.hovered_card.clone()
        {
            self.press = Some(Press { id, at: self.cursor, started: now });
        }
        Task::none()
    }

    pub(super) fn press_ended(&mut self) -> Task<Message> {
        self.press = None;
        if self.drag.is_some() {
            return self.release_drag();
        }
        Task::none()
    }

    pub(super) fn floor_pressed(&mut self) -> Task<Message> {
        // A drag owns this release: its drop is not a press on empty ground.
        if self.drag.is_some() {
            return Task::none();
        }
        self.exit_selection();
        self.context = None;
        Task::none()
    }
}
