//! The rename sheet: the name being written, and the write that keeps it.

use super::{Mareader, RenameKind, Sheet, SHEET_INPUT};
use crate::ui::sheet;
use iced::{Background, Border, Element, Length, Padding, Task};
use iced::widget::{operation, text_input};
use library_core::book;
use library_core::shelf;
use crate::app::message::Message;

impl Mareader {
    pub(in crate::app) fn ask_rename(&mut self, kind: RenameKind, id: String) -> Task<Message> {
            self.context = None;
            let draft = match kind {
                RenameKind::Row => book::find_row(&self.library.books, &id)
                    .map(|row| match row {
                        book::Row::Book(book) => book.title(),
                        book::Row::Link { name, .. } => name.clone(),
                    })
                    .unwrap_or_default(),
                RenameKind::Shelf => shelf::find(&self.library.shelves, &id)
                    .map(|shelf| shelf.name.clone())
                    .unwrap_or_default(),
            };
            self.sheet = Some(Sheet::Rename { kind, id, draft });
            // The field appears in the next view; focus lands with it.
            operation::focus(SHEET_INPUT)
        }

    pub(super) fn rename_panel(&self, draft: &str) -> Element<'_, Message> {
        let input = text_input("Name", draft)
            .id(SHEET_INPUT)
            .on_input(Message::SheetDraft)
            .on_submit(Message::SheetSave)
            .size(13)
            .width(Length::Fill)
            .padding(Padding { top: 7.0, right: 10.0, bottom: 7.0, left: 10.0 })
            .style(move |_theme, _status| text_input::Style {
                background: Background::Color(self.tokens.paper),
                border: Border {
                    color: self.tokens.line,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                icon: self.tokens.muted,
                placeholder: self.tokens.muted,
                value: self.tokens.ink,
                selection: self.tokens.accent_soft,
            });
        sheet::panel(
            self.tokens,
            "Rename",
            input.into(),
            vec![
                sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                sheet::confirm_button(self.tokens, "Save", Message::SheetSave, false),
            ],
        )
    }

    pub(super) fn save_rename(&mut self, kind: RenameKind, id: String, draft: String) -> Task<Message> {
        let name = draft.trim().to_string();
        if name.is_empty() {
            return Task::none();
        }
        match kind {
            RenameKind::Row => {
                if let Some(row) = book::find_row_mut(&mut self.library.books, &id) {
                    match row {
                        book::Row::Book(book) => {
                            book.title = Some(name);
                            book.title_locked = true;
                        }
                        book::Row::Link { name: own, .. } => *own = name,
                    }
                }
            }
            RenameKind::Shelf => {
                if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &id) {
                    shelf.name = name;
                }
            }
        }
        self.persist_library()
    }
}
