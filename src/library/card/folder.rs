//! The folder cell: the same frame with the shelves inside it stacked as a plate
//! on the face.

use iced::widget::{button, column, container, mouse_area, text, Column, Row, Space, Stack};
use iced::{Alignment, Background, Border, Color, Element, Length};
use library_core::blob::LibraryBlob;
use library_core::book;
use library_core::shelf::{children_of, find, Shelf};
use crate::app::{ContextTarget, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::facts::{self, FolderFacts};
use crate::library::{DragFacts, SelectionFacts};
use crate::theme::{mix, wash, Elevation, Tokens};
use super::kit::{
    PLATE_DEPTH, THUMB_CAP, card_button_style, chars_per_line, check_corner, cover_gradient,
    dim_layer, elide, held_layer, nest_ring, sensors,
};
use super::badge::badge_row;


/// One folder's plate: a 3:4 window previewing what is inside — folders
/// first, then books, four cells and no more — with the name and the
/// summary beneath, and the badges over the plate's corner. Like the book's
/// card, the plate owns its shelf; the preview and the facts are read fresh
/// from the library on the frame they are asked for.
#[allow(clippy::too_many_arguments)]
pub fn folder_card(
    tokens: Tokens,
    library: &LibraryBlob,
    shelf: Shelf,
    facts: FolderFacts,
    width: f32,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    // The card's own 8px of air: the plate sits inside it, and the badges
    // sit 6px inside the plate's corner.
    let plate_w = width - 16.0;
    let plate_h = plate_w * 4.0 / 3.0;
    let selected = selection.selected.contains(shelf.id.as_str());
    let held = drag.holds(shelf.id.as_str());
    let lit = selection.lit == Some(shelf.id.as_str());
    let nest = drag.nests_into(shelf.id.as_str());
    let sensor_id = shelf.id.clone();

    let plate_view = plate(tokens, library, &shelf.id, 0, plate_w, plate_h);
    let mut layers: Vec<Element<'static, Message>> = vec![plate_view];
    if let Some(badges) = badge_row(tokens, &facts) {
        layers
            .push(container(badges).width(plate_w).padding(6.0).align_x(Alignment::End).into());
    }
    if selection.selecting {
        layers.push(check_corner(tokens, selected));
    }
    let framed: Element<'static, Message> =
        Stack::with_children(layers).width(plate_w).height(plate_h).into();

    let name = shelf.name.clone();
    let summary_line = facts::summary(facts.books, facts.inside);
    let tap_id = shelf.id.clone();
    let hover_id = shelf.id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Folder(shelf.id)
    };
    let click = button(
        column![
            container(framed).padding(8.0),
            column![
                text(elide(&name, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
                text(summary_line).size(12).color(tokens.muted),
            ]
            .spacing(2)
            .width(width),
        ]
        .spacing(8)
        .width(width),
    )
    .padding(0)
    .style(move |_, status| card_button_style(tokens, status))
    .on_press(Message::CardTap(tap_id));
    let cell: Element<'static, Message> = mouse_area(click)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    // A folder's membership is its whole cell: the accent's tint and inset
    // ring (folder.css), not a ring on the plate alone.
    let cell: Element<'static, Message> = if selected || lit {
        container(cell)
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.accent, 0.10))),
                border: Border { color: tokens.accent, width: 2.0, radius: 10.0.into() },
                ..container::Style::default()
            })
            .into()
    } else {
        cell
    };
    let mut dressed: Vec<Element<'static, Message>> = vec![cell];
    if held {
        dressed.push(held_layer(tokens));
    } else if selection.selecting && !selected {
        dressed.push(dim_layer(tokens, hovered));
    }
    if nest {
        dressed.push(nest_ring(tokens));
    }
    if drag.live() {
        dressed.push(sensors(&sensor_id, true));
    }
    if dressed.len() == 1 {
        dressed.pop().unwrap_or_else(|| Space::new().into())
    } else {
        Stack::with_children(dressed).into()
    }
}

/// What fills one cell of a plate: a folder, previewed as a plate of its
/// own, or a book, previewed as its cover.
enum PlateItem {
    Folder(String),
    Book,
}

