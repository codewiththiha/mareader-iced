use crate::app::{ContextTarget, LIBRARY_SCROLL, MenuKind, Message};
use crate::chrome::desktop;
use crate::chrome::icons::{IconName, icon};
use crate::theme::{Tokens, mix};
use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow};
use library_core::blob::LibraryBlob;
use library_core::book::{Row, book_rows};
use library_core::query;

use super::metrics::{
    COL_GAP, CONTENT_MAX, CONTENT_PAD, CONTENT_TOP, ROW_GAP, content_width, grid_metrics,
};
use super::rows::{DragFacts, SelectionFacts, level_folders, level_rows};
use super::{card, facts, list};

/// The shelf's surface: the level's body under the bar's height.
///
/// The level is computed here and handed to the layouts BY OWNERSHIP: the
/// element tree the window lays out must never borrow a vec that dies with
/// this function, so the rows and the folders move into the cards that
/// paint them. `width` is the window's live width — the grid counts its
/// tracks against it the same way the web grid's auto-fit counted against
/// its measured box.
#[allow(clippy::too_many_arguments)]
pub fn view(
    tokens: Tokens,
    library: &LibraryBlob,
    shelf: &str,
    terms: &str,
    hovered: Option<&str>,
    width: f32,
    selection: SelectionFacts<'_>,
    drag: DragFacts<'_>,
) -> Element<'static, Message> {
    let rows = level_rows(library, shelf, terms);
    let folders = level_folders(library, shelf, terms);
    let has_books = book_rows(&library.books).count() > 0;
    let has_anything = has_books || !folders.is_empty();

    let body: Element<'static, Message> = if !has_anything {
        empty_state(tokens)
    } else {
        // The quiet line: a level narrowed to nothing while the library
        // still holds books says why.
        let quiet = if rows.is_empty() && folders.is_empty() && has_books {
            Some(if query::is_active(terms) {
                "No books match this search."
            } else {
                "This shelf is empty."
            })
        } else {
            None
        };

        let inner_width = content_width(width);
        let layout: Element<'static, Message> = if library.view.is_list() {
            list::view(tokens, library, rows, folders, hovered, selection, drag)
        } else {
            grid(
                tokens,
                library,
                library.view.columns,
                rows,
                folders,
                hovered,
                inner_width,
                selection,
                drag,
            )
        };

        let mut inner: iced::widget::Column<'static, Message> = column![layout];
        if let Some(line) = quiet {
            inner = inner.push(
                container(text(line).size(13).color(tokens.muted))
                    .width(Length::Fill)
                    .padding(Padding { top: 64.0, right: 0.0, bottom: 64.0, left: 0.0 })
                    .center_x(Length::Fill),
            );
        }
        framed(inner.into())
    };

    // The level's own floor: a press on the empty ground leaves the
    // selection and closes a context menu, and a right-click on it asks
    // the level's menu. Card and row presses are captured by their own
    // widgets first, so the floor only hears the presses nothing claimed.
    let floor = mouse_area(body)
        .on_press(Message::FloorPressed)
        .on_right_press(Message::ContextMenu(ContextTarget::Level));
    container(floor)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: desktop::TITLE_BAR_H,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
}

/// The level's frame: the scroll, and the 1152px column of content centred
/// inside it with the web page's own air around it.
fn framed<'a>(inner: Element<'a, Message>) -> Element<'a, Message> {
    scrollable(
        container(
            container(inner)
                .width(Length::Fill)
                .max_width(CONTENT_MAX)
                .padding(Padding {
                    top: CONTENT_TOP,
                    right: CONTENT_PAD,
                    bottom: CONTENT_TOP,
                    left: CONTENT_PAD,
                }),
        )
        .width(Length::Fill)
        .center_x(Length::Fill),
    )
    .id(LIBRARY_SCROLL)
    .on_scroll(Message::ShelfViewport)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The grid: the folders at this level, then the books, then the add card —
/// drained into rows of `n` tracks. The track count is the view's columns
/// when the reader pinned them, else whatever the live width holds. The
/// rows and the folders arrive by ownership and move, cell by cell, into
/// the cards that paint them.
#[allow(clippy::too_many_arguments)]
fn grid(
    tokens: Tokens,
    library: &LibraryBlob,
    pinned: Option<u8>,
    rows: Vec<Row>,
    folders: Vec<library_core::shelf::Shelf>,
    hovered: Option<&str>,
    width: f32,
    selection: SelectionFacts<'_>,
    drag: DragFacts<'_>,
) -> Element<'static, Message> {
    let (tracks, cell) = grid_metrics(width, pinned);

    let mut cells: Vec<Element<'static, Message>> = Vec::new();
    for shelf in folders {
        let facts = facts::folder_facts(library, &shelf.id);
        let hot = hovered.is_some_and(|id| id == shelf.id.as_str());
        cells.push(card::folder_card(
            tokens, library, shelf, facts, cell, hot, selection, drag,
        ));
    }
    for entry in rows {
        match entry {
            Row::Book(book) => {
                let hot = hovered.is_some_and(|id| id == book.id.as_str());
                cells.push(card::book_card(tokens, book, cell, hot, selection, drag));
            }
            Row::Link { id, name, target, .. } => {
                let hot = hovered.is_some_and(|hovered| hovered == id.as_str());
                cells.push(card::link_card(
                    tokens, id, name, target, cell, hot, selection, drag,
                ));
            }
        }
    }
    cells.push(card::add_card(tokens, cell));

    // Drain rather than chunk: an `Element` is not `Clone`, so the rows
    // take their cells by ownership.
    let mut layout: iced::widget::Column<'static, Message> = column![];
    while !cells.is_empty() {
        let take = tracks.min(cells.len());
        let line: Vec<Element<'static, Message>> = cells.drain(..take).collect();
        layout = layout.push(iced::widget::Row::with_children(line).spacing(COL_GAP));
    }
    container(layout.spacing(ROW_GAP))
        .width(Length::Fill)
        .align_x(Alignment::Start)
        .into()
}

/// The library with nothing in it: one import door and the drop hint beside
/// it. The door opens the same add menu the grid's last cell and the list's
/// last row open.
fn empty_state(tokens: Tokens) -> Element<'static, Message> {
    let door = button(
        row![
            icon(IconName::Plus, 17, tokens.paper),
            text("Import books").size(14).color(tokens.paper),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(Padding { top: 10.0, right: 18.0, bottom: 10.0, left: 18.0 })
    .style(move |_, status| {
        let background = match status {
            button::Status::Hovered => mix(tokens.accent, tokens.ink, 0.10),
            button::Status::Pressed => mix(tokens.accent, tokens.ink, 0.18),
            _ => tokens.accent,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(background)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
            text_color: tokens.paper,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleMenu(MenuKind::Add));

    container(
        column![
            door,
            text("Or drop a document anywhere in the window").size(12).color(tokens.muted),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}