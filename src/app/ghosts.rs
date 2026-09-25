//! The drag's own drawing: the lifted cells, the count badge and the fold
//! plate the sink aims at.
use iced::border::Radius;
use iced::widget::{column, container, text, Column, Row, Space, Stack};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Point};
use library_core::blob::LibraryBlob;
use library_core::book::{self};
use library_core::shelf::{self};

use crate::chrome::icons::{icon, IconName};
use crate::library::card::{plate_seam, THUMB_CAP};
use crate::library::drag::{DragPayload, FoldPreview};
use crate::theme::{mix, Elevation, Tokens};
use super::message::Message;
use super::selection::Drag;


/// The sunk ghost's scale: a third of its size is what keeps the crumb it
/// covers readable.
const SUNK_SCALE: f32 = 0.38;

/// The ghost's box: the web layer's own 9rem cover at A4 proportion
/// (drag.css's 144×204), plus the fan's headroom — the reference shifts
/// each tile out of the stack by (7n, −6n) pixels, so the stack needs
/// 21px of right and 18px of top before its first tile.
const GHOST_W: f32 = 144.0;

const GHOST_H: f32 = 203.7;

const FAN_RISE: f32 = 18.0;

const FAN_SHIFT: f32 = 21.0;

/// The drag layer: the payload drawn at the pointer — the fan of cover
/// tiles a drag lifts, or, once the fold is brewing, the plate of the
/// shelf the drop will make. Non-interactive by construction: a wash of
/// containers the pointer never meets, so every press underneath keeps
/// working while the ghost rides above them.
pub(super) fn ghost_layer(
    tokens: Tokens,
    drag: &Drag,
    library: &LibraryBlob,
    fold: Option<&FoldPreview>,
    at: Point,
) -> Element<'static, Message> {
    // Sunk, which happens on one kind of target only: a titlebar crumb.
    // At full size the ghost covers the name of the very level the reader
    // is aiming at, so the anchor moves to the parked spot's centre, the
    // ghost shrinks to a third and the crumb stays readable — the web
    // layer's own translate(-50%,-50%) and scale(0.38). Its 0.85 opacity
    // rides the same shrink; an iced container carries no opacity, and
    // the third-size ghost uncovers the crumb whatever its alpha.
    let scale = if drag.sunk.is_some() { SUNK_SCALE } else { 1.0 };
    let ghost: Element<'static, Message> = match fold {
        Some(preview) => fold_plate(tokens, preview.filled),
        None => ghost_fan(tokens, &ghost_tiles(library, &drag.payload), drag.payload.len(), scale),
    };
    let (left, top) = match drag.sunk {
        Some(spot) => (
            (spot.x - 0.5 * GHOST_W * scale).max(0.0),
            (spot.y - 0.5 * GHOST_H * scale).max(0.0),
        ),
        // The web layer anchors the ghost so the pointer sits at 38% of
        // the cover's width and 32% of its height; the headroom offsets
        // the box by the fan's own rise on top of that.
        None => ((at.x - 0.38 * GHOST_W).max(0.0), (at.y - 0.32 * GHOST_H - FAN_RISE).max(0.0)),
    };
    container(ghost)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top, right: 0.0, bottom: 0.0, left })
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .into()
}

/// The fan's tiles: the payload's own labels, books first — the order the
/// web ghost stacks them in. A book a scan just took off the shelf draws
/// no tile rather than an empty one.
fn ghost_tiles(library: &LibraryBlob, payload: &DragPayload) -> Vec<(String, bool)> {
    let mut tiles: Vec<(String, bool)> = Vec::new();
    for id in &payload.books {
        let Some(row) = book::find_row(&library.books, id) else { continue };
        let label = match row {
            book::Row::Book(book) => book.title(),
            book::Row::Link { name, .. } => name.clone(),
        };
        tiles.push((label, false));
    }
    for id in &payload.folders {
        let name = shelf::find(&library.shelves, id)
            .map(|each| each.name.clone())
            .unwrap_or_default();
        tiles.push((name, true));
    }
    tiles
}

/// The first letter, uppercased — the ghost tile's stand-in cover, the
/// same initial the web ghost letters a tile with.
fn initial(label: &str) -> String {
    label.chars().next().map(|letter| letter.to_uppercase().collect()).unwrap_or_default()
}

