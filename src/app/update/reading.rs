//! The reading surface's messages: it answers everything itself, except the
//! one place the app's own route is concerned.
use iced::time::Instant;
use iced::Task;

use crate::app::message::Message;
use crate::app::Mareader;
use crate::reader;
use crate::route::Route;

impl Mareader {
    pub(super) fn close_document(&mut self, now: Instant) -> Task<Message> {
        let effects = self.reader.update(reader::Message::Close, now);
        self.route = Route::Library;
        self.menu = None;
        self.apply_reader_effects(effects)
    }

    pub(super) fn reader_message(
        &mut self,
        message: reader::Message,
        now: Instant,
    ) -> Task<Message> {
        let effects = self.reader.update(message, now);
        self.apply_reader_effects(effects)
    }
}
