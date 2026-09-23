//! The caption clusters: the window commands each desktop expects, drawn
//! the way that desktop draws them.
//!
//! macOS owns its traffic lights — AppKit paints them over the content when
//! the window asks for a transparent, full-size-content titlebar, so this
//! module returns nothing there and the bar keeps a left inset clear of
//! them. Windows gets its square 46px full-height buttons, the close one
//! flushing red under the pointer. Linux gets the GNOME family: 24px
//! circles with a translucent fill that tracks the theme's ink.

use iced::widget::{button, row, Button, Space};
use iced::{Alignment, Background, Color, Element, Length};

use super::icons::{icon, IconName};
use super::platform::{Os, GNOME_BUTTON_D, WIN_CAPTION_W};
use super::titlebar::{Message, WindowAction};
use crate::theme::{fade, Tokens};

/// The cluster for a platform: nothing on macOS, squares on Windows,
/// circles on GNOME.
pub fn view<'a>(tokens: Tokens, os: Os, maximized: bool, factor: f32) -> Element<'a, Message> {
    match os {
        Os::MacOs => Space::new().into(),
        Os::Windows => windows(tokens, maximized, factor),
        Os::Linux => gnome(tokens, maximized, factor),
    }
}

/// The Windows caption family: minimize, maximize/restore, close. Full
/// height, no rounding, a surface wash under the pointer and the platform's
/// own red under the close button.
fn windows<'a>(tokens: Tokens, maximized: bool, factor: f32) -> Element<'a, Message> {
    let max_glyph =
        if maximized { IconName::WindowRestore } else { IconName::WindowMaximize };
    row![
        win_button(tokens, factor, IconName::WindowMinimize, WindowAction::Minimize, false),
        win_button(tokens, factor, max_glyph, WindowAction::ToggleMaximize, false),
        win_button(tokens, factor, IconName::Close, WindowAction::Close, true),
    ]
    .height(Length::Fill)
    .into()
}

fn win_button<'a>(
    tokens: Tokens,
    factor: f32,
    glyph: IconName,
    action: WindowAction,
    is_close: bool,
) -> Button<'a, Message> {
    button(icon(glyph, 12, fade(tokens.ink, factor)))
        .width(WIN_CAPTION_W)
        .height(Length::Fill)
        .padding(0)
        .style(move |_, status| win_style(tokens, factor, status, is_close))
        .on_press(Message::Window(action))
}

/// The Windows close red, `#e81123`.
const WIN_CLOSE_RED: Color = Color::from_rgb(0.910, 0.067, 0.137);

fn win_style(tokens: Tokens, factor: f32, status: button::Status, is_close: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let (background, text_color) = match (is_close, hovered) {
        (true, true) => (Some(WIN_CLOSE_RED), Color::WHITE),
        (_, true) => (
            Some(fade(
                match status {
                    button::Status::Pressed => tokens.line,
                    _ => tokens.surface,
                },
                factor,
            )),
            fade(tokens.ink, factor),
        ),
        (_, false) => (None, fade(tokens.ink, factor)),
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
fn gnome<'a>(tokens: Tokens, maximized: bool, factor: f32) -> Element<'a, Message> {
    let max_glyph =
        if maximized { IconName::WindowRestore } else { IconName::WindowMaximize };
    row![
        gnome_button(tokens, factor, IconName::WindowMinimize, WindowAction::Minimize),
        gnome_button(tokens, factor, max_glyph, WindowAction::ToggleMaximize),
        gnome_button(tokens, factor, IconName::Close, WindowAction::Close),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn gnome_button<'a>(
    tokens: Tokens,
    factor: f32,
    glyph: IconName,
    action: WindowAction,
) -> Button<'a, Message> {
    button(icon(glyph, 12, fade(tokens.ink, factor)))
        .width(GNOME_BUTTON_D)
        .height(GNOME_BUTTON_D)
        .padding(0)
        .style(move |_, status| gnome_style(tokens, factor, status))
        .on_press(Message::Window(action))
}

fn gnome_style(tokens: Tokens, factor: f32, status: button::Status) -> button::Style {
    let wash = match status {
        button::Status::Hovered => Some(Color { a: 0.16 * factor, ..tokens.ink }),
        button::Status::Pressed => Some(Color { a: 0.24 * factor, ..tokens.ink }),
        _ => Some(Color { a: 0.08 * factor, ..tokens.ink }),
    };
    button::Style {
        background: wash.map(Background::Color),
        border: iced::Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: (GNOME_BUTTON_D / 2.0).into(),
        },
        text_color: fade(tokens.ink, factor),
        shadow: iced::Shadow::default(),
        snap: false,
    }
}
