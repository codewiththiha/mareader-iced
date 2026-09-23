//! The shelf's titlebar slots: the breadcrumb on the left, the search pill
//! in the centre.
//!
//! The breadcrumb is the library page's only prose, and it is navigation:
//! one crumb per level, `Home` first and the open shelf last, the `Next`
//! glyph between them. The web bar folded long chains behind an ellipsis
//! and armed every crumb as a drop target; the fold and the drag arrive
//! with the governance flows — here every crumb is shown and every crumb
//! but the last is a way back.
//!
//! The search pill narrows the shelf where it stands: the placeholder names
//! the library's count, each keystroke filters the level, and a filled pill
//! wears its own clear button.

use iced::widget::{button, container, row, text, text_input, Row};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding};

use library_core::blob::LibraryBlob;
use library_core::shelf::{find, ancestors, ALL_SHELF};
use library_core::text as lib_text;

use crate::app::Message;
use crate::chrome::icons::{icon, IconName};
use crate::chrome::titlebar::ghost_button_style;
use crate::theme::{fade, wash, Tokens};

/// The crumb's ink at rest, weighted for the level it names.
const MEDIUM: Font = Font { weight: iced::font::Weight::Medium, ..Font::DEFAULT };

/// One crumb's character budget — long shelf names cut here rather than
/// pushing the clusters apart.
fn crumb_label(name: &str) -> String {
    let count = name.chars().count();
    if count <= 24 {
        return name.to_owned();
    }
    let cut: String = name.chars().take(23).collect();
    format!("{cut}…")
}

fn crumb_button<'a>(
    tokens: Tokens,
    label: &'a str,
    factor: f32,
    current: bool,
    message: Message,
) -> Element<'a, Message> {
    let color = fade(if current { tokens.ink } else { tokens.muted }, factor);
    let mut face = text(crumb_label(label)).size(13).color(color);
    if current {
        face = face.font(MEDIUM);
    }
    button(container(face).max_width(160.0))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(move |_, status| ghost_button_style(tokens, factor, status))
        .on_press(message)
        .into()
}

/// The breadcrumb chain for the level the shelf is standing on.
pub fn breadcrumb<'a>(
    tokens: Tokens,
    library: &'a LibraryBlob,
    shelf: &str,
    factor: f32,
) -> Element<'a, Message> {
    // A fresh element per gap: `Element` is not `Clone`, and a closure is
    // the honest spelling of "the same separator, again".
    let separator = |tokens: Tokens, factor: f32| -> Element<'a, Message> {
        container(icon(IconName::Next, 13, fade(tokens.muted, factor)))
            .padding(Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 2.0 })
            .into()
    };

    let mut items: Vec<Element<'a, Message>> = vec![crumb_button(
        tokens,
        "Home",
        factor,
        shelf == ALL_SHELF,
        Message::Navigate(ALL_SHELF.to_string()),
    )];

    if shelf != ALL_SHELF {
        for level in ancestors(&library.shelves, shelf) {
            items.push(separator(tokens, factor));
            items.push(crumb_button(
                tokens,
                &level.name,
                factor,
                false,
                Message::Navigate(level.id.clone()),
            ));
        }
        if let Some(current) = find(&library.shelves, shelf) {
            items.push(separator(tokens, factor));
            // The last crumb is the shelf the reader stands on; its menu —
            // rename, duplicate, take apart — lands with governance.
            items.push(
                container(
                    text(crumb_label(&current.name))
                        .size(13)
                        .font(MEDIUM)
                        .color(fade(tokens.ink, factor)),
                )
                .max_width(160.0)
                .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
                .into(),
            );
        }
    }

    Row::with_children(items).spacing(2).align_y(Alignment::Center).into()
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
            placeholder: fade(tokens.muted, factor),
            value: fade(tokens.ink, factor),
            selection: tokens.accent_soft,
        });

    let mut face = row![icon(IconName::Search, 15, fade(tokens.muted, factor)), input]
        .spacing(8)
        .align_y(Alignment::Center);
    if !query.is_empty() {
        face = face.push(
            button(icon(IconName::Close, 12, fade(tokens.muted, factor)))
                .padding(4.0)
                .style(move |_, status| ghost_button_style(tokens, factor, status))
                .on_press(Message::Query(String::new())),
        );
    }

    container(face)
        .width(Length::Fill)
        .max_width(576.0)
        .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(fade(wash(tokens.surface, 0.70), factor))),
            border: Border { color: fade(tokens.line, factor), width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}
