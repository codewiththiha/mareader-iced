//! The shelf's cells: one book's card, one folder's plate, one link's tile,
//! and the add card that closes every grid.
//!
//! The grid's geometry is the web app's, carried over whole: a cover frame
//! at A4 portrait (210×297), 6px corners, the paper's own gradient behind a
//! title until real cover art lands with the engines, an info block under
//! the cover (the title on two lines, the author or the page line beneath),
//! and the 3px progress hairline when a book has one. Hover deepens the
//! shadow — the web card also translated up 2px, which a layout cell cannot
//! do natively, so the shadow carries the lift alone.

use iced::gradient::Linear;
use iced::widget::{button, column, container, mouse_area, text, Column, Row, Space, Stack};
use iced::{
    Alignment, Background, Border, Color, Element, Gradient, Length, Padding, Radians, Shadow,
    Vector,
};

use library_core::blob::LibraryBlob;
use library_core::book::{self, Book};
use library_core::shelf::{children_of, find, Shelf};
use library_core::text as lib_text;
use reader_core::format::Format;

use crate::app::{ContextTarget, MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::facts::{self, Badge, FolderFacts};
use crate::library::drag::Band;
use crate::library::{DragFacts, SelectionFacts};
use crate::theme::{mix, wash, Tokens};

/// The cover's aspect, A4 portrait: height = width × 297/210.
pub const COVER_RATIO: f32 = 297.0 / 210.0;
/// The most member covers a folder plate shows, in its 2×2 window. The
/// drag's fold preview borrows the same cap: a preview of more cells would
/// promise a plate the library does not draw.
pub(crate) const THUMB_CAP: usize = 4;
/// Past this depth a folder cell draws as a glyph: a few pixels of a
/// nested plate is a smear, not a preview.
const PLATE_DEPTH: usize = 2;

/// How many characters fit a line of `size`px text in `width` — the elision
/// budget. iced has no line-clamp; the shelf's texts are pre-cut to the
/// lines their block reserves so a long title cannot grow its card.
fn chars_per_line(width: f32, size: f32) -> usize {
    ((width / (size * 0.52)) as usize).max(4)
}

/// Cut `s` to `max_chars` characters with an ellipsis — character-based on
/// purpose, since the budget above counts characters too.
fn elide(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The web cover's fallback gradient: 18% of the accent into the paper,
/// 160 degrees.
fn cover_gradient(tokens: Tokens) -> Gradient {
    Gradient::Linear(
        Linear::new(Radians(2.7925))
            .add_stop(0.0, mix(tokens.paper, tokens.accent, 0.18))
            .add_stop(0.74, tokens.paper),
    )
}

/// The cover frame: the gradient, the radius, and the shadow that deepens
/// under the pointer.
fn cover_style(tokens: Tokens, hovered: bool, selected: bool) -> container::Style {
    let shadow = if hovered {
        Shadow {
            color: wash(Color::BLACK, 0.26),
            offset: Vector::new(0.0, 10.0),
            blur_radius: 24.0,
        }
    } else {
        Shadow {
            color: wash(Color::BLACK, 0.18),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        }
    };
    container::Style {
        background: Some(Background::Gradient(cover_gradient(tokens))),
        border: if selected {
            // The ring of membership (select.css: 0 0 0 2px accent).
            Border { color: tokens.accent, width: 2.0, radius: 6.0.into() }
        } else {
            Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 6.0.into() }
        },
        shadow,
        ..container::Style::default()
    }
}

/// The card's click layer: nothing of its own — the cover's shadow and the
/// pointer are the hover story.
fn card_button_style(tokens: Tokens, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Pressed => Some(Background::Color(wash(tokens.surface, 0.01))),
        _ => None,
    };
    button::Style {
        background,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
        text_color: tokens.ink,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// The mark that says which ones you have already tapped. Empty it is a
/// ring on the art; filled it is the accent with a check, so the set is
/// legible without reading the bar's count. Bottom-left rather than
/// top-left: the top-left corner is the missing-file badge's, and a book
/// can be both missing and selected.
fn check_chip(tokens: Tokens, selected: bool) -> Element<'static, Message> {
    let face: Element<'static, Message> = if selected {
        container(icon(IconName::Check, 11, tokens.paper))
            .width(18.0)
            .height(18.0)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    } else {
        Space::new().width(18.0).height(18.0).into()
    };
    container(face)
        .width(18.0)
        .height(18.0)
        .style(move |_| container::Style {
            background: Some(Background::Color(if selected {
                tokens.accent
            } else {
                wash(Color::BLACK, 0.34)
            })),
            border: Border {
                color: if selected { tokens.accent } else { wash(Color::WHITE, 0.78) },
                width: 1.5,
                radius: 999.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The chip parked in the art's bottom-left corner, 6px in.
fn check_corner(tokens: Tokens, selected: bool) -> Element<'static, Message> {
    container(check_chip(tokens, selected))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(6.0)
        .align_x(Alignment::Start)
        .align_y(Alignment::End)
        .into()
}

/// The step back a card out of the set takes while choosing: the web card's
/// 0.62 opacity, here a wash of the paper over it — and 0.95 under the
/// pointer, because the reader is about to tap one of them. A layout cell
/// cannot fade its own ink, so the wash carries the step back. The add door
/// never wears it: dimming a door would advertise a choice it does not
/// offer.
pub fn dim_layer(tokens: Tokens, hovered: bool) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(
                tokens.paper,
                if hovered { 0.05 } else { 0.38 },
            ))),
            ..container::Style::default()
        })
        .into()
}