/// The fan itself: at most `THUMB_CAP` tiles, each shifted up-and-right
/// out of the stack (drag.css's `translate(7n, -6n)`), with the payload's
/// count on the corner when it is more than one. The web fan's per-tile
/// rotation is the one thing left out: a 0.14 container carries no
/// rotation, and the stepped stack reads the same without it.
fn ghost_fan(
    tokens: Tokens,
    tiles: &[(String, bool)],
    total: usize,
    scale: f32,
) -> Element<'static, Message> {
    let (w, h) = (GHOST_W * scale, GHOST_H * scale);
    let (rise, shift) = (FAN_RISE * scale, FAN_SHIFT * scale);
    let mut layers: Vec<Element<'static, Message>> = Vec::new();
    for (fan, (label, folder)) in tiles.iter().take(THUMB_CAP).enumerate() {
        let face: Element<'static, Message> = if *folder {
            container(icon(IconName::Open, (16.0 * scale) as u16, tokens.muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        } else {
            container(text(initial(label)).size(36.0 * scale).color(tokens.muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        };
        let tile = container(face)
            .width(w)
            .height(h)
            .style(move |_| container::Style {
                background: Some(Background::Color(tokens.surface)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    // The web tile's own book-spine corners: 3px on the
                    // spine side, 6px on the fore-edge.
                    radius: Radius {
                        top_left: 3.0,
                        top_right: 6.0,
                        bottom_right: 6.0,
                        bottom_left: 3.0,
                    },
                },
                shadow: Elevation::Plate.shadow(),
                ..container::Style::default()
            });
        layers.push(
            container(tile)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: rise - 6.0 * scale * fan as f32,
                    right: 0.0,
                    bottom: 0.0,
                    left: 7.0 * scale * fan as f32,
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        );
    }
    if total > 1 {
        layers.push(
            container(count_badge(tokens, total, scale))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: rise - 8.0 * scale,
                    right: shift - 8.0 * scale,
                    bottom: 0.0,
                    left: 0.0,
                })
                .align_x(Alignment::End)
                .align_y(Alignment::Start)
                .into(),
        );
    }
    container(Stack::with_children(layers)).width(w + shift).height(h + rise).into()
}

/// The payload's count, on the fan's top corner: the accent's pill the web
/// ghost wears when the drag is carrying more than one.
fn count_badge(tokens: Tokens, total: usize, scale: f32) -> Element<'static, Message> {
    container(text(total.to_string()).size(11.0 * scale).color(tokens.paper))
        .padding(Padding {
            top: 3.0 * scale,
            right: 7.0 * scale,
            bottom: 3.0 * scale,
            left: 7.0 * scale,
        })
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
            shadow: Elevation::Badge.shadow(),
            ..container::Style::default()
        })
        .into()
}

/// The fold's promise, in the ghost's own hands: the folder card's plate
/// drawn the way the drop would leave it — `filled` cells wearing the
/// accent's tint because the shelf does not exist yet, the next cell
/// holding the plus that says the plate is still taking items, and the
/// label naming what the drop will do.
fn fold_plate(tokens: Tokens, filled: usize) -> Element<'static, Message> {
    const W: f32 = 136.0;
    let plate_h = W * 3.0 / 4.0;
    let gap = 3.0;
    let cell_w = (W - gap) / 2.0;
    let cell_h = (plate_h - gap) / 2.0;
    let mut lines: Vec<Element<'static, Message>> = Vec::with_capacity(2);
    for line_ix in 0..2usize {
        let mut line: Row<'static, Message> = Row::new().spacing(gap);
        for slot_ix in 0..2usize {
            let at = line_ix * 2 + slot_ix;
            let cell: Element<'static, Message> = if at < filled {
                container(Space::new().width(cell_w).height(cell_h))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.surface,
                            tokens.accent,
                            0.34,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            } else if at == filled {
                container(icon(IconName::Plus, 14, tokens.accent))
                    .width(cell_w)
                    .height(cell_h)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.surface,
                            tokens.accent,
                            0.10,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            } else {
                container(Space::new().width(cell_w).height(cell_h))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.paper,
                            tokens.surface,
                            0.72,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            };
            line = line.push(cell);
        }
        lines.push(line.into());
    }
    let plate = container(Column::with_children(lines).spacing(gap))
        .width(W)
        .height(plate_h)
        .style(move |_| container::Style {
            background: Some(Background::Color(plate_seam(tokens))),
            border: Border { color: plate_seam(tokens), width: 1.0, radius: 8.0.into() },
            shadow: Elevation::Float.shadow(),
            ..container::Style::default()
        });
    container(
        column![
            plate,
            container(text("New shelf").size(11).color(tokens.accent))
                .width(Length::Fill)
                .center_x(Length::Fill),
        ]
        .spacing(5)
        .width(W),
    )
    .into()
}
