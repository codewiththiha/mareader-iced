//! The folder badge: the chip saying where a folder shelf's books live, and
//! the dot for a tree that tracks its rungs.

use iced::widget::{container, text, Row, Space};
use iced::{Alignment, Background, Border, Element, Padding};
use crate::app::Message;
use crate::library::facts::{Badge, FolderFacts};
use crate::theme::{mix, wash, Tokens};


/// The plate's corner badges: where the books live, and — when the tree
/// tracks this rung — the dot that says so. `None` when a shelf wears
/// neither, and then the corner stays clean.
pub(super) fn badge_row(tokens: Tokens, facts: &FolderFacts) -> Option<Element<'static, Message>> {
    if facts.badge.is_none() && !facts.watched {
        return None;
    }
    let mut badges = Row::new().spacing(4).align_y(Alignment::Center);
    if let Some(badge) = facts.badge {
        badges = badges.push(badge_chip(tokens, badge));
    }
    if facts.watched {
        badges = badges.push(watch_dot(tokens));
    }
    Some(badges.into())
}

/// The chip saying where a folder's books live — "Copied", "On disk",
/// "Mixed". The quietest thing on the plate on purpose: it is true of
/// nearly every card. Shared by the card's corner and the list's row, so
/// one shelf cannot describe itself two ways.
pub fn badge_chip(tokens: Tokens, badge: Badge) -> Element<'static, Message> {
    let (border, background, color) = if badge.mixed {
        // Mixed content is slightly louder than the single-kind badges, so
        // the reader notices the shelf holds two kinds.
        (
            mix(tokens.line, tokens.accent, 0.30),
            mix(tokens.paper, tokens.accent, 0.12),
            mix(tokens.ink, tokens.muted, 0.20),
        )
    } else {
        (tokens.line, wash(tokens.paper, 0.84), tokens.muted)
    };
    container(text(badge.words).size(10).color(color))
        .padding(Padding { top: 1.0, right: 6.0, bottom: 1.0, left: 6.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            border: Border { color: border, width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}

/// A watched folder's dot: the accent under a hairline of the line. The web
/// dot breathed on a 2.4s loop; a breathing decoration would keep the whole
/// window redrawing for chrome nobody is looking at, so the native dot
/// holds still and says the same thing.
fn watch_dot(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(8.0).height(8.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}