/// The band readers a drag wears on a cell: proportional zones reporting
/// which part of it the pointer is on — halves for a book, whose bottom
/// half is the after its top is not, and quarters-and-a-half for a folder,
/// whose middle band is the nest its edges are not. Enter-only on purpose:
/// a sensor that captured a press would steal the release the drop is
/// answered by. The list rows and the grid cards wear the same zones, so
/// one gesture reads the same in both layouts.
pub fn sensors(id: &str, folder: bool) -> Element<'static, Message> {
    let zones: [(Band, u16); 3] = if folder {
        [(Band::Top, 1), (Band::Middle, 2), (Band::Bottom, 1)]
    } else {
        [(Band::Top, 1), (Band::Bottom, 1), (Band::Bottom, 0)]
    };
    let mut column = Column::new().width(Length::Fill).height(Length::Fill);
    for (band, portion) in zones {
        if portion == 0 {
            continue;
        }
        column = column.push(
            mouse_area(
                Space::new().width(Length::Fill).height(Length::FillPortion(portion)),
            )
            .on_enter(Message::DragBand(id.to_string(), band)),
        );
    }
    column.into()
}

/// What a drag is holding: every cell in the payload fades — not only the
/// one the press began on — because the set the reader picked up has to
/// stay readable as a set. The web cells drop to 0.45 opacity; a layout
/// cell cannot fade its own ink, so the wash carries it, the same answer
/// the choosing step-back wears.
pub fn held_layer(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.paper, 0.55))),
            ..container::Style::default()
        })
        .into()
}

/// The seam a drop would write: two pixels of the accent, the cover's
/// height, round-ended. The web seam stands in the gutter to the card's
/// left; a layout cell cannot draw outside itself, so the line hugs the
/// card's own edge — the same accent, one gutter over.
fn seam_line(tokens: Tokens, height: f32) -> Element<'static, Message> {
    container(Space::new().width(2.0).height(height))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// The seam parked at the card's left edge, over the cover's height.
fn seam_corner(tokens: Tokens, height: f32) -> Element<'static, Message> {
    container(seam_line(tokens, height))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .into()
}

/// The fold's ring on a card: the accent around the cover the drop would
/// replace with a shelf, and the halo that says the ring is an offer, not
/// a position — a line at the card's edge would promise a seam the drop no
/// longer honours. The web halo is a hard 6px spread; an iced shadow has
/// no spread, so the glow carries the same accent at the same reach.
fn fold_ring(tokens: Tokens, height: f32) -> Element<'static, Message> {
    container(
        container(Space::new().width(Length::Fill).height(height)).style(move |_| {
            container::Style {
                background: None,
                border: Border { color: tokens.accent, width: 2.0, radius: 6.0.into() },
                shadow: Shadow {
                    color: wash(tokens.accent, 0.20),
                    offset: Vector::new(0.0, 0.0),
                    blur_radius: 10.0,
                },
                ..container::Style::default()
            }
        }),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Start)
    .align_y(Alignment::Start)
    .into()
}

