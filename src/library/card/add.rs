//! The add cell: the plus door that closes every grid and opens the add menu.

use iced::widget::{button, container};
use iced::{Background, Border, Color, Element, Length, Shadow};
use crate::app::{MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::theme::{wash, Tokens};
use super::kit::COVER_RATIO;


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
