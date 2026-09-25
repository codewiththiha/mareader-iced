//! The remove sheets: one row, or the chosen set — books and folders alike.

use crate::app::Mareader;
use crate::ui::sheet;
use iced::Element;
use iced::widget::text;
use library_core::text as lib_text;
use crate::app::message::Message;


impl Mareader {
    pub(super) fn remove_panel(&self, name: &str) -> Element<'_, Message> {
        let body = text(format!(
            "Remove “{name}” from the library? {}",
            "The file on disk stays where it is."
        ))
        .size(13)
        .color(self.tokens.muted);
        sheet::panel(
            self.tokens,
            "Remove",
            body.into(),
            vec![
                sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
            ],
        )
    }

    pub(super) fn remove_many_panel(&self, books: &[String], shelves: &[String]) -> Element<'_, Message> {
        let mut parts: Vec<String> = Vec::new();
        if !books.is_empty() {
            parts.push(lib_text::plural(books.len(), "book", "books"));
        }
        if !shelves.is_empty() {
            parts.push(lib_text::plural(shelves.len(), "shelf", "shelves"));
        }
        let body = text(format!(
            "Remove {} from the library? {}",
            parts.join(" and "),
            "The files on disk stay where they are."
        ))
        .size(13)
        .color(self.tokens.muted);
        sheet::panel(
            self.tokens,
            "Remove",
            body.into(),
            vec![
                sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
            ],
        )
    }
}
