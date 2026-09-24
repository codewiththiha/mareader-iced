//! The menu of one book row: open, choose, rename, duplicate, reveal, take out —
//! the web app's own order, kept for a link as well.

use iced::{Element, Size};
use library_core::book::Row;

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};
use super::CONTEXT_W;

/// The right-click on a book or a link: open it, choose it, rename it,
/// duplicate it, reveal it, take it out of the library — the web entry
/// menu's own order, which one list keeps for every kind of row.
pub fn row_menu(tokens: Tokens, row: &Row) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(CONTEXT_W);

    match row {
        Row::Book(book) => {
            rows.push(menu::item(
                tokens,
                Some(IconName::Open),
                "Open",
                None,
                false,
                Some(Message::OpenBook(book.id.clone())),
            ));
            size = size.row(menu::ROW_H);

            rows.push(menu::item(
                tokens,
                Some(IconName::Check),
                "Select",
                None,
                false,
                Some(Message::SelectRow(book.id.clone())),
            ));
            size = size.row(menu::ROW_H);

            rows.push(menu::item(
                tokens,
                Some(IconName::Pencil),
                "Rename…",
                None,
                false,
                Some(Message::AskRenameRow(book.id.clone())),
            ));
            size = size.row(menu::ROW_H);

            // The duplicate of a book whose address died is a duplicate of
            // nothing: the row stands disabled, the web's own off-when-dead
            // rule.
            rows.push(menu::item(
                tokens,
                Some(IconName::Copy),
                "Duplicate",
                None,
                false,
                (!book.missing).then(|| Message::DuplicateRow(book.id.clone())),
            ));
            size = size.row(menu::ROW_H);

            rows.push(menu::item(
                tokens,
                Some(IconName::Folder),
                "Reveal in folder",
                None,
                false,
                (!book.missing).then(|| Message::RevealRow(book.id.clone())),
            ));
            size = size.row(menu::ROW_H);
        }
        Row::Link { id, target, .. } => {
            // A link opens onto the shelf it points at — when it still
            // points at one.
            let open = library_core::id::is_shelf(target)
                .then(|| Message::Navigate(target.clone()));
            rows.push(menu::item(
                tokens,
                Some(IconName::Open),
                "Open shelf",
                None,
                false,
                open,
            ));
            size = size.row(menu::ROW_H);

            rows.push(menu::item(
                tokens,
                Some(IconName::Check),
                "Select",
                None,
                false,
                Some(Message::SelectRow(id.clone())),
            ));
            size = size.row(menu::ROW_H);

            rows.push(menu::item(
                tokens,
                Some(IconName::Pencil),
                "Rename…",
                None,
                false,
                Some(Message::AskRenameRow(id.clone())),
            ));
            size = size.row(menu::ROW_H);

            // A link at a book duplicates into the library's own copy of
            // what it opens; a link at a shelf stays a pointer, filed
            // beside the first.
            rows.push(menu::item(
                tokens,
                Some(IconName::Copy),
                "Duplicate",
                None,
                false,
                Some(Message::DuplicateRow(id.clone())),
            ));
            size = size.row(menu::ROW_H);
        }
    }

    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    let (label, message) = removal_row(row);
    rows.push(menu::danger_item(None, label, message));
    size = size.row(menu::ROW_H);

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}

/// The removal row a row's own shape asks for: a link is a pointer rather
/// than a book, and the question says which of the two it is about to drop.
pub(super) fn removal_row(row: &Row) -> (&'static str, Message) {
    let label = if matches!(row, Row::Link { .. }) {
        "Remove link"
    } else {
        "Remove from library"
    };
    (label, Message::AskRemoveRow(row_id_of(row)))
}

fn row_id_of(row: &Row) -> String {
    match row {
        Row::Book(book) => book.id.clone(),
        Row::Link { id, .. } => id.clone(),
    }
}
