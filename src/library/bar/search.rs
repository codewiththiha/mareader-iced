//! The search pill that narrows the shelf where it stands.

use crate::app::Message;
use crate::chrome::icons::{IconName, icon};
use crate::theme::{Tokens, wash};
use crate::ui::buttons;
use iced::widget::{Id, button, container, row, text_input};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};
use library_core::{text as lib_text};

/// The rename field's identity, so the app can hand it focus the moment it
/// appears.
pub const RENAME_INPUT: Id = Id::new("shelf-rename");

/// The crumb, mid-rename: the field the new name is typed into. Enter
/// commits; Escape backs out.
pub(super) fn rename_field(tokens: Tokens, draft: &str, factor: f32) -> Element<'static, Message> {
    let input = text_input("Shelf name", draft)
        .id(RENAME_INPUT)
        .on_input(Message::RenameDraft)
        .on_submit(Message::CommitRename)
        .size(13)
        .width(176.0)
        .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(wash(wash(tokens.paper, 0.9), factor)),
            border: Border { color: wash(tokens.accent, factor), width: 1.0, radius: 6.0.into() },
            icon: tokens.muted,
            placeholder: wash(tokens.muted, factor),
            value: wash(tokens.ink, factor),
            selection: tokens.accent_soft,
        });
    input.into()
}

/// The search pill: an always-present filter over the shelf.
pub fn search<'a>(
    tokens: Tokens,
    book_count: usize,
    query: &'a str,
    factor: f32,
) -> Element<'a, Message> {
    let placeholder = if book_count == 0 {
        "Search the library".to_string()
    } else {
        format!("Search {}", lib_text::plural(book_count, "book", "books"))
    };

    let input = text_input(&placeholder, query)
        .on_input(Message::Query)
        .size(13)
        .width(Length::Fill)
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
            icon: Color::TRANSPARENT,
            placeholder: wash(tokens.muted, factor),
            value: wash(tokens.ink, factor),
            selection: tokens.accent_soft,
        });

    let mut face = row![icon(IconName::Search, 15, wash(tokens.muted, factor)), input]
        .spacing(8)
        .align_y(Alignment::Center);
    if !query.is_empty() {
        face = face.push(
            button(icon(IconName::Close, 12, wash(tokens.muted, factor)))
                .padding(4.0)
                .style(move |_, status| buttons::ghost(tokens, factor, status))
                .on_press(Message::Query(String::new())),
        );
    }

    container(face)
        .width(Length::Fill)
        .max_width(576.0)
        .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(wash(tokens.surface, 0.70), factor))),
            border: Border { color: wash(tokens.line, factor), width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}
