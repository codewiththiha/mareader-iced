//! The bar's two panels: the shelf view menu and the add menu.
//!
//! Both are built from the popover vocabulary in [`crate::ui::menu`] and
//! answer with the application's own messages. Each builder returns the
//! panel together with the size its rows add up to, so placement can clamp
//! against what the panel really occupies before layout exists.

use std::borrow::Cow;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding, Size};

use library_core::book::Row;
use library_core::shelf::Shelf;
use library_core::sort::SortKey;
use library_core::view::{CoverFit, LibraryLayout, LibraryView, COLUMNS_MAX, COLUMNS_MIN};

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::menu::{self, PanelSize};

/// The add menu's panel width.
const ADD_W: f32 = 220.0;
/// The view menu's panel width.
const VIEW_W: f32 = 264.0;
/// The shelf menu's panel width.
const SHELF_W: f32 = 224.0;

/// The sort keys in the order the menu lists them.
const SORTS: [SortKey; 5] = [
    SortKey::Manual,
    SortKey::Title,
    SortKey::Author,
    SortKey::Added,
    SortKey::LastRead,
];

/// One computed menu line: what the app knows about a watch seat or a
/// restore candidate, laid out by the menu. `None` for the message renders
/// the row disabled — listed, but not quietly dropped.
pub struct MenuLine {
    pub icon: IconName,
    pub label: String,
    pub sublabel: Option<String>,
    pub message: Option<Message>,
}

/// The watch row's face: the toggle the folder context menu shows, with its
/// message already aimed at the seat's shelf.
pub struct WatchFacts {
    pub icon: IconName,
    pub label: String,
    pub sublabel: String,
    pub message: Message,
}

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

/// A context menu's panel width.
const CONTEXT_W: f32 = 232.0;

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

    let (remove_label, remove_message) = match row {
        Row::Book { .. } => ("Remove from library", Message::AskRemoveRow(row_id_of(row))),
        Row::Link { .. } => ("Remove link", Message::AskRemoveRow(row_id_of(row))),
    };
    rows.push(menu::danger_item(None, remove_label, remove_message));
    size = size.row(menu::ROW_H);

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}

fn row_id_of(row: &Row) -> String {
    match row {
        Row::Book(book) => book.id.clone(),
        Row::Link { id, .. } => id.clone(),
    }
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

/// The shelf view menu's panel: shelves, layouts, columns, covers, sorting
/// and the reload door.
pub fn view_menu<'a>(tokens: Tokens, view: &'a LibraryView) -> (Element<'a, Message>, Size) {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut size = PanelSize::new(VIEW_W);

    // The shelf itself.
    rows.push(menu::item(
        tokens,
        Some(IconName::Plus),
        "New shelf",
        None,
        false,
        Some(Message::CreateShelf),
    ));
    size = size.row(menu::ROW_H);
    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    // The two layouts, checked by the one on screen.
    rows.push(menu::item(
        tokens,
        None,
        "List",
        None,
        view.is_list(),
        Some(Message::SetLayout(LibraryLayout::List)),
    ));
    size = size.row(menu::ROW_H);
    rows.push(menu::item(
        tokens,
        None,
        "Grid",
        None,
        !view.is_list(),
        Some(Message::SetLayout(LibraryLayout::Grid)),
    ));
    size = size.row(menu::ROW_H);
    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    // Columns: the stepper only exists for the grid.
    if view.columns_enabled() {
        rows.push(columns_row(tokens, view));
        size = size.row(menu::ROW_H + 10.0);
        rows.push(menu::separator(tokens));
        size = size.row(menu::SEP_H);
    }

    // The covers' fit.
    rows.push(menu::section(tokens, "Book covers"));
    size = size.row(menu::SECTION_H);
    rows.push(menu::item(
        tokens,
        None,
        CoverFit::Fit.label(),
        None,
        view.cover == CoverFit::Fit,
        Some(Message::SetCover(CoverFit::Fit)),
    ));
    size = size.row(menu::ROW_H);
    rows.push(menu::item(
        tokens,
        None,
        CoverFit::Crop.label(),
        None,
        view.cover == CoverFit::Crop,
        Some(Message::SetCover(CoverFit::Crop)),
    ));
    size = size.row(menu::ROW_H);
    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    // The sort: a direction pair while the key is not the reader's own
    // order, then the keys themselves.
    rows.push(menu::section(tokens, "Sort by"));
    size = size.row(menu::SECTION_H);
    if !view.sort.is_manual() {
        rows.push(direction_row(tokens, view.sort_asc));
        size = size.row(menu::TOGGLE_H + 8.0);
    }
    for key in SORTS {
        rows.push(menu::item(
            tokens,
            None,
            key.label(),
            None,
            view.sort == key,
            Some(Message::SetSort(key)),
        ));
        size = size.row(menu::ROW_H);
    }
    rows.push(menu::separator(tokens));
    size = size.row(menu::SEP_H);

    // The honest reset.
    rows.push(menu::item(
        tokens,
        Some(IconName::Reload),
        "Reload Window",
        Some("Re-reads the library from disk".into()),
        false,
        Some(Message::Reload),
    ));
    size = size.row(menu::TALL_ROW_H);

    (menu::popover(tokens, rows, VIEW_W), size.size())
}

/// The columns row: the label, the Auto pill, and the round steppers around
/// the count. Stepping pins the count off whatever auto-fit last measured;
/// Auto hands it back.
fn columns_row<'a>(tokens: Tokens, view: &'a LibraryView) -> Element<'a, Message> {
    let auto = view.columns.is_none();
    let effective = view.columns.unwrap_or(view.auto_fit);
    let value = if auto { "–".to_string() } else { effective.to_string() };

    let minus = if auto || effective > COLUMNS_MIN {
        Some(Message::StepColumns(-1))
    } else {
        None
    };
    let plus = if auto || effective < COLUMNS_MAX {
        Some(Message::StepColumns(1))
    } else {
        None
    };

    let face = row![
        container(text("Columns").size(13).color(tokens.ink)).width(Length::Fill),
        menu::toggle(tokens, None, "Auto", auto, if auto { None } else { Some(Message::AutoColumns) }),
        menu::stepper_button(tokens, IconName::Minus, minus),
        container(text(value).size(12).color(tokens.ink))
            .width(20.0)
            .center_x(Length::Fill),
        menu::stepper_button(tokens, IconName::Plus, plus),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    container(face)
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 8.0, bottom: 8.0, left: 8.0 })
        .into()
}

/// The sort direction pair: two equal pills, the live one on the accent's
/// bed.
fn direction_row(tokens: Tokens, ascending: bool) -> Element<'static, Message> {
    let face = row![
        menu::toggle(
            tokens,
            Some(IconName::ChevronUp),
            "Ascending",
            ascending,
            Some(Message::SetSortAsc(true)),
        ),
        menu::toggle(
            tokens,
            Some(IconName::ChevronDown),
            "Descending",
            !ascending,
            Some(Message::SetSortAsc(false)),
        ),
    ]
    .spacing(4);

    container(face)
        .width(Length::Fill)
        .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .into()
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
        let (icon, label, message) = if selecting {
            (IconName::Undo, "Clear selection", Message::ClearSelection)
        } else {
            (IconName::Check, "Select all", Message::SelectAll)
        };
        rows.push(menu::item(tokens, Some(icon), label, None, false, Some(message)));
        size = size.row(menu::ROW_H);
    }

    (menu::popover(tokens, rows, CONTEXT_W), size.size())
}
