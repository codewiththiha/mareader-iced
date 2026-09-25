//! Walking to a row and lighting it: the flash, the scroll, and the level the
//! way opens through.

use crate::app::message::Message;
use crate::app::{LIBRARY_SCROLL, Mareader};
use crate::library::reveal::{self, Reveal};
use crate::library;
use iced::Task;
use iced::time::Instant;
use iced::widget::{operation, scrollable};

impl Mareader {
    /// The one write a reveal is: what to light, and a new beat of its own
    /// — a second reveal of the same thing is a second reveal, so the
    /// clock the flash dies on starts over.
    fn light(&mut self, id: &str) {
        let nonce = match &self.reveal {
            Some((revealed, _)) => revealed.nonce + 1,
            None => 1,
        };
        self.reveal = Some((Reveal { id: id.to_string(), nonce }, Instant::now()));
    }

    /// The flash's own offset: where the grid or the list owes a card a
    /// light, off the same facts the views read.
    fn reveal_scroll(&self, id: &str) -> Task<Message> {
        let rows = library::level_rows(&self.library, &self.shelf, &self.query);
        let folders = library::level_folders(&self.library, &self.shelf, &self.query);
        let folders_len = folders.len();
        let index = folders
            .iter()
            .position(|each| each.id == id)
            .or_else(|| rows.iter().position(|row| row.id() == id).map(|ix| ix + folders_len));
        let Some(index) = index else {
            return Task::none();
        };
        let y = if self.library.view.is_list() {
            reveal::list_offset(index, self.shelf_viewport_h)
        } else {
            let (tracks, cell) =
                library::content_metrics(self.viewport.width, self.library.view.columns);
            reveal::grid_offset(index, tracks, cell, self.shelf_viewport_h)
        };
        operation::scroll_to(LIBRARY_SCROLL, scrollable::AbsoluteOffset { x: None, y: Some(y) })
    }

    /// The web's full reveal: navigate, scroll to the card, and light the
    /// answer where the landing put it — the web page called it the
    /// scroll-into-view and the flash, and the native floor pays both from
    /// one write.
    pub(in crate::app) fn reveal_shelf(&mut self, shelf_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_shelf(&self.library.shelves, shelf_id);
        self.light(shelf_id);
        self.reveal_scroll(shelf_id)
    }

    /// The row half of the same light: the level the card lives on, then
    /// the scroll and the ring — in that order, because the reveal's whole
    /// story is that the way and the light never disagree.
    pub(in crate::app) fn reveal_book(&mut self, book_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_row(&self.library.shelves, book_id);
        self.light(book_id);
        self.reveal_scroll(book_id)
    }

    /// "Already imported": the whole of the reveal — no row, and the
    /// reader lands where the answer is. From 3g's own answers onward
    /// every way out of that question ends on the same light.
    pub(in crate::app) fn reveal_existing(&mut self, book_id: &str) -> Task<Message> {
        self.reveal_book(book_id)
    }
}