/// A folder cell about to take the hold INSIDE itself: the accent's tint,
/// ring and halo over the whole cell (folder.css `.folder-drag-over`) —
/// the loudest thing a card wears, because it is the one answer the reader
/// is waiting for.
fn nest_ring(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.accent, 0.12))),
            border: Border { color: tokens.accent, width: 2.0, radius: 10.0.into() },
            shadow: Shadow {
                color: wash(tokens.accent, 0.20),
                offset: Vector::new(0.0, 0.0),
                blur_radius: 10.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// One book's card in the grid: cover, info, and the progress hairline.
/// The card OWNS its book — the level's rows are computed fresh on every
/// view, and an element may not borrow a vec that dies with the function
/// that built it.
pub fn book_card(
    tokens: Tokens,
    book: Book,
    width: f32,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let title = book.title();
    let sub = book
        .author()
        .unwrap_or_else(|| lib_text::page_line(book.page, book.num_pages));
    let progress = book.progress();
    let missing = book.missing;
    let selected = selection.selected.contains(book.id.as_str());
    // The reveal's own light: the cell the sheet's answer named wears the
    // membership ring, from the ground the light already paints.
    let lit = selection.lit == Some(book.id.as_str());
    // The drag's three facts about this card, read before the id moves
    // into the right-click's answer.
    let held = drag.holds(book.id.as_str());
    let seam = drag.inserts_before(book.id.as_str());
    let fold = drag.folds_with(book.id.as_str());
    let sensor_id = book.id.clone();

    // The cover face: the title centred on the gradient, and — for a book
    // the library has lost sight of — the wash and the badge that say so.
    let title_layer = container(
        text(elide(&title, chars_per_line(width - 20.0, 13.0) * 5))
            .size(13)
            .color(wash(tokens.ink, 0.80)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(10.0)
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    // The cover's layers by depth: the title, then the missing wash and
    // badge when the library has lost sight of the book, then the
    // selection's mark — the corner pieces never overlap.
    let mut layers: Vec<Element<'static, Message>> = vec![title_layer.into()];
    if missing {
        layers.push(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .style(move |_| container::Style {
                    background: Some(Background::Color(wash(tokens.paper, 0.55))),
                    ..container::Style::default()
                })
                .into(),
        );
        layers.push(
            container(
                container(icon(IconName::Close, 11, tokens.muted))
                    .padding(4.0)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(tokens.surface)),
                        border: Border {
                            color: tokens.line,
                            width: 1.0,
                            radius: 999.0.into(),
                        },
                        ..container::Style::default()
                    }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(6.0)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .into(),
        );
    }
    if selection.selecting {
        layers.push(check_corner(tokens, selected));
    }

    let cover = container(Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .width(width)
        .height(cover_h)
        .style(move |_| cover_style(tokens, hovered, selected || lit));

    let info: Element<'static, Message> = column![
        text(elide(&title, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
        text(elide(&sub, chars_per_line(width, 12.0))).size(12).color(tokens.muted),
    ]
    .spacing(2)
    .width(width)
    .into();

    let mut card = column![cover, info].spacing(8).width(width);
    if let Some(fraction) = progress
        && fraction > 0.0
    {
        card = card.push(progress_bar(tokens, width, fraction));
    }

    // One tap message for every cell: the app decides what a tap means —
    // a membership while choosing, an open otherwise — and what the hold
    // that started on this card swallowed.
    let tap_id = book.id.clone();
    let hover_id = book.id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Row(book.id)
    };
    let click = button(card)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status))
        .on_press(Message::CardTap(tap_id));
    let cell: Element<'static, Message> = mouse_area(click)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    // The cell's overlays by depth: the step back or the hold's fade, then
    // the seam, then the fold's ring — each a non-interactive wash, so the
    // button underneath still owns every press.
    let mut dressed: Vec<Element<'static, Message>> = vec![cell];
    if held {
        dressed.push(held_layer(tokens));
    } else if selection.selecting && !selected {
        dressed.push(dim_layer(tokens, hovered));
    }
    if seam {
        dressed.push(seam_corner(tokens, cover_h));
    }
    if fold {
        dressed.push(fold_ring(tokens, cover_h));
    }
    if drag.live() {
        dressed.push(sensors(&sensor_id, false));
    }
    if dressed.len() == 1 {
        dressed.pop().unwrap_or_else(|| Space::new().into())
    } else {
        Stack::with_children(dressed).into()
    }
}

/// The 3px progress hairline: the line at 60% as the track, the accent as
/// the fill.
fn progress_bar(tokens: Tokens, width: f32, fraction: f64) -> Element<'static, Message> {
    let fill = (width * fraction.clamp(0.0, 1.0) as f32).max(3.0);
    container(
        container(Space::new().width(fill).height(3.0)).style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 1.5.into() },
            ..container::Style::default()
        }),
    )
    .width(width)
    .height(3.0)
    .style(move |_| container::Style {
        background: Some(Background::Color(wash(tokens.line, 0.60))),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 1.5.into() },
        ..container::Style::default()
    })
    .into()
}

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
            shadow: Shadow {
                color: wash(Color::BLACK, 0.18),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
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

/// The plate's corner badges: where the books live, and — when the tree
/// tracks this rung — the dot that says so. `None` when a shelf wears
/// neither, and then the corner stays clean.
fn badge_row(tokens: Tokens, facts: &FolderFacts) -> Option<Element<'static, Message>> {
    if facts.badge.is_none() && !facts.watched {
        return None;
    }
    let mut badges = Row::new().spacing(4).align_y(Alignment::Center);
    if let Some(badge) = facts.badge {
        badges = badges.push(badge_chip(tokens, badge));
    }
    if facts.watched {
        badges = badges.push(watch_dot(tokens));
    }
    Some(badges.into())
}

