//! The ＋ menu: add a book or a folder, and the folder, restore and confirm rows
//! it can answer with.

use std::borrow::Cow;

use iced::widget::{container, text};
use iced::{Element, Length, Padding, Size};

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};
use super::{ADD_W, MenuLine};

/// The add menu's folder-side facts, computed when the panel is built.
pub struct AddFacts {
    /// The "choose files from this folder" door, when the level the reader
    /// stands on is a watched folder's.
    pub from_folder: Option<MenuLine>,
    /// The Restore section's rows — books removed from this folder and
    /// books still inside it on disk but filed elsewhere — empty when the
    /// folder offers nothing back.
    pub restore: Vec<MenuLine>,
    /// The two-choice face a Moved row swaps the whole panel into.
    pub confirm: Option<ConfirmFace>,
}

/// The Moved answer: one book, two shelves — or follow it to where it went.
pub struct ConfirmFace {
    pub back: MenuLine,
    /// The question line between the rows: `"{title}" is on another shelf.`
    pub question: String,
    pub also: MenuLine,
    pub go: MenuLine,
}

/// The level bar's plus: two import doors, and — when the level the reader
/// stands on is a watched folder's — the folder's own doors: browse to it
/// for files, and give back what it once held.
///
/// The Restore section is the ledger's answer to "this folder could give
/// something back": books removed from it and books still inside it on disk
/// but filed on shelves elsewhere. A Moved row does not act; it swaps the
/// panel into [`ConfirmFace`], the two-choice answer — show it here as
/// well, or go look at where it went.
pub fn add_menu(tokens: Tokens, facts: &AddFacts) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(ADD_W);

    if let Some(confirm) = &facts.confirm {
        rows.push(menu::item(
            tokens,
            Some(confirm.back.icon),
            confirm.back.label.clone(),
            confirm.back.sublabel.clone().map(Cow::Owned),
            false,
            confirm.back.message.clone(),
        ));
        size = size.row(menu::ROW_H);

        rows.push(menu::separator(tokens));
        size = size.row(menu::SEP_H);

        rows.push(
            container(text(confirm.question.clone()).size(12).color(tokens.muted))
                .width(Length::Fill)
                .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
                .into(),
        );
        size = size.row(menu::ROW_H);

        rows.push(menu::item(
            tokens,
            Some(confirm.also.icon),
            confirm.also.label.clone(),
            confirm.also.sublabel.clone().map(Cow::Owned),
            false,
            confirm.also.message.clone(),
        ));
        size = size.row(menu::TALL_ROW_H);

        rows.push(menu::item(
            tokens,
            Some(confirm.go.icon),
            confirm.go.label.clone(),
            confirm.go.sublabel.clone().map(Cow::Owned),
            false,
            confirm.go.message.clone(),
        ));
        size = size.row(menu::TALL_ROW_H);

        return (menu::popover(tokens, rows, ADD_W), size.size());
    }

    rows.push(menu::item(
        tokens,
        Some(IconName::Open),
        "Choose files…",
        None,
        false,
        Some(Message::PickFiles),
    ));
    size = size.row(menu::ROW_H);

    rows.push(menu::item(
        tokens,
        Some(IconName::Library),
        "Choose a folder…",
        None,
        false,
        Some(Message::PickFolder),
    ));
    size = size.row(menu::ROW_H);

    if let Some(line) = &facts.from_folder {
        rows.push(menu::separator(tokens));
        size = size.row(menu::SEP_H);

        rows.push(menu::item(
            tokens,
            Some(line.icon),
            line.label.clone(),
            line.sublabel.clone().map(Cow::Owned),
            false,
            line.message.clone(),
        ));
        size = size.row(menu::TALL_ROW_H);
    }

    if !facts.restore.is_empty() {
        rows.push(menu::separator(tokens));
        size = size.row(menu::SEP_H);

        rows.push(menu::section(tokens, "Restore"));
        size = size.row(menu::SECTION_H);

        for line in &facts.restore {
            rows.push(menu::item(
                tokens,
                Some(line.icon),
                line.label.clone(),
                line.sublabel.clone().map(Cow::Owned),
                false,
                line.message.clone(),
            ));
            size = size.row(menu::TALL_ROW_H);
        }
    }

    (menu::popover(tokens, rows, ADD_W), size.size())
}
