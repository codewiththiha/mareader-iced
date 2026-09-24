//! The list layout: one row per book and one row per shelf — the level as a
//! table for scanning rather than browsing.
//!
//! Same books, same order as the grid; only the shape changes. The frame is
//! the web list's card: a hairline border, 12px corners, the surface at 40%
//! behind the rows, and a hairline between rows. Each row is the list.css
//! shape — the 41.6px cover thumbnail, the title and its second line, the
//! format chip that only the non-PDF formats wear.

use iced::widget::{button, column, container, mouse_area, row, text, Column, Space, Stack};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use library_core::blob::LibraryBlob;
use library_core::book::{Book, Row};
use library_core::shelf::Shelf;
use library_core::text as lib_text;

use crate::app::{ContextTarget, MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::card::{self, elide_line};
use crate::library::{DragFacts, SelectionFacts};
use crate::library::facts::{self, FolderFacts};
use crate::theme::{wash, Tokens};

/// The level as rows: the shelves at this level first, then the books, then
/// the add door — divided by hairlines. The rows are handed over by
/// ownership: the level is computed fresh on every view, and the elements
/// may not borrow the vec they came from.
pub fn view(
    tokens: Tokens,
    library: &LibraryBlob,
    rows: Vec<Row>,
    folders: Vec<Shelf>,
    hovered: Option<&str>,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let mut items: Vec<Element<'static, Message>> = Vec::new();
    let divided = |items: &mut Vec<Element<'static, Message>>, element: Element<'static, Message>| {
        if !items.is_empty() {
            items.push(hairline(tokens));
        }
        items.push(element);
    };

    for shelf in folders {
        let facts = facts::folder_facts(library, &shelf.id);
        let hot = hovered.is_some_and(|id| id == shelf.id.as_str());
        divided(&mut items, shelf_row(tokens, shelf, facts, hot, selection, drag));
    }
    for entry in rows {
        match entry {
            Row::Book(book) => {
                let hot = hovered.is_some_and(|id| id == book.id.as_str());
                divided(&mut items, book_row(tokens, book, hot, selection, drag));
            }
            Row::Link { id, name, target, .. } => {
                let hot = hovered.is_some_and(|hovered| hovered == id.as_str());
                divided(&mut items, link_row(tokens, id, name, target, hot, selection, drag));
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

/// The row's inset rule of membership: 2px of the accent down the left
/// edge (select.css `inset 2px 0 0`). An overlay rather than a border,
/// because a layout border would ring all four sides.
fn rule_layer(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(2.0).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            ..container::Style::default()
        })
        .into()
}

/// What every row wears while choosing or dragging: the rule when it is
/// in the set, the step back when it is not, the hold's fade when the drag
/// carries it, the seam or ring the drop would write, and — while a drag
/// is live — the band sensors that read which part of the row the pointer
/// is on. All overlays are non-interactive washes except the sensors, so
/// the row's button still owns every press.
#[allow(clippy::too_many_arguments)]
fn dressed(
    tokens: Tokens,
    cell: Element<'static, Message>,
    hovered: bool,
    selection: SelectionFacts,
    selected: bool,
    drag: DragFacts,
    id: &str,
    folder: bool,
) -> Element<'static, Message> {
    let mut layers: Vec<Element<'static, Message>> = vec![cell];
    if selection.selecting && selected {
        layers.push(rule_layer(tokens));
    }
    if drag.holds(id) {
        layers.push(card::held_layer(tokens));
    } else if selection.selecting && !selected {
        layers.push(card::dim_layer(tokens, hovered));
    }
    if folder {
        if drag.nests_into(id) {
            layers.push(inset_ring(tokens, true));
        }
        if drag.sibling_before(id) {
            layers.push(edge_rule(tokens, true));
        }
        if drag.sibling_after(id) {
            layers.push(edge_rule(tokens, false));
        }
    } else {
        if drag.inserts_before(id) {
            layers.push(edge_rule(tokens, true));
        }
        if drag.inserts_after(id) {
            layers.push(edge_rule(tokens, false));
        }
        if drag.folds_with(id) {
            layers.push(inset_ring(tokens, false));
        }
    }
    if drag.live() {
        layers.push(card::sensors(id, folder));
    }
    if layers.len() == 1 {
        layers.pop().unwrap_or_else(|| Space::new().into())
    } else {
        Stack::with_children(layers).into()
    }
}

/// The row's seam of a coming drop: two pixels of the accent along the top
/// (a before) or the bottom (an after) edge — the two halves of one row,
/// never both lit, because the band that paints them is the band the
/// commit resolves its index from.
fn edge_rule(tokens: Tokens, top: bool) -> Element<'static, Message> {
    container(
        container(Space::new().width(Length::Fill).height(2.0)).style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            ..container::Style::default()
        }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_y(if top { Alignment::Start } else { Alignment::End })
    .into()
}

/// The row's inset ring: the fold's promise around the whole row, and —
/// tinted — the shelf row about to take the hold inside itself. An inset
/// rather than an outer ring, because an outer one would shove its
/// neighbours in a divided list.
fn inset_ring(tokens: Tokens, tint: bool) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: tint.then_some(Background::Color(wash(tokens.accent, 0.10))),
            border: Border { color: tokens.accent, width: 2.0, radius: 0.0.into() },
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

/// A shelf's row: the way-in chevron, the folder glyph, the name, the
/// badge saying where its books live and the summary both layouts share.
/// The row opens the shelf.
fn shelf_row(
    tokens: Tokens,
    shelf: Shelf,
    facts: FolderFacts,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let summary = facts::summary(facts.books, facts.inside);
    let name = elide_line(&shelf.name);
    let selected = selection.selected.contains(shelf.id.as_str());
    let lit = selection.lit == Some(shelf.id.as_str());
    let tap_id = shelf.id.clone();
    let dressed_id = shelf.id.clone();
    let hover_id = shelf.id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Folder(shelf.id)
    };
    let mut face = row![
        icon(IconName::Next, 12, tokens.muted),
        icon(IconName::Folder, 15, tokens.muted),
        container(text(name).size(13).color(tokens.ink)).width(Length::Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if let Some(badge) = facts.badge {
        face = face.push(card::badge_chip(tokens, badge));
    }
    let line = button(face.push(text(summary).size(12).color(tokens.muted)))
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .style(move |_, status| card::row_button_style(tokens, status, selected || lit))
        .on_press(Message::CardTap(tap_id));
    let cell: Element<'static, Message> = mouse_area(line)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    dressed(tokens, cell, hovered, selection, selected, drag, &dressed_id, true)
}

/// A book's row: the thumbnail, the title and its second line, the chip.
fn book_row(
    tokens: Tokens,
    book: Book,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let title = elide_line(&book.title());
    let sub = elide_line(
        &book
            .author()
            .unwrap_or_else(|| lib_text::page_line(book.page, book.num_pages)),
    );
    let selected = selection.selected.contains(book.id.as_str());
    let lit = selection.lit == Some(book.id.as_str());
    let mut line = row![
        card::list_thumb(tokens, selection.selecting.then_some(selected)),
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
    let tap_id = book.id.clone();
    let dressed_id = book.id.clone();
    let hover_id = book.id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Row(book.id)
    };
    let row_el = button(line)
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .style(move |_, status| card::row_button_style(tokens, status, selected || lit))
        .on_press(Message::CardTap(tap_id));
    let cell: Element<'static, Message> = mouse_area(row_el)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    dressed(tokens, cell, hovered, selection, selected, drag, &dressed_id, false)
}

/// A link's row: the glyph stands where the cover sits.
#[allow(clippy::too_many_arguments)]
fn link_row(
    tokens: Tokens,
    id: String,
    name: String,
    target: String,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let label = elide_line(&name);
    let selected = selection.selected.contains(id.as_str());
    let lit = selection.lit == Some(id.as_str());
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

    let hover_id = id.clone();
    let tap_id = id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Row(id.clone())
    };
    let action = button(face)
        .width(Length::Fill)
        .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
        .style(move |_, status| card::row_button_style(tokens, status, selected || lit));
    let action = if library_core::id::is_shelf(&target) {
        action.on_press(Message::CardTap(id))
    } else {
        action
    };
    let cell: Element<'static, Message> = mouse_area(action)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    dressed(tokens, cell, hovered, selection, selected, drag, &tap_id, false)
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
    .style(move |_, status| card::row_button_style(tokens, status, false))
    .on_press(Message::ToggleMenu(MenuKind::Add))
    .into()
}
