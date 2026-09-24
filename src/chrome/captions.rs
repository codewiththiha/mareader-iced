//! The caption clusters: the window commands each desktop expects, drawn
//! the way that desktop draws them.
//!
//! macOS owns its traffic lights — AppKit paints them over the content when
//! the window asks for a transparent, full-size-content titlebar, so this
//! module returns nothing there and the bar keeps a left inset clear of
//! them. Windows gets its square 46px full-height buttons, the close one
//! flushing red under the pointer. Linux gets the GNOME family: 24px
//! circles with a translucent fill that tracks the theme's ink.
//!
//! Like the bar itself, the clusters are generic over the application's
//! message: every caption emits the chrome's own [`Message`] through the
//! `chrome` adapter the bar hands down.

use iced::widget::{button, row, Button, Space};
use iced::{Alignment, Background, Color, Element, Length};

use super::icons::{icon, IconName};
use super::desktop::{GNOME_BUTTON_D, WIN_CAPTION_W};
use crate::platform::Os;
use super::titlebar::{Message, WindowAction};
use crate::theme::{wash, Tokens};

/// The cluster for a platform: nothing on macOS, squares on Windows,
/// circles on GNOME.
pub fn view<'a, M: Clone + 'a>(
    tokens: Tokens,
    os: Os,
    maximized: bool,
    factor: f32,
    chrome: fn(Message) -> M,
) -> Element<'a, M> {
    match os {
        Os::Mac => Space::new().into(),
        Os::Windows => windows(tokens, maximized, factor, chrome),
        Os::Linux => gnome(tokens, maximized, factor, chrome),
    }
}

/// The Windows caption family: minimize, maximize/restore, close. Full
/// height, no rounding, a surface wash under the pointer and the platform's
/// own red under the close button.
fn windows<'a, M: Clone + 'a>(
    tokens: Tokens,
    maximized: bool,
    factor: f32,
    chrome: fn(Message) -> M,
) -> Element<'a, M> {
    let max_glyph =
        if maximized { IconName::WindowRestore } else { IconName::WindowMaximize };
    row![
        win_button(tokens, factor, IconName::WindowMinimize, WindowAction::Minimize, false, chrome),
        win_button(tokens, factor, max_glyph, WindowAction::ToggleMaximize, false, chrome),
        win_button(tokens, factor, IconName::Close, WindowAction::Close, true, chrome),
    ]
    .height(Length::Fill)
    .into()
}

fn win_button<'a, M: Clone + 'a>(
    tokens: Tokens,
    factor: f32,
    glyph: IconName,
    action: WindowAction,
    is_close: bool,
    chrome: fn(Message) -> M,
) -> Button<'a, M> {
    button(icon(glyph, 12, wash(tokens.ink, factor)))
        .width(WIN_CAPTION_W)
        .height(Length::Fill)
        .padding(0)
        .style(move |_, status| win_style(tokens, factor, status, is_close))
        .on_press(chrome(Message::Window(action)))
}

/// The Windows close red, `#e81123`.
const WIN_CLOSE_RED: Color = Color::from_rgb(0.910, 0.067, 0.137);

fn win_style(tokens: Tokens, factor: f32, status: button::Status, is_close: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, text_color) = match (is_close, hovered) {
        (true, true) => (Some(WIN_CLOSE_RED), Color::WHITE),
        (_, true) => (
            Some(wash(
                match status {
                    button::Status::Pressed => tokens.line,
                    _ => tokens.surface,
                },
                factor,
            )),
            wash(tokens.ink, factor),
        ),
        (_, false) => (None, wash(tokens.ink, factor)),
    };
    button::Style {
        background: background.map(Background::Color),
        border: iced::Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
        text_color,
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// The GNOME caption family: the same three commands as translucent
/// circles, the maximize glyph swapping the same way.
fn gnome<'a, M: Clone + 'a>(
    tokens: Tokens,
    maximized: bool,
    factor: f32,
    chrome: fn(Message) -> M,
) -> Element<'a, M> {
    let max_glyph =
        if maximized { IconName::WindowRestore } else { IconName::WindowMaximize };
    row![
        gnome_button(tokens, factor, IconName::WindowMinimize, WindowAction::Minimize, chrome),
        gnome_button(tokens, factor, max_glyph, WindowAction::ToggleMaximize, chrome),
        gnome_button(tokens, factor, IconName::Close, WindowAction::Close, chrome),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn gnome_button<'a, M: Clone + 'a>(
    tokens: Tokens,
    factor: f32,
    glyph: IconName,
    action: WindowAction,
    chrome: fn(Message) -> M,
) -> Button<'a, M> {
    button(icon(glyph, 12, wash(tokens.ink, factor)))
        .width(GNOME_BUTTON_D)
        .height(GNOME_BUTTON_D)
        .padding(0)
        .style(move |_, status| gnome_style(tokens, factor, status))
        .on_press(chrome(Message::Window(action)))
}

fn gnome_style(tokens: Tokens, factor: f32, status: button::Status) -> button::Style {
    let backdrop = match status {
        button::Status::Hovered => Some(Color { a: 0.16 * factor, ..tokens.ink }),
        button::Status::Pressed => Some(Color { a: 0.24 * factor, ..tokens.ink }),
        _ => Some(Color { a: 0.08 * factor, ..tokens.ink }),
    };
    button::Style {
        background: backdrop.map(Background::Color),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: (GNOME_BUTTON_D / 2.0).into(),
        },
        text_color: wash(tokens.ink, factor),
        shadow: iced::Shadow::default(),
        snap: false,
    }
}
