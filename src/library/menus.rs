//! The bar's two panels: the shelf view menu and the add menu.
//!
//! Both are built from the popover vocabulary in [`crate::ui::menu`] and
//! answer with the application's own messages. Each builder returns the
//! panel together with the size its rows add up to, so placement can clamp
//! against what the panel really occupies before layout exists.

use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding, Size};

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

/// The sort keys in the order the menu lists them.
const SORTS: [SortKey; 5] = [
    SortKey::Manual,
    SortKey::Title,
    SortKey::Author,
    SortKey::Added,
    SortKey::LastRead,
];

/// The add door's panel: the two pickers. (The in-folder picker and the
/// restore list arrive with the import and governance flows that own them.)
pub fn add_menu(tokens: Tokens) -> (Element<'static, Message>, Size) {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut size = PanelSize::new(ADD_W);

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

    (menu::popover(tokens, rows, ADD_W), size.size())
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
        Some("Re-reads the library from disk"),
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
