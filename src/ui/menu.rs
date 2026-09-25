//! The floating menu panels: the popover vocabulary the shelf's bar speaks.
//!
//! The web app drew its menus as DOM popovers anchored to their buttons.
//! Natively a menu is a layer on the application's stack, anchored at the
//! last known pointer position and placed by `ui_geom::floating` — the same
//! clamp arithmetic the web app's context menus ran, so a menu near an edge
//! flips and slides back into the window exactly as it did there.
//!
//! This module owns the panel itself (shell.css's surface-popover: a
//! surface card, a hairline, 12px corners, the float shadow) and the row
//! primitives inside it: icon-plus-label items with a check slot, section
//! captions, hairline separators and the small pill toggles.
//!
//! Placement needs a size before layout exists, so every builder shares the
//! row metrics below and the menus that stack them keep a running height on
//! the same numbers.

use std::borrow::Cow;

use iced::widget::{button, column, container, row, text, Column, Space};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Shadow, Size,
};

use crate::chrome::icons::{icon, IconName};
use crate::theme::{mix, wash, DANGER, Elevation, Tokens};

/// One menu item's height: 6px of padding around an 18px line.
pub const ROW_H: f32 = 30.0;
/// An item carrying a sublabel: the row plus its second, smaller line.
pub const TALL_ROW_H: f32 = 46.0;
/// A section caption's height.
pub const SECTION_H: f32 = 24.0;
/// A separator's height: the hairline plus its 4px margins.
pub const SEP_H: f32 = 9.0;
/// A small toggle pill's height (the columns stepper's "Auto", the sort
/// direction pair).
pub const TOGGLE_H: f32 = 28.0;
/// The panel's inner padding, all sides.
pub const PAD: f32 = 8.0;
/// How far a placed panel keeps itself from the window's edges.
const VIEWPORT_MARGIN: f64 = 8.0;

/// Place a panel of `size` at `anchor`, clamped into the viewport — the
/// context-menu rule: the panel's top-left goes where the pointer is, and
/// the clamp pulls it back when that would hang it off an edge.
pub fn place(anchor: Point, size: Size, viewport: Size) -> Point {
    let placed = ui_geom::floating::place_context_menu(
        point_at(anchor),
        size_of(size),
        size_of(viewport),
        VIEWPORT_MARGIN,
    );
    Point::new(placed.rect.x as f32, placed.rect.y as f32)
}

/// The app's `f32` geometry as the crate's `f64` geometry. Not `From` impls:
/// the orphan rule forbids them here (both the trait and the type are
/// foreign), and the conversion cannot move into the crate without giving it
/// a dependency on iced — which is what keeps it testable on its own.
fn point_at(point: Point) -> ui_geom::floating::Point {
    ui_geom::floating::Point::new(f64::from(point.x), f64::from(point.y))
}

fn size_of(size: Size) -> ui_geom::floating::Size {
    ui_geom::floating::Size::new(f64::from(size.width), f64::from(size.height))
}

/// The panel frame: rows inside, the surface-popover card around them.
pub fn popover<'a, M: Clone + 'a>(
    tokens: Tokens,
    rows: Vec<Element<'a, M>>,
    width: f32,
) -> Element<'a, M> {
    container(Column::with_children(rows).spacing(0).width(Length::Fill))
        .width(width)
        .padding(PAD)
        .style(move |_| panel_style(tokens))
        .into()
}

/// shell.css's `surface-popover` and `--shadow-float`, in one style.
fn panel_style(tokens: Tokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.surface)),
        border: Border { color: tokens.line, width: 1.0, radius: 12.0.into() },
        shadow: Elevation::Float.shadow(),
        ..container::Style::default()
    }
}

