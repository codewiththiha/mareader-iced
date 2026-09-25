//! The view menu: the layout, the sort key and its order, the cover fit, and the
//! column count the flow and the reader share.

use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding, Size};
use library_core::sort::SortKey;
use library_core::view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, LibraryLayout, LibraryView};

use crate::app::Message;
use crate::chrome::icons::IconName;
use crate::theme::Tokens;
use crate::ui::popover::{self, PanelSize};
use super::VIEW_W;

/// The sort keys in the order the menu lists them.
const SORTS: [SortKey; 5] = [
    SortKey::Manual,
    SortKey::Title,
    SortKey::Author,
    SortKey::Added,
    SortKey::LastRead,
];

/// The shelf view menu's panel: shelves, layouts, columns, covers, sorting
/// and the reload door.
pub fn view_menu<'a>(tokens: Tokens, view: &'a LibraryView) -> (Element<'a, Message>, Size) {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut size = PanelSize::new(VIEW_W);

    // The shelf itself.
    rows.push(popover::item(
        tokens,
        Some(IconName::Plus),
        "New shelf",
        None,
        false,
        Some(Message::CreateShelf),
    ));
    size = size.row(popover::ROW_H);
    rows.push(popover::separator(tokens));
    size = size.row(popover::SEP_H);

    // The two layouts, checked by the one on screen.
    rows.push(popover::item(
        tokens,
        None,
        "List",
        None,
        view.is_list(),
        Some(Message::SetLayout(LibraryLayout::List)),
    ));
    size = size.row(popover::ROW_H);
    rows.push(popover::item(
        tokens,
        None,
        "Grid",
        None,
        !view.is_list(),
        Some(Message::SetLayout(LibraryLayout::Grid)),
    ));
    size = size.row(popover::ROW_H);
    rows.push(popover::separator(tokens));
    size = size.row(popover::SEP_H);

    // Columns: the stepper only exists for the grid.
    if view.columns_enabled() {
        rows.push(columns_row(tokens, view));
        size = size.row(popover::ROW_H + 10.0);
        rows.push(popover::separator(tokens));
        size = size.row(popover::SEP_H);
    }

    // The covers' fit.
    rows.push(popover::section(tokens, "Book covers"));
    size = size.row(popover::SECTION_H);
    rows.push(popover::item(
        tokens,
        None,
        CoverFit::Fit.label(),
        None,
        view.cover == CoverFit::Fit,
        Some(Message::SetCover(CoverFit::Fit)),
    ));
    size = size.row(popover::ROW_H);
    rows.push(popover::item(
        tokens,
        None,
        CoverFit::Crop.label(),
        None,
        view.cover == CoverFit::Crop,
        Some(Message::SetCover(CoverFit::Crop)),
    ));
    size = size.row(popover::ROW_H);
    rows.push(popover::separator(tokens));
    size = size.row(popover::SEP_H);

    // The sort: a direction pair while the key is not the reader's own
    // order, then the keys themselves.
    rows.push(popover::section(tokens, "Sort by"));
    size = size.row(popover::SECTION_H);
    if !view.sort.is_manual() {
        rows.push(direction_row(tokens, view.sort_asc));
        size = size.row(popover::TOGGLE_H + 8.0);
    }
    for key in SORTS {
        rows.push(popover::item(
            tokens,
            None,
            key.label(),
            None,
            view.sort == key,
            Some(Message::SetSort(key)),
        ));
        size = size.row(popover::ROW_H);
    }
    rows.push(popover::separator(tokens));
    size = size.row(popover::SEP_H);

    // The honest reset.
    rows.push(popover::item(
        tokens,
        Some(IconName::Reload),
        "Reload Window",
        Some("Re-reads the library from disk".into()),
        false,
        Some(Message::Reload),
    ));
    size = size.row(popover::TALL_ROW_H);

    (popover::panel(tokens, rows, VIEW_W), size.size())
}

/// The columns row: the label, the Auto pill, and the round steppers around
/// the count. Stepping pins the count off whatever auto-fit last measured;
/// Auto hands it back.
/// What the columns stepper shows and offers. Auto reads as a dash and pins
/// the count the flow is showing on its first press, so both steps are live;
/// a pinned count is dead at the end of its range.
pub(super) struct ColumnsStep {
    pub(super) face: String,
    pub(super) down: bool,
    pub(super) up: bool,
}

pub(super) fn columns_step(columns: Option<u8>, auto_fit: u8) -> ColumnsStep {
    let auto = columns.is_none();
    let effective = columns.unwrap_or(auto_fit);
    ColumnsStep {
        face: if auto { "–".to_string() } else { effective.to_string() },
        down: auto || effective > COLUMNS_MIN,
        up: auto || effective < COLUMNS_MAX,
    }
}

fn columns_row<'a>(tokens: Tokens, view: &'a LibraryView) -> Element<'a, Message> {
    let auto = view.columns.is_none();
    let step = columns_step(view.columns, view.auto_fit);
    let minus = step.down.then_some(Message::StepColumns(-1));
    let plus = step.up.then_some(Message::StepColumns(1));

    let face = row![
        container(text("Columns").size(13).color(tokens.ink)).width(Length::Fill),
        popover::toggle(tokens, None, "Auto", auto, if auto { None } else { Some(Message::AutoColumns) }),
        popover::stepper_button(tokens, IconName::Minus, minus),
        container(text(step.face).size(12).color(tokens.ink))
            .width(20.0)
            .center_x(Length::Fill),
        popover::stepper_button(tokens, IconName::Plus, plus),
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
        popover::toggle(
            tokens,
            Some(IconName::ChevronUp),
            "Ascending",
            ascending,
            Some(Message::SetSortAsc(true)),
        ),
        popover::toggle(
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
