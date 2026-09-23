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
use iced::widget::{button, column, container, mouse_area, stack, text, Column, Row, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Gradient, Length, Padding, Radians, Shadow,
    Vector,
};

use library_core::book::Book;
use library_core::shelf::Shelf;
use library_core::text::{self as lib_text, plural};
use reader_core::format::Format;

use crate::app::{ContextTarget, MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::theme::{mix, wash, Tokens};

/// The cover's aspect, A4 portrait: height = width × 297/210.
pub const COVER_RATIO: f32 = 297.0 / 210.0;
/// The most member covers a folder plate shows, in its 2×2 window.
const THUMB_CAP: usize = 4;

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
fn cover_style(tokens: Tokens, hovered: bool) -> container::Style {
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
        border: Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 6.0.into() },
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

/// One book's card in the grid: cover, info, and the progress hairline.
/// The card OWNS its book — the level's rows are computed fresh on every
/// view, and an element may not borrow a vec that dies with the function
/// that built it.
pub fn book_card(tokens: Tokens, book: Book, width: f32, hovered: bool) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let title = book.title();
    let sub = book
        .author()
        .unwrap_or_else(|| lib_text::page_line(book.page, book.num_pages));
    let progress = book.progress();
    let missing = book.missing;

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

    let face: Element<'static, Message> = if missing {
        stack![
            title_layer,
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .style(move |_| container::Style {
                    background: Some(Background::Color(wash(tokens.paper, 0.55))),
                    ..container::Style::default()
                }),
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
            .align_y(Alignment::Start),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        title_layer.into()
    };

    let cover = container(face)
        .width(width)
        .height(cover_h)
        .style(move |_| cover_style(tokens, hovered));

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

    let hover_id = book.id.clone();
    let right_id = book.id.clone();
    let click = button(card)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status))
        .on_press(Message::OpenBook(book.id));
    mouse_area(click)
        .on_enter(Message::CardHover(Some(hover_id)))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(ContextTarget::Row(right_id)))
        .into()
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

/// One folder's plate: a 3:4 card whose window shows up to four of its
/// members' covers, the name and the count beneath. Like the book's card,
/// the plate owns its shelf.
pub fn folder_card(tokens: Tokens, shelf: Shelf, width: f32) -> Element<'static, Message> {
    let plate_h = width * 4.0 / 3.0;
    let members = shelf.books.len().min(THUMB_CAP);

    // The seam the tiles float on — the grid.css mix of the line and the
    // muted ink — shows through the 3px gaps and the 6px inset.
    let tile_w = (width - 12.0 - 3.0) / 2.0;
    let tile_h = (plate_h - 12.0 - 3.0) / 2.0;

    let mut tiles: Vec<Element<'static, Message>> = Vec::with_capacity(2);
    for line_ix in 0..2usize {
        let mut line = Row::new().spacing(3);
        for slot_ix in 0..2usize {
            line = if line_ix * 2 + slot_ix < members {
                line.push(
                    container(Space::new().width(tile_w).height(tile_h))
                        .style(move |_| container::Style {
                            background: Some(Background::Gradient(cover_gradient(tokens))),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 3.0.into(),
                            },
                            ..container::Style::default()
                        }),
                )
            } else {
                line.push(
                    container(Space::new().width(tile_w).height(tile_h))
                        .style(move |_| container::Style {
                            background: Some(Background::Color(wash(tokens.surface, 0.50))),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 0.0,
                                radius: 3.0.into(),
                            },
                            ..container::Style::default()
                        }),
                )
            };
        }
        tiles.push(line.into());
    }

    let plate = container(Column::with_children(tiles).spacing(3))
        .width(width)
        .height(plate_h)
        .padding(6.0)
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(mix(tokens.line, tokens.muted, 0.20), 0.35))),
            border: Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 8.0.into() },
            shadow: Shadow {
                color: wash(Color::BLACK, 0.18),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..container::Style::default()
        });

    let count = plural(shelf.books.len(), "book", "books");
    let name = shelf.name.clone();
    let right_id = shelf.id.clone();
    let click = button(
        column![
            plate,
            text(elide(&name, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
            text(count).size(12).color(tokens.muted),
        ]
        .spacing(8)
        .width(width),
    )
    .padding(0)
    .style(move |_, status| card_button_style(tokens, status))
    .on_press(Message::Navigate(shelf.id));
    mouse_area(click)
        .on_right_press(Message::ContextMenu(ContextTarget::Folder(right_id)))
        .into()
}

/// A row that points at a shelf rather than being a book: the link glyph on
/// the cover's tile. A link whose target is not a shelf is listed but
/// quiet — nothing to open until the shelves it may name exist.
pub fn link_card(
    tokens: Tokens,
    id: String,
    name: String,
    target: String,
    width: f32,
) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let face = container(icon(IconName::Link, 28, wash(tokens.muted, 0.90)))
        .width(width)
        .height(cover_h)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.surface, 0.60))),
            border: Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 6.0.into() },
            ..container::Style::default()
        });
    let body = column![
        face,
        text(elide(&name, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
    ]
    .spacing(8)
    .width(width);

    let action = button(body)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status));
    let action = if library_core::id::is_shelf(&target) {
        action.on_press(Message::Navigate(target))
    } else {
        // Listed but dead: no press, nothing to open.
        action
    };
    mouse_area(action)
        .on_right_press(Message::ContextMenu(ContextTarget::Row(id)))
        .into()
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
pub fn list_thumb(tokens: Tokens) -> Element<'static, Message> {
    let width = 41.6;
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
        .into()
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
/// pointer.
pub fn row_button_style(tokens: Tokens, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(Background::Color(wash(tokens.line, 0.45))),
        button::Status::Pressed => Some(Background::Color(wash(tokens.line, 0.70))),
        _ => None,
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
