//! The app's modal surfaces: a panel in the middle of the window over a
//! scrim that cancels on press.
//!
//! The web app slid its sheets up from below; natively the panel sits
//! centred, because there is no page to slide over. What stays is the
//! contract: one question at a time, an escape hatch on the scrim and the
//! Escape key, and the affirmative button on the right.

use std::borrow::Cow;

use iced::widget::{button, container, mouse_area, stack, text, Column, Row, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Shadow};

use crate::theme::{wash, Elevation, Tokens};

/// The panel's width — wide enough for a question and a name, narrow enough
/// to read as a card over the shelf.
pub const SHEET_W: f32 = 380.0;

/// The import sheet's width: it carries whole sections of options, so it
/// needs the room the question sheets do not.
pub const IMPORT_W: f32 = 460.0;

/// The question sheet's width: its answer rows carry a sentence of note
/// each, so it is a little wider than the yes-or-no panel.
pub const CONFLICT_W: f32 = 420.0;

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
    title: impl Into<Cow<'a, str>>,
    body: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    panel_sized(tokens, SHEET_W, title, body, actions)
}

/// The panel at an explicit width, for sheets wider than a question. The
/// title is a `Cow`, because the question sheets build their heading off the
/// library at render time and the others hand it a literal.
pub fn panel_sized<'a, M: Clone + 'a>(
    tokens: Tokens,
    width: f32,
    title: impl Into<Cow<'a, str>>,
    body: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let title: Cow<'a, str> = title.into();
    let title = text(title).size(15).font(MEDIUM).color(tokens.ink);
    panel_of(tokens, width, title.into(), body, actions)
}

fn panel_of<'a, M: Clone + 'a>(
    tokens: Tokens,
    width: f32,
    title: Element<'a, M>,
    body: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    container(
        Column::new()
            .push(title)
            .push(body)
            .push(
                container(Row::with_children(actions).spacing(8).align_y(Alignment::Center))
                    .width(Length::Fill)
                    .align_x(Alignment::End),
            )
            .spacing(14)
            .width(Length::Fill),
    )
    .width(width)
    .padding(Padding { top: 20.0, right: 20.0, bottom: 20.0, left: 20.0 })
    .style(move |_| container::Style {
        background: Some(Background::Color(tokens.surface)),
        border: Border { color: tokens.line, width: 1.0, radius: 14.0.into() },
        shadow: Elevation::Sheet.shadow(),
        ..container::Style::default()
    })
    .into()
}

/// One answer on a question sheet: its name, and the line under it that
/// says what choosing it does. Not the small action buttons — a merge, a
/// replace and an "as new" are consequences the reader has to be able to
/// read before the click, so the note wraps and the row is the width of the
/// sheet.
pub fn choice_row<'a, M: Clone + 'a>(
    tokens: Tokens,
    label: &'a str,
    note: String,
    message: M,
) -> Element<'a, M> {
    button(
        Column::new()
            .push(text(label).size(13).color(tokens.ink))
            .push(text(note).size(11).color(tokens.muted))
            .spacing(2)
            .align_x(Alignment::Start)
            .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(Padding { top: 10.0, right: 14.0, bottom: 10.0, left: 14.0 })
    .style(move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => wash(tokens.ink, 0.06),
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(background)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
            text_color: tokens.ink,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(message)
    .into()
}

/// The answer rows in one bordered group with hairlines between them: the
/// web sheet's divided list, natively — iced's border is one edge-set per
/// container, so a divider is a hairline of its own rather than a row's
/// bottom edge.
pub fn choice_group<'a, M: Clone + 'a>(
    tokens: Tokens,
    choices: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let mut column = Column::new();
    for (i, choice) in choices.into_iter().enumerate() {
        if i > 0 {
            column = column.push(
                container(Space::new().width(Length::Fill).height(1))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(tokens.line)),
                        ..container::Style::default()
                    }),
            );
        }
        column = column.push(choice);
    }
    container(column.width(Length::Fill))
        .width(Length::Fill)
        .clip(true)
        .style(move |_| container::Style {
            border: Border { color: tokens.line, width: 1.0, radius: 12.0.into() },
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
