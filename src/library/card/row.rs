//! What a list row borrows from the cell kit: its thumbnail, the format chip,
//! the row's own button face, and the one-line elision.

use iced::widget::{button, container, text, Space, Stack};
use iced::{Background, Border, Color, Element, Length, Padding, Shadow};
use reader_core::format::Format;
use crate::app::Message;
use crate::chrome::icons::{icon, IconName};
use crate::theme::{wash, Elevation, Tokens};
use super::kit::{COVER_RATIO, cover_gradient, elide};

/// The list row's own numbers: the thumbnail's box, the air a row keeps
/// around its content, and the pitch it takes — the padding twice, the
/// thumbnail, and the hairline between rows. The reveal's scroll reads the
/// pitch too.
pub const THUMB_W: f32 = 41.6;
pub const ROW_PAD: f32 = 8.0;
pub const ROW_H: f32 = ROW_PAD * 2.0 + 1.0 + THUMB_W * COVER_RATIO;

/// The list's book thumbnail: the cover's gradient in the row's footprint.
pub fn list_thumb(tokens: Tokens, check: Option<bool>) -> Element<'static, Message> {
    let width = THUMB_W;
    let base: Element<'static, Message> =
        container(Space::new().width(width).height(width * COVER_RATIO))
            .style(move |_| container::Style {
                background: Some(Background::Gradient(cover_gradient(tokens))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 3.0.into() },
                shadow: Elevation::Thumb.shadow(),
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
