//! The reading surface: the pages, the strip that scrolls them, the cover over a
//! fresh mount, the bar that moves it all, and the rail beside it.

mod bar;
mod sheet;
mod single;
mod stream;

use iced::widget::{button, column, container, row, stack, text, Space};
use iced::{Alignment, Background, Element, Length, Padding};

use super::document::DocStatus;
use super::sidebar;
use super::{Message, Reader};
use crate::theme::Tokens;
use crate::ui::buttons;

/// The surface.
pub(super) fn view(reader: &Reader, tokens: Tokens) -> Element<'_, Message> {
    container(desk(reader, tokens))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.paper)),
            ..container::Style::default()
        })
        .into()
}

/// The reading area and the chrome over it, with the rail beside or over it: a
/// sibling the page gives up width for, or a layer on top of it. Either way the
/// rail's own open and close is one motion (see `sidebar::rail`).
fn desk<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let book: Element<'a, Message> =
        stack(vec![reading_area(reader, tokens), bar::view(reader, tokens)])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    if reader.sidebar.docks() {
        return row![sidebar::docked(reader, tokens), book]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    // The rail over the page, and the edge that opens it out of the window's
    // own left border.
    stack(vec![
        book,
        sidebar::floating(reader, tokens),
        sidebar::edge(reader),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// What fills the reading area: the book, or the cover over a mount still racing
/// to the reader's own page. The bar stays over whatever it is.
fn reading_area<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    match reader.document.status {
        DocStatus::Error => centre(error_card(reader, tokens)),
        DocStatus::Opening => centre(opening(reader, tokens)),
        _ if reader.cover_up() => cover(tokens),
        _ if reader.viewer.mode.can_scroll() => stream::view(reader, tokens),
        _ => single::view(reader, tokens),
    }
}

/// The reading area's own box: the content centred on the desk.
fn centre<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// The cover: the reader's own paper, over the mount, until the page they left
/// off at has painted — so the first thing they see is their place rather than a
/// race toward it.
fn cover(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.paper)),
            ..container::Style::default()
        })
        .into()
}

/// The document is being read from disk: the sheet it will open on, and one
/// sentence, so the wait has something to look at.
fn opening<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let (width, height) = reader.page_box_px();
    column![
        sheet::view(width, height, None, tokens),
        text("Opening…").size(12).color(tokens.muted),
    ]
    .align_x(Alignment::Center)
    .spacing(12)
    .into()
}

/// A document that did not open: the sentence, and the way back.
fn error_card<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let sentence = reader
        .document
        .error
        .clone()
        .unwrap_or_else(|| "This document could not be opened.".to_string());
    let name = reader.name();
    column![
        text(name).size(16).color(tokens.ink),
        container(text(sentence).size(13).color(tokens.muted))
            .max_width(460.0)
            .center_x(Length::Fill),
        button(text("Back to the shelf").size(13).color(tokens.ink))
            .padding(Padding {
                top: 8.0,
                right: 14.0,
                bottom: 8.0,
                left: 14.0,
            })
            .style(move |_, status| buttons::ghost(tokens, 1.0, status))
            .on_press(Message::Close),
    ]
    .align_x(Alignment::Center)
    .spacing(14)
    .width(Length::Shrink)
    .into()
}
