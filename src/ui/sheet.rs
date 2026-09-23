//! The app's modal surfaces: a panel in the middle of the window over a
//! scrim that cancels on press.
//!
//! The web app slid its sheets up from below; natively the panel sits
//! centred, because there is no page to slide over. What stays is the
//! contract: one question at a time, an escape hatch on the scrim and the
//! Escape key, and the affirmative button on the right.

use iced::widget::{button, container, mouse_area, stack, text, Column, Row, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Shadow, Vector};

use crate::theme::{wash, Tokens};

/// The panel's width — wide enough for a question and a name, narrow enough
/// to read as a card over the shelf.
const SHEET_W: f32 = 380.0;

const MEDIUM: Font = Font { weight: iced::font::Weight::Medium, ..Font::DEFAULT };

/// A sheet over its scrim: the scrim cancels, the panel floats centred.
pub fn overlay<'a, M: Clone + 'a>(panel: Element<'a, M>, cancel: M) -> Element<'a, M> {
    stack![
        mouse_area(container(Space::new().width(Length::Fill).height(Length::Fill)))
            .on_press(cancel),
        container(panel)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(24.0)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The panel itself: a title, a body, and the action row along the bottom
/// edge — cancel on the left of the cluster, the answer at the right edge.
pub fn panel<'a, M: Clone + 'a>(
    tokens: Tokens,
    title: &'a str,
    body: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    container(
        Column::new()
            .push(text(title).size(15).font(MEDIUM).color(tokens.ink))
            .push(body)
            .push(
                container(Row::with_children(actions).spacing(8).align_y(Alignment::Center))
                    .width(Length::Fill)
                    .align_x(Alignment::End),
            )
            .spacing(14)
            .width(Length::Fill),
    )
    .width(SHEET_W)
    .padding(Padding { top: 20.0, right: 20.0, bottom: 20.0, left: 20.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(tokens.surface)),
        border: Border { color: tokens.line, width: 1.0, radius: 14.0.into() },
        shadow: Shadow {
            color: wash(Color::BLACK, 0.35),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 36.0,
        },
        ..container::Style::default()
    })
    .into()
}

/// The sheet's cancel button: quiet, outlined.
pub fn cancel_button<'a, M: Clone + 'a>(
    tokens: Tokens,
    label: &'a str,
    message: M,
) -> Element<'a, M> {
    button(text(label).size(13).color(tokens.ink))
        .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
        .style(move |_, status| {
            let background = match status {
                button::Status::Hovered | button::Status::Pressed => wash(tokens.ink, 0.06),
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(background)),
                border: Border { color: tokens.line, width: 1.0, radius: 8.0.into() },
                text_color: tokens.ink,
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .on_press(message)
        .into()
}

/// The sheet's affirmative button: filled with the accent — or with the
/// danger red when the answer takes something away.
pub fn confirm_button<'a, M: Clone + 'a>(
    tokens: Tokens,
    label: &'a str,
    message: M,
    danger: bool,
) -> Element<'a, M> {
    let fill = if danger { crate::theme::DANGER } else { tokens.accent };
    button(text(label).size(13).color(Color::WHITE))
        .padding(Padding { top: 6.0, right: 14.0, bottom: 6.0, left: 14.0 })
        .style(move |_, status| {
            let alpha = match status {
                button::Status::Hovered => 0.88,
                _ => 1.0,
            };
            button::Style {
                background: Some(Background::Color(wash(fill, alpha))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
                text_color: Color::WHITE,
                shadow: Shadow::default(),
                snap: false,
            }
        })
        .on_press(message)
        .into()
}
