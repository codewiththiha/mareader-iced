//! The button vocabulary: the chrome's own button faces, shared by every
//! surface that draws one.
//!
//! The ghost face — nothing at rest, a wash under the pointer — was written
//! in the titlebar and reached for from the shelf's bar and the reader's
//! controls; it lives here so the app-wide look has an app-wide home.

use iced::widget::button;
use iced::{Background, Border, Color, Shadow};

use crate::theme::{wash, Tokens};

/// A ghost button: nothing at rest, a surface wash under the pointer, and the
/// line under the finger. `factor` is the reveal's own — a fading bar passes
/// its opacity, anything at rest passes `1.0`.
pub fn ghost(tokens: Tokens, factor: f32, status: button::Status) -> button::Style {
    let backdrop = match status {
        button::Status::Hovered => Some(wash(tokens.surface, factor)),
        button::Status::Pressed => Some(wash(tokens.line, factor)),
        _ => None,
    };
    let ink = match status {
        button::Status::Disabled => tokens.muted,
        _ => tokens.ink,
    };
    button::Style {
        background: backdrop.map(Background::Color),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
        text_color: wash(ink, factor),
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ghost_button_is_bare_until_it_is_touched() {
        let tokens = Tokens::light();
        let rest = ghost(tokens, 1.0, button::Status::Active);
        assert!(rest.background.is_none(), "nothing at rest");
        assert_eq!(rest.text_color, tokens.ink);

        let hovered = ghost(tokens, 1.0, button::Status::Hovered);
        assert_eq!(
            hovered.background,
            Some(Background::Color(tokens.surface)),
            "a surface wash under the pointer"
        );
        let pressed = ghost(tokens, 1.0, button::Status::Pressed);
        assert_eq!(pressed.background, Some(Background::Color(tokens.line)));
    }

    #[test]
    fn a_disabled_ghost_is_muted_and_the_reveal_fades_every_colour() {
        let tokens = Tokens::light();
        let disabled = ghost(tokens, 1.0, button::Status::Disabled);
        assert_eq!(disabled.text_color, tokens.muted);

        let half = ghost(tokens, 0.5, button::Status::Hovered);
        assert!((half.text_color.a - tokens.ink.a * 0.5).abs() < 1e-6);
        let backdrop = half.background.expect("hovered");
        if let Background::Color(color) = backdrop {
            assert!((color.a - tokens.surface.a * 0.5).abs() < 1e-6, "the bed fades with the bar");
        } else {
            panic!("the ghost's bed is a plain colour");
        }
    }
}