/// Folders first, then books — a plate that disagreed with the page about
/// the folder's contents would preview something else. Books and folders
/// share the four cells rather than each getting their own. A member that
/// is a link and not a book is skipped: a pointer is not content.
fn plate_items(library: &LibraryBlob, shelf_id: &str) -> Vec<PlateItem> {
    let mut out: Vec<PlateItem> = children_of(&library.shelves, Some(shelf_id))
        .into_iter()
        .map(|child| PlateItem::Folder(child.id.clone()))
        .collect();
    if let Some(shelf) = find(&library.shelves, shelf_id) {
        out.extend(
            shelf
                .books
                .iter()
                .filter_map(|member| book::find_row(&library.books, member))
                .filter(|row| row.book().is_some())
                .map(|_| PlateItem::Book),
        );
    }
    out.truncate(THUMB_CAP);
    out
}

/// The joinery the cells float on: the theme's line nudged a fifth of the
/// way toward the muted ink — a seam at the ink's weight is a stroke, and a
/// plate drawn in strokes is a table.
pub(crate) fn plate_seam(tokens: Tokens) -> Color {
    mix(tokens.line, tokens.muted, 0.20)
}

/// The recessed tone of a cell with nothing in it: toward the paper rather
/// than toward the line, so an empty cell and a seam are opposite moves in
/// every base and never the same tone.
fn cell_recess(tokens: Tokens) -> Color {
    mix(tokens.surface, tokens.paper, 0.28)
}

/// One plate: a 2×2 window over a shelf's contents, recursive on purpose —
/// a cell that holds a folder holds that folder's own plate, because "what
/// is inside" is the same question at every depth. Only the outermost plate
/// wears the hairline and the shadow; a nested one that lifted with the
/// card would lift twice.
fn plate(
    tokens: Tokens,
    library: &LibraryBlob,
    shelf_id: &str,
    depth: usize,
    width: f32,
    height: f32,
) -> Element<'static, Message> {
    let items = plate_items(library, shelf_id);
    // The deeper the plate, the tighter the joinery: a quarter of a quarter
    // is small enough that the outer plate's gutters would eat it.
    let gap = if depth == 0 { 3.0 } else { 1.0 };
    let cell_w = (width - gap) / 2.0;
    let cell_h = (height - gap) / 2.0;
    let radius = if depth == 0 { 3.0 } else { 1.0 };

    let mut lines: Vec<Element<'static, Message>> = Vec::with_capacity(2);
    for line_ix in 0..2usize {
        let mut line = Row::new().spacing(gap);
        for slot_ix in 0..2usize {
            let cell = match items.get(line_ix * 2 + slot_ix) {
                Some(PlateItem::Folder(id)) if depth < PLATE_DEPTH => {
                    plate(tokens, library, id, depth + 1, cell_w, cell_h)
                }
                Some(PlateItem::Folder(_)) => {
                    recess_cell(tokens, cell_w, cell_h, radius, icon(IconName::Open, 12, tokens.muted))
                }
                Some(PlateItem::Book) => container(Space::new().width(cell_w).height(cell_h))
                    .style(move |_| container::Style {
                        background: Some(Background::Gradient(cover_gradient(tokens))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: radius.into(),
                        },
                        ..container::Style::default()
                    })
                    .into(),
                None => recess_cell(
                    tokens,
                    cell_w,
                    cell_h,
                    radius,
                    Space::new().width(0.0).height(0.0).into(),
                ),
            };
            line = line.push(cell);
        }
        lines.push(line.into());
    }

    let face = container(Column::with_children(lines).spacing(gap)).width(width).height(height);
    if depth == 0 {
        face.style(move |_| container::Style {
            background: Some(Background::Color(plate_seam(tokens))),
            border: Border { color: plate_seam(tokens), width: 1.0, radius: 8.0.into() },
            shadow: Elevation::Pill.shadow(),
            ..container::Style::default()
        })
        .into()
    } else {
        face.style(move |_| container::Style {
            background: Some(Background::Color(plate_seam(tokens))),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
            ..container::Style::default()
        })
        .into()
    }
}

/// A cell on the recessed ground: a deep folder's glyph, or nothing at all.
fn recess_cell(
    tokens: Tokens,
    width: f32,
    height: f32,
    radius: f32,
    face: Element<'static, Message>,
) -> Element<'static, Message> {
    container(face)
        .width(width)
        .height(height)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(cell_recess(tokens))),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: radius.into() },
            ..container::Style::default()
        })
        .into()
}
