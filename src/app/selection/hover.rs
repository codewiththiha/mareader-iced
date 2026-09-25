//! The pointer over a cell: what it makes hot, how long a press has to last
//! to hold, and the tap the release becomes.

use super::{DRAG_THRESHOLD_PX, Family, Hot, SELECT_PRESS_MS, SELECT_SLOP_PX};
use crate::app::Mareader;
use crate::app::message::Message;
use crate::library::drag::Band;
use iced::Task;
use iced::time::Instant;
use library_core::book;
use library_core::shelf;
use std::path::PathBuf;

impl Mareader {
    /// What a tap means: a membership while choosing, an open otherwise.
    /// The flag a finished hold leaves behind swallows this one tap — the
    /// release still belongs to the cell's button, and the choice the hold
    /// just started must not immediately toggle the cell back out.
    pub(in crate::app) fn card_tap(&mut self, id: &str) -> Task<Message> {
        if self.tap_swallow.take().is_some_and(|swallowed| swallowed == id) {
            return Task::none();
        }
        if self.selecting {
            self.toggle_selected(id);
            return Task::none();
        }
        self.menu = None;
        self.context = None;
        // Resolved to an owned answer first: the resolve borrows the blob,
        // and the acts that follow borrow the app.
        enum Tap {
            Open { id: String, path: PathBuf },
            Shelf(String),
            Nothing,
        }
        let tap = match book::find_row(&self.library.books, id) {
            Some(book::Row::Book(book)) => Tap::Open {
                id: book.id.clone(),
                path: PathBuf::from(book.path()),
            },
            // A link opens onto the shelf it points at — when it still
            // points at one.
            Some(book::Row::Link { target, .. }) if library_core::id::is_shelf(target) => {
                Tap::Shelf(target.clone())
            }
            _ => {
                if shelf::find(&self.library.shelves, id).is_some() {
                    Tap::Shelf(id.to_string())
                } else {
                    Tap::Nothing
                }
            }
        };
        match tap {
            Tap::Open { id, path } => self.open_row(Some(id), path),
            Tap::Shelf(shelf) => {
                self.navigate_to(shelf);
                Task::none()
            }
            Tap::Nothing => Task::none(),
        }
    }

    /// The hold machine's beat: a press that travelled past the drag's
    /// threshold is no hold (the drag session that lands later owns the
    /// gesture from there); a press that stayed quiet long enough starts
    /// the choice and marks the coming tap swallowed.
    pub(in crate::app) fn on_hold_tick(&mut self, at: Instant) {
        let Some(press) = &self.press else { return };
        let moved = (self.cursor.x - press.at.x).hypot(self.cursor.y - press.at.y);
        if moved > DRAG_THRESHOLD_PX {
            // The press became a drag: the hold machine hands the cell over
            // and stops counting — the drag's own clock, the dwell, starts
            // when its hot target does.
            let id = press.id.clone();
            self.press = None;
            self.begin_drag(&id, at);
            return;
        }
        if at.duration_since(press.started).as_millis() >= SELECT_PRESS_MS && moved <= SELECT_SLOP_PX
        {
            let id = press.id.clone();
            self.press = None;
            self.tap_swallow = Some(id.clone());
            self.enter_selection(&id);
        }
    }

    /// The hold's answer: the mode is on and the cell held is its first
    /// member.
    pub(in crate::app) fn enter_selection(&mut self, id: &str) {
        self.selecting = true;
        self.selected.insert(id.to_string());
    }

    /// A drag's hot target changed: the band falls back to the middle,
    /// both dwells' clock restarts, a rest that was counting is forgotten
    /// and a ghost that was parked grows back. The same fact the cells
    /// dress by — the hover truth — decides when the clocks restart, so a
    /// pointer that circles inside one card keeps its rest and a pointer
    /// that crosses a seam does not.
    pub(in crate::app) fn on_hot_change(&mut self, next: Option<Hot>, at: Instant) {
        let Some(drag) = &mut self.drag else { return };
        if drag.last_hot == next {
            return;
        }
        drag.last_hot = next;
        drag.band = Band::Middle;
        drag.dwell_armed = false;
        drag.sunk = None;
        drag.hot_started = at;
    }

    /// An exit's clear, guarded: it lands only when the session's hot
    /// still belongs to the exiting family, because the bar's messages
    /// queue after the level's and the enter of the new hot arrives
    /// before the exit of the old one.
    pub(in crate::app) fn on_hot_exit(&mut self, family: Family, at: Instant) {
        let owns = self
            .drag
            .as_ref()
            .and_then(|drag| drag.last_hot.as_ref())
            .is_some_and(|hot| hot.family() == family);
        if owns {
            self.on_hot_change(None, at);
        }
    }

    /// The panel shut, wholesale: a chain that changed (the fold is an
    /// answer about levels that no longer show) or a drag that ended (the
    /// web intent closes with the session) both land here.
    pub(in crate::app) fn close_ellipsis(&mut self) {
        self.ellipsis_open = false;
        self.ellipsis_close_at = None;
    }
}
