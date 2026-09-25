//! The window the app lives in: its identity and events, the scale factor the
//! grid snaps against, the pointer, the frames, and the titlebar's chrome.
use iced::time::Instant;
use iced::widget::scrollable;
use iced::{window, Point, Task};

use crate::app::message::Message;
use crate::app::{Mareader, FLASH_DWELL};
use crate::chrome::titlebar;
use crate::reader;

impl Mareader {
    pub(super) fn window_found(&mut self, id: Option<window::Id>) -> Task<Message> {
        if self.window.is_none() {
            self.window = id;
        }
        // Ask for the scale factor it opened with — the grid snaps
        // against it from the first frame — and start the boot pass.
        let scale = match id {
            Some(id) => window::scale_factor(id).map(Message::ScaleFactor),
            None => Task::none(),
        };
        Task::batch([scale, self.start_measure_pass()])
    }

    pub(super) fn window_maximized(&mut self, maximized: bool) -> Task<Message> {
        self.maximized = maximized;
        Task::none()
    }

    pub(super) fn scale_factor_changed(&mut self, factor: f32) -> Task<Message> {
        self.set_scale_factor(factor);
        Task::none()
    }

    pub(super) fn cursor_moved(&mut self, position: Option<Point>, now: Instant) -> Task<Message> {
        if let Some(position) = position {
            self.cursor = position;
        }
        self.titlebar.on_cursor(position, self.route, now);
        Task::none()
    }

    pub(super) fn tick(&mut self, at: Instant) -> Task<Message> {
        self.titlebar.on_tick(self.route, at);
        self.toasts.on_tick(at);
        self.on_hold_tick(at);
        self.on_drag_tick(at);
        // The reveal's light wears off on its own clock.
        let reveal_expired = matches!(
            &self.reveal,
            Some((_, started)) if at.duration_since(*started) >= FLASH_DWELL
        );
        if reveal_expired {
            self.reveal = None;
        }
        // A cancelled grace cleared the deadline; this one ran out.
        if let Some(due) = self.ellipsis_close_at
            && at >= due
        {
            self.ellipsis_close_at = None;
            self.ellipsis_open = false;
        }
        // Frames drive a zoom in flight; a still reader costs no redraws.
        let effects = self.reader.update(reader::Message::Tick, at);
        self.apply_reader_effects(effects)
    }

    pub(super) fn shelf_viewport(&mut self, viewport: scrollable::Viewport) -> Task<Message> {
        self.shelf_viewport_h = viewport.bounds().height;
        Task::none()
    }

    pub(super) fn chrome_toggle_pin(&mut self, now: Instant) -> Task<Message> {
        self.titlebar.toggle_pin(self.route, now);
        Task::none()
    }

    pub(super) fn chrome_window_action(&mut self, action: titlebar::WindowAction) -> Task<Message> {
        let Some(id) = self.window else { return Task::none() };
        match action {
            titlebar::WindowAction::Drag => window::drag(id),
            titlebar::WindowAction::Minimize => window::minimize(id, true),
            // The answer comes back as a query, not a tracked toggle.
            titlebar::WindowAction::ToggleMaximize => Task::batch([
                window::toggle_maximize(id),
                window::is_maximized(id).map(Message::Maximized),
            ]),
            titlebar::WindowAction::Close => window::close(id),
        }
    }
}