/// One menu item: the icon slot, the label (with an optional second line),
/// and the trailing check slot. `None` for the message renders the row
/// disabled — listed, but not quietly dropped.
///
/// The label is a `Cow`, so a row whose name is computed at build time — a
/// restore row, a watch row — needs no second builder.
pub fn item<'a, M: Clone + 'a>(
    tokens: Tokens,
    glyph: Option<IconName>,
    label: impl Into<Cow<'a, str>>,
    sublabel: Option<Cow<'a, str>>,
    checked: bool,
    message: Option<M>,
) -> Element<'a, M> {
    let label: Cow<'a, str> = label.into();
    let icon_slot: Element<'a, M> = match glyph {
        Some(name) => container(icon(name, 15, tokens.muted)).width(16.0).into(),
        None => Space::new().width(16.0).into(),
    };
    let labels: Element<'a, M> = match sublabel {
        Some(sub) => column![
            text(label).size(13).color(tokens.ink),
            text(sub).size(11).color(tokens.muted),
        ]
        .spacing(1)
        .width(Length::Fill)
        .into(),
        None => container(text(label).size(13).color(tokens.ink))
            .width(Length::Fill)
            .into(),
    };
    let check_slot: Element<'a, M> = if checked {
        container(icon(IconName::Check, 14, tokens.accent)).width(16.0).into()
    } else {
        Space::new().width(16.0).into()
    };

    let face = row![icon_slot, labels, check_slot].spacing(8).align_y(Alignment::Center);
    let action = button(face)
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
        .style(move |_, status| item_style(tokens, status));
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The item row's chrome: nothing at rest, a line wash under the pointer,
/// muted ink when the row is disabled.
fn item_style(tokens: Tokens, status: button::Status) -> button::Style {
    let wash_color = match status {
        button::Status::Hovered => Some(wash(tokens.line, 0.60)),
        button::Status::Pressed => Some(tokens.line),
        _ => None,
    };
    let ink = match status {
        button::Status::Disabled => tokens.muted,
        _ => tokens.ink,
    };
    button::Style {
        background: wash_color.map(Background::Color),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
        text_color: ink,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// The row that takes something away: the item's shape, the palette's
/// danger red, and an optional icon — the colour carries the warning, so a
/// row without one still lands its label where the others start.
pub fn danger_item<'a, M: Clone + 'a>(
    glyph: Option<IconName>,
    label: impl Into<Cow<'a, str>>,
    message: M,
) -> Element<'a, M> {
    let icon_slot: Element<'a, M> = match glyph {
        Some(name) => container(icon(name, 15, DANGER)).width(16.0).into(),
        None => Space::new().width(16.0).into(),
    };
    let label: Cow<'a, str> = label.into();
    let face = row![
        icon_slot,
        container(text(label).size(13).color(DANGER)).width(Length::Fill)
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    button(face)
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 })
        .style(move |_, status| danger_style(status))
        .on_press(message)
        .into()
}