/// The chip saying where a folder's books live — "Copied", "On disk",
/// "Mixed". The quietest thing on the plate on purpose: it is true of
/// nearly every card. Shared by the card's corner and the list's row, so
/// one shelf cannot describe itself two ways.
pub fn badge_chip(tokens: Tokens, badge: Badge) -> Element<'static, Message> {
    let (border, background, color) = if badge.mixed {
        // Mixed content is slightly louder than the single-kind badges, so
        // the reader notices the shelf holds two kinds.
        (
            mix(tokens.line, tokens.accent, 0.30),
            mix(tokens.paper, tokens.accent, 0.12),
            mix(tokens.ink, tokens.muted, 0.20),
        )
    } else {
        (tokens.line, wash(tokens.paper, 0.84), tokens.muted)
    };
    container(text(badge.words).size(10).color(color))
        .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            border: Border { color: border, width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// A watched folder's dot: the accent under a hairline of the line. The web
/// dot breathed on a 2.4s loop; a breathing decoration would keep the whole
/// window redrawing for chrome nobody is looking at, so the native dot
/// holds still and says the same thing.
fn watch_dot(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(8.0).height(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// A row that points at a shelf rather than being a book: the link glyph on
/// the cover's tile. A link whose target is not a shelf is listed but
/// quiet — nothing to open until the shelves it may name exist.
#[allow(clippy::too_many_arguments)]
pub fn link_card(
    tokens: Tokens,
    id: String,
    name: String,
    target: String,
    width: f32,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let selected = selection.selected.contains(id.as_str());
    let held = drag.holds(id.as_str());
    let lit = selection.lit == Some(id.as_str());
    let seam = drag.inserts_before(id.as_str());
    let fold = drag.folds_with(id.as_str());
    let sensor_id = id.clone();
    let mut layers: Vec<Element<'static, Message>> = vec![
        container(icon(IconName::Link, 28, wash(tokens.muted, 0.90)))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    ];
    if selection.selecting {
        layers.push(check_corner(tokens, selected));
    }
    let face = container(Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .width(width)
        .height(cover_h)
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.surface, 0.60))),
            border: if selected || lit {
                Border { color: tokens.accent, width: 2.0, radius: 6.0.into() }
            } else {
                Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 6.0.into() }
            },
            ..container::Style::default()
        });
    let body = column![
        face,
        text(elide(&name, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
    ]
    .spacing(8)
    .width(width);

    let hover_id = id.clone();
    let right = if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        ContextTarget::Row(id.clone())
    };
    let action = button(body)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status));
    let action = if library_core::id::is_shelf(&target) {
        action.on_press(Message::CardTap(id))
    } else {
        // Listed but dead: no press, nothing to open.
        action
    };
    let cell: Element<'static, Message> = mouse_area(action)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into();
    let mut dressed: Vec<Element<'static, Message>> = vec![cell];
    if held {
        dressed.push(held_layer(tokens));
    } else if selection.selecting && !selected {
        dressed.push(dim_layer(tokens, hovered));
    }
    if seam {
        dressed.push(seam_corner(tokens, cover_h));
    }
    if fold {
        dressed.push(fold_ring(tokens, cover_h));
    }
    if drag.live() {
        dressed.push(sensors(&sensor_id, false));
    }
    if dressed.len() == 1 {
        dressed.pop().unwrap_or_else(|| Space::new().into())
    } else {
        Stack::with_children(dressed).into()
    }
}

