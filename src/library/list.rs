//! The list layout: one row per book and one row per shelf — the level as a
//! table for scanning rather than browsing.
//!
//! Same books, same order as the grid; only the shape changes. The frame is
//! the web list's card: a hairline border, 12px corners, the surface at 40%
//! behind the rows, and a hairline between rows. Each row is the list.css
//! shape — the 41.6px cover thumbnail, the title and its second line, the
//! format chip that only the non-PDF formats wear.

use iced::widget::{button, column, container, mouse_area, row, text, Column, Space};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use library_core::book::{Book, Row};
use library_core::shelf::Shelf;
use library_core::text::{self as lib_text, plural};

use crate::app::{ContextTarget, MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::card::{self, elide_line};
use crate::theme::{wash, Tokens};

/// The level as rows: the shelves at this level first, then the books, then
/// the add door — divided by hairlines. The rows are handed over by
/// ownership: the level is computed fresh on every view, and the elements
/// may not borrow the vec they came from.
pub fn view(
    tokens: Tokens,
    rows: Vec<Row>,
    folders: Vec<Shelf>,
) -> Element<'static, Message> {
    let mut items: Vec<Element<'static, Message>> = Vec::new();
    let divided = |items: &mut Vec<Element<'static, Message>>, element: Element<'static, Message>| {
        if !items.is_empty() {
            items.push(hairline(tokens));
        }
        items.push(element);
    };

    for shelf in folders {
        divided(&mut items, shelf_row(tokens, shelf));
    }
    for entry in rows {
        match entry {
            Row::Book(book) => divided(&mut items, book_row(tokens, book)),
            Row::Link { id, name, target, .. } => {
                divided(&mut items, link_row(tokens, id, name, target));
            }
        }
    }
    divided(&mut items, add_row(tokens));

    container(Column::with_children(items).width(Length::Fill))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.surface, 0.40))),
            border: Border { color: tokens.line, width: 1.0, radius: 12.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// The hairline the rows are divided by.
fn hairline(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(1.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.line)),
            ..container::Style::default()
        })
        .into()
}

/// A shelf's row: the way-in chevron, the folder glyph, the name and the
/// count. The row opens the shelf.
fn shelf_row(tokens: Tokens, shelf: Shelf) -> Element<'static, Message> {
    let count = plural(shelf.books.len(), "book", "books");
    let name = elide_line(&shelf.name);
    let right_id = shelf.id.clone();
    let line = button(
        row![
            icon(IconName::Next, 12, tokens.muted),
            icon(IconName::Folder, 15, tokens.muted),
            container(text(name).size(13).color(tokens.ink)).width(Length::Fill),
            text(count).size(12).color(tokens.muted),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_, status| card::row_button_style(tokens, status))
    .on_press(Message::Navigate(shelf.id));
    mouse_area(line)
        .on_right_press(Message::ContextMenu(ContextTarget::Folder(right_id)))
        .into()
}

/// A book's row: the thumbnail, the title and its second line, the chip.
fn book_row(tokens: Tokens, book: Book) -> Element<'static, Message> {
    let title = elide_line(&book.title());
    let sub = elide_line(
        &book
            .author()
            .unwrap_or_else(|| lib_text::page_line(book.page, book.num_pages)),
    );
    let mut line = row![
        card::list_thumb(tokens),
        column![
            text(title).size(13).color(tokens.ink),
            text(sub).size(12).color(tokens.muted),
        ]
        .spacing(2)
        .width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    if let Some(chip) = card::format_chip(tokens, book.format) {
        line = line.push(chip);
    }
    let right_id = book.id.clone();
    let row = button(line)
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .style(move |_, status| card::row_button_style(tokens, status))
        .on_press(Message::OpenBook(book.id.clone()));
    mouse_area(row)
        .on_right_press(Message::ContextMenu(ContextTarget::Row(right_id)))
        .into()
}

/// A link's row: the glyph stands where the cover sits.
fn link_row(tokens: Tokens, id: String, name: String, target: String) -> Element<'static, Message> {
    let label = elide_line(&name);
    let face = row![
        container(icon(IconName::Link, 14, tokens.muted))
            .width(41.6)
            .padding(Padding { top: 10.0, right: 0.0, bottom: 10.0, left: 0.0 })
            .center_x(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.surface, 0.60))),
                border: Border { color: tokens.line, width: 1.0, radius: 3.0.into() },
                ..container::Style::default()
            }),
        container(text(label).size(13).color(tokens.ink)).width(Length::Fill),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let action = button(face)
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .style(move |_, status| card::row_button_style(tokens, status));
    let action = if library_core::id::is_shelf(&target) {
        action.on_press(Message::Navigate(target))
    } else {
        action
    };
    mouse_area(action)
        .on_right_press(Message::ContextMenu(ContextTarget::Row(id)))
        .into()
}

/// The list's last row: the add door, wearing the row's own shape.
fn add_row(tokens: Tokens) -> Element<'static, Message> {
    button(
        row![icon(IconName::Plus, 15, tokens.muted), text("Add books").size(13).color(tokens.ink)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 10.0, right: 12.0, bottom: 10.0, left: 12.0 })
    .style(move |_, status| card::row_button_style(tokens, status))
    .on_press(Message::ToggleMenu(MenuKind::Add))
    .into()
}
