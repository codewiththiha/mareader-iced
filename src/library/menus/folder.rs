//! The menu of a folder shelf: stand on it, choose it, rename it, duplicate it,
//! answer for its watch, shelf inside it, or take it apart.

use iced::{Element, Size};
use library_core::shelf::Shelf;

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};
use super::CONTEXT_W;

/// The watch row's face: the toggle the folder context menu shows, with its
/// message already aimed at the seat's shelf.
pub struct WatchFacts {
    pub icon: IconName,
    pub label: String,
    pub sublabel: String,
    pub message: Message,
}

/// The right-click on a folder shelf: stand on it, choose it, rename it,
/// duplicate it, answer for its watch, mint a shelf inside it, take it
/// apart. `watch` is the folder's watch seat — the toggle row a folder
/// shelf has and nothing else does, because nothing else has a rung to
/// answer for.
pub fn folder_menu(
    tokens: Tokens,
    shelf: &Shelf,
    watch: Option<WatchFacts>,
) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(CONTEXT_W);

    rows.push(menu::item(
        tokens,
        Some(IconName::Open),
        "Open shelf",
        None,
        false,
        Some(Message::Navigate(shelf.id.clone())),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Check),
        "Select",
        None,
        false,
        Some(Message::SelectRow(shelf.id.clone())),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Pencil),
        "Rename…",
        None,
        false,
        Some(Message::AskRenameShelf(shelf.id.clone())),
    ));
    size = size.row(menu::ROW_H);

    // A second tree of the reader's own, holding fresh copies of the books:
    // never a second door onto the same rows, and never a second shelf of
    // one directory — the copy of a folder shelf is virtual like any other.
    rows.push(menu::item(
        tokens,
        Some(IconName::Copy),
        "Duplicate",
        None,
        false,
        Some(Message::DuplicateShelf(shelf.id.clone())),
    ));
    size = size.row(menu::ROW_H);

    if let Some(watch) = watch {
        rows.push(menu::item(
            tokens,
            Some(watch.icon),
            watch.label,
            Some(watch.sublabel.into()),
            false,
            Some(watch.message),
        ));
        size = size.row(menu::TALL_ROW_H);
    }

    rows.push(menu::item(
        tokens,
        Some(IconName::Plus),
        "New shelf",
        None,
        false,
        Some(Message::NewShelfInside(shelf.id.clone())),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    rows.push(menu::danger_item(None, "Take shelf apart", Message::TakeApart(shelf.id.clone())));
    size = size.row(menu::ROW_H);

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}
