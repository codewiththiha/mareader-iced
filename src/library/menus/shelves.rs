//! The menus of a shelf-shaped target: the shelf row itself, and the level's own
//! floor, which offers a new shelf and the way in or out of the selection.

use iced::{Element, Size};

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};
use super::{selection_row, CONTEXT_W, SHELF_W};

/// The shelf's own menu, hung off the last crumb: rename it, or take it
/// apart. Taking a shelf apart is not a question the web app asked twice —
/// a shelf the reader made holds no copies, so its books simply come up a
/// level and the shelf goes. Its duplicate is the reader's own second tree
/// over fresh copies of the books — the store batch rides a run of its own.
pub fn shelf_menu(tokens: Tokens, shelf: &str) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(SHELF_W);

    rows.push(menu::item(
        tokens,
        Some(IconName::Type),
        "Rename…",
        None,
        false,
        Some(Message::StartRename),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Copy),
        "Duplicate",
        None,
        false,
        Some(Message::DuplicateShelf(shelf.to_string())),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Close),
        "Remove shelf",
        None,
        false,
        Some(Message::RemoveShelf),
    ));
    size = size.row(menu::ROW_H);

    (menu::popover(tokens, rows, SHELF_W), size.size())
}

/// Right-click on the level's own floor: make a shelf here, and — when the
/// level holds anything — the selection's two doors, which trade places
/// with the mode the reader is in.
pub fn level_menu(
    tokens: Tokens,
    selecting: bool,
    anything: bool,
) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(CONTEXT_W);

    rows.push(menu::item(
        tokens,
        Some(IconName::Plus),
        "New shelf",
        None,
        false,
        Some(Message::CreateShelf),
    ));
    size = size.row(menu::ROW_H);

    if anything {
        let face = selection_row(selecting);
        rows.push(menu::item(tokens, Some(face.icon), face.label, None, false, Some(face.message)));
        size = size.row(menu::ROW_H);
    }

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}
