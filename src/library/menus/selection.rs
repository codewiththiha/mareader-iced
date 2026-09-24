//! The menu over a selection, and the line that trades places with the mode the
//! reader is in: a set answers about itself, not about the item under the cursor.

use iced::{Element, Size};

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};
use super::CONTEXT_W;

/// The selection row's face: a level holding a selection is offered the way
/// out of it, one holding none the way in.
pub(super) struct SelectionRow {
    pub(super) icon: IconName,
    pub(super) label: &'static str,
    pub(super) message: Message,
}

pub(super) fn selection_row(selecting: bool) -> SelectionRow {
    if selecting {
        SelectionRow { icon: IconName::Undo, label: "Clear selection", message: Message::ClearSelection }
    } else {
        SelectionRow { icon: IconName::Check, label: "Select all", message: Message::SelectAll }
    }
}

/// Right-click on an item that is in the set, while choosing: the menu
/// answers about the whole set, not the item under the cursor.
pub fn selection_menu(tokens: Tokens, count: usize) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(CONTEXT_W);

    rows.push(menu::section(tokens, format!("{count} selected")));
    size = size.row(menu::SECTION_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Plus),
        "New shelf from these",
        None,
        false,
        Some(Message::FileSelectionOnNewShelf),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Copy),
        format!("Duplicate ({count})"),
        None,
        false,
        Some(Message::DuplicateSelection),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    rows.push(menu::danger_item(
        Some(IconName::Close),
        format!("Remove ({count})"),
        Message::AskRemoveSelection,
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Undo),
        "Clear selection",
        None,
        false,
        Some(Message::ClearSelection),
    ));
    size = size.row(menu::ROW_H);

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}
