//! The cell kit: the shapes every shelf cell is built from, and the rules a
//! cell and a list row both follow — what a cell wears for the shelf's
//! state, the bands a live drag reads, and the senses it answers with.

use iced::gradient::Linear;
use iced::widget::{button, container, mouse_area, Column, Space, Stack};
use iced::{Alignment, Background, Border, Color, Element, Gradient, Length, Radians, Shadow};
use crate::app::{ContextTarget, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::drag::Band;
use crate::library::{DragFacts, SelectionFacts};
use crate::theme::{mix, wash, Elevation, Tokens};

/// The cover's aspect, A4 portrait: height = width × 297/210.
pub const COVER_RATIO: f32 = 297.0 / 210.0;
/// The pitch a grid row keeps under its cover: the caption's two lines, the
/// air below the cover, and the room the progress hairline takes. A row is
/// as tall as its tallest cell, so this is the number the reveal's scroll
/// and the grid must agree on.
pub const META_H: f32 = 44.0;
/// The most member covers a folder plate shows, in its 2×2 window. The
/// drag's fold preview borrows the same cap: a preview of more cells would
/// promise a plate the library does not draw.
pub(crate) const THUMB_CAP: usize = 4;
/// Past this depth a folder cell draws as a glyph: a few pixels of a
/// nested plate is a smear, not a preview.
pub(super) const PLATE_DEPTH: usize = 2;

/// How many characters fit a line of `size`px text in `width` — the elision
/// budget. iced has no line-clamp; the shelf's texts are pre-cut to the
/// lines their block reserves so a long title cannot grow its card.
pub(super) fn chars_per_line(width: f32, size: f32) -> usize {
    ((width / (size * 0.52)) as usize).max(4)
}

/// Cut `s` to `max_chars` characters with an ellipsis — character-based on
/// purpose, since the budget above counts characters too.
pub(super) fn elide(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The web cover's fallback gradient: 18% of the accent into the paper,
/// 160 degrees.
pub(super) fn cover_gradient(tokens: Tokens) -> Gradient {
    Gradient::Linear(
        Linear::new(Radians(2.7925))
            .add_stop(0.0, mix(tokens.paper, tokens.accent, 0.18))
            .add_stop(0.74, tokens.paper),
    )
}

/// The cover frame: the gradient, the radius, and the shadow that deepens
/// under the pointer.
pub(super) fn cover_style(tokens: Tokens, hovered: bool, selected: bool) -> container::Style {
    let shadow = if hovered {
        Elevation::Hover.shadow()
    } else {
        Elevation::Pill.shadow()
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
pub(super) fn card_button_style(tokens: Tokens, status: button::Status) -> button::Style {
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
pub(super) fn check_corner(tokens: Tokens, selected: bool) -> Element<'static, Message> {
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
fn dim_layer(tokens: Tokens, hovered: bool) -> Element<'static, Message> {
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
fn sensors(id: &str, folder: bool) -> Element<'static, Message> {
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

/// The fade a cell wears for the shelf's state: a held cell fades first, and
/// a set's own members are left out of the dim. `None` when it wears none.
pub fn state_fade(
    tokens: Tokens,
    held: bool,
    selecting: bool,
    selected: bool,
    hovered: bool,
) -> Option<Element<'static, Message>> {
    if held {
        Some(held_layer(tokens))
    } else if selecting && !selected {
        Some(dim_layer(tokens, hovered))
    } else {
        None
    }
}

/// The bands a live drag reads on a cell, and nothing once it has ended.
pub fn drag_bands(drag: DragFacts, id: &str, folder: bool) -> Option<Element<'static, Message>> {
    drag.live().then(|| sensors(id, folder))
}

/// A cell and the layers it wears: the bare cell when it wears nothing, the
/// stack when it does.
pub fn stack_layers(mut layers: Vec<Element<'static, Message>>) -> Element<'static, Message> {
    if layers.len() == 1 {
        layers.pop().expect("the cell is the only layer")
    } else {
        Stack::with_children(layers).into()
    }
}

/// A cell's own senses: hover in and out, and the right-click that opens its
/// menu. The id goes into the hover message, so the cell knows itself.
pub fn sensed(
    cell: impl Into<Element<'static, Message>>,
    id: &str,
    right: ContextTarget,
) -> Element<'static, Message> {
    mouse_area(cell)
        .on_enter(Message::CardHover(Some(id.to_string())))
        .on_exit(Message::CardHover(None))
        .on_right_press(Message::ContextMenu(right))
        .into()
}

/// Who a right-click answers for: the set, when the cell is one of its
/// members, the cell's own target otherwise.
pub fn right_target(selection: SelectionFacts, selected: bool, own: ContextTarget) -> ContextTarget {
    if selection.selecting && selected {
        ContextTarget::Selection
    } else {
        own
    }
}

/// What a drag is holding: every cell in the payload fades — not only the
/// one the press began on — because the set the reader picked up has to
/// stay readable as a set. The web cells drop to 0.45 opacity; a layout
/// cell cannot fade its own ink, so the wash carries it, the same answer
/// the choosing step-back wears.
fn held_layer(tokens: Tokens) -> Element<'static, Message> {
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
pub(super) fn seam_corner(tokens: Tokens, height: f32) -> Element<'static, Message> {
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
pub(super) fn fold_ring(tokens: Tokens, height: f32) -> Element<'static, Message> {
    container(
        container(Space::new().width(Length::Fill).height(height)).style(move |_| {
            container::Style {
                background: None,
                border: Border { color: tokens.accent, width: 2.0, radius: 6.0.into() },
                shadow: Elevation::Ring(tokens.accent).shadow(),
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
pub(super) fn nest_ring(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.accent, 0.12))),
            border: Border { color: tokens.accent, width: 2.0, radius: 10.0.into() },
            shadow: Elevation::Ring(tokens.accent).shadow(),
            ..container::Style::default()
        })
        .into()
}