/// The grid's last cell: the add door. A cover-shaped tile with the plus,
/// quiet until the pointer asks for it.
pub fn add_card(tokens: Tokens, width: f32) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    button(
        container(icon(IconName::Plus, 32, tokens.muted))
            .width(width)
            .height(cover_h)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .padding(0)
    .style(move |_, status| {
        let background = match status {
            button::Status::Hovered => Some(Background::Color(wash(tokens.surface, 0.60))),
            button::Status::Pressed => Some(Background::Color(tokens.surface)),
            _ => Some(Background::Color(Color::TRANSPARENT)),
        };
        button::Style {
            background,
            border: Border { color: tokens.line, width: 1.0, radius: 6.0.into() },
            text_color: tokens.ink,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleMenu(MenuKind::Add))
    .into()
}

/// The list's book thumbnail: the cover's gradient in the row's footprint.
pub fn list_thumb(tokens: Tokens, check: Option<bool>) -> Element<'static, Message> {
    let width = 41.6;
    let base: Element<'static, Message> =
        container(Space::new().width(width).height(width * COVER_RATIO))
            .style(move |_| container::Style {
                background: Some(Background::Gradient(cover_gradient(tokens))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 3.0.into() },
                shadow: Shadow {
                    color: wash(Color::BLACK, 0.16),
                    offset: Vector::new(0.0, 1.0),
                    blur_radius: 2.0,
                },
                ..container::Style::default()
            })
            .into();
    let Some(selected) = check else {
        return base;
    };
    // The row's thumbnail is small enough that the mark covers it rather
    // than sitting in a corner of it: the wash over the art, the check
    // centred on the wash.
    let mut layers = vec![base];
    layers.push(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(Color::BLACK, 0.34))),
                ..container::Style::default()
            })
            .into(),
    );
    if selected {
        layers.push(
            container(icon(IconName::Check, 13, tokens.paper))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into(),
        );
    }
    Stack::with_children(layers).width(width).height(width * COVER_RATIO).into()
}

/// The list's format chip, worn only by the formats that are not PDF — the
/// default needs no announcement.
pub fn format_chip(tokens: Tokens, format: Format) -> Option<Element<'static, Message>> {
    if format == Format::Pdf {
        return None;
    }
    Some(
        container(text(format.label()).size(11).color(tokens.muted))
            .padding(Padding { top: 2.0, right: 7.0, bottom: 2.0, left: 7.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
                ..container::Style::default()
            })
            .into(),
    )
}

/// The row's shared chrome: nothing at rest, the line's wash under the
/// pointer — and, for a row in the set, the accent's tint of membership
/// (select.css: accent at 10%, a shade deeper under the pointer).
pub fn row_button_style(tokens: Tokens, status: button::Status, selected: bool) -> button::Style {
    let background = if selected {
        Some(Background::Color(wash(
            tokens.accent,
            match status {
                button::Status::Hovered | button::Status::Pressed => 0.18,
                _ => 0.10,
            },
        )))
    } else {
        match status {
            button::Status::Hovered => Some(Background::Color(wash(tokens.line, 0.45))),
            button::Status::Pressed => Some(Background::Color(wash(tokens.line, 0.70))),
            _ => None,
        }
    };
    button::Style {
        background,
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
        text_color: tokens.ink,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Cut a line of text to what one row can hold — the list's single-line
/// elision, wider than any real row so only the truly long are touched.
pub fn elide_line(s: &str) -> String {
    elide(s, 96)
}