/// The danger row's chrome: the item's wash, red ink throughout.
fn danger_style(status: button::Status) -> button::Style {
    let wash_color = match status {
        button::Status::Hovered => Some(wash(DANGER, 0.10)),
        button::Status::Pressed => Some(wash(DANGER, 0.18)),
        _ => None,
    };
    button::Style {
        background: wash_color.map(Background::Color),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
        text_color: DANGER,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A section caption: the small muted label the menu's groups open with.
pub fn section<'a, M: Clone + 'a>(tokens: Tokens, label: impl Into<Cow<'a, str>>) -> Element<'a, M> {
    let label: Cow<'a, str> = label.into();
    container(text(label).size(11).color(tokens.muted))
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 8.0, bottom: 4.0, left: 8.0 })
        .into()
}

/// The hairline between a menu's groups.
pub fn separator<M: Clone + 'static>(tokens: Tokens) -> Element<'static, M> {
    container(
        container(Space::new().width(Length::Fill).height(1.0)).style(move |_| {
            container::Style {
                background: Some(Background::Color(tokens.line)),
                ..container::Style::default()
            }
        }),
    )
    .padding(Padding { top: 4.0, right: 0.0, bottom: 4.0, left: 0.0 })
    .width(Length::Fill)
    .into()
}

/// The pressed-or-quiet control every selected-or-not button in the app wears:
/// the bordered accent chip the web's `ToggleButton` draws. `None` for the
/// message renders it inert.
pub fn toggle<'a, M: Clone + 'a>(
    tokens: Tokens,
    glyph: Option<IconName>,
    label: &'a str,
    active: bool,
    message: Option<M>,
) -> Element<'a, M> {
    let ink = if active { tokens.ink } else { tokens.muted };
    let face: Element<'a, M> = match glyph {
        Some(name) => row![icon(name, 12, ink), text(label).size(12).color(ink)]
            .spacing(4)
            .align_y(Alignment::Center)
            .into(),
        None => text(label).size(12).color(ink).into(),
    };
    let action = button(container(face).width(Length::Fill).center_x(Length::Fill))
        .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
        .style(move |_, status| {
            let background = if active {
                tokens.accent_soft
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => wash(tokens.line, 0.45),
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    color: if active { tokens.accent } else { tokens.line },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: ink,
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// A round stepper button: the columns minus and plus. Idle on the line's
/// wash, solid under the pointer, muted and dead when disabled.
pub fn stepper_button<M: Clone + 'static>(
    tokens: Tokens,
    glyph: IconName,
    message: Option<M>,
) -> Element<'static, M> {
    let idle = wash(tokens.line, 0.60);
    let action = button(icon(glyph, 13, tokens.ink))
        .width(22.0)
        .height(22.0)
        .padding(0)
        .style(move |_, status| {
            let (background, text_color) = match status {
                button::Status::Hovered => (Some(tokens.line), tokens.ink),
                button::Status::Pressed => (Some(mix(tokens.line, tokens.ink, 0.25)), tokens.ink),
                button::Status::Disabled => (Some(idle), tokens.muted),
                button::Status::Active => (Some(idle), tokens.ink),
            };
            button::Style {
                background: background.map(Background::Color),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 11.0.into() },
                text_color,
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The panel size a stack of rows adds up to — kept on the same metrics the
/// builders draw with, so placement clamps against what the panel really
/// occupies.
pub struct PanelSize {
    width: f32,
    height: f32,
}

impl PanelSize {
    pub fn new(width: f32) -> Self {
        Self { width, height: PAD * 2.0 }
    }

    /// Add one row's height; returns itself so the menus can tally as they
    /// build.
    pub fn row(mut self, height: f32) -> Self {
        self.height += height;
        self
    }

    pub fn size(self) -> Size {
        Size::new(self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_with_room_lands_where_the_pointer_is() {
        let at = place(Point::new(400.0, 300.0), Size::new(200.0, 150.0), Size::new(1200.0, 800.0));
        assert_eq!(at, Point::new(400.0, 300.0));
    }

    #[test]
    fn a_panel_past_an_edge_is_pulled_back_by_the_margin() {
        let viewport = Size::new(1200.0, 800.0);
        let panel = Size::new(200.0, 150.0);
        let at = place(Point::new(1150.0, 760.0), panel, viewport);
        assert!(at.x <= 1200.0 - 200.0 - VIEWPORT_MARGIN as f32 + 1e-4);
        assert!(at.y <= 800.0 - 150.0 - VIEWPORT_MARGIN as f32 + 1e-4);
        let at = place(Point::new(-20.0, -20.0), panel, viewport);
        assert!(at.x >= VIEWPORT_MARGIN as f32 - 1e-4);
        assert!(at.y >= VIEWPORT_MARGIN as f32 - 1e-4);
    }

    #[test]
    fn a_panel_wider_than_the_window_still_answers_a_finite_point() {
        // An oversized panel collapses the clamp range; it must not produce a
        // NaN, because the point goes straight into the layer's position.
        let at = place(Point::new(10.0, 10.0), Size::new(2000.0, 2000.0), Size::new(500.0, 400.0));
        assert!(at.x.is_finite() && at.y.is_finite());
    }

    #[test]
    fn a_stack_of_rows_tallies_the_metrics_it_draws_with() {
        let size = PanelSize::new(220.0)
            .row(ROW_H)
            .row(SECTION_H)
            .row(TALL_ROW_H)
            .row(SEP_H)
            .size();
        assert_eq!(size.width, 220.0);
        let rows = ROW_H + SECTION_H + TALL_ROW_H + SEP_H;
        assert_eq!(size.height, rows + PAD * 2.0, "the padding is on both edges");
    }
}
