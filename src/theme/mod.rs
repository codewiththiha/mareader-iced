//! The design tokens, carried over from the web app's `styles/tokens.css`.
//!
//! Seven colour tokens per base mode — paper, ink, muted, surface, line,
//! accent, accent-soft — plus the amber of the current search match. The
//! three bases (light, dark, dim) are the STRUCTURE; the appearance system
//! that arrives in a later phase layers the continuous tint on top of them
//! exactly as `reader_core::appearance` defines. Until then the window
//! follows the system's light/dark answer.

use iced::theme::{Palette, Style};
use iced::{Color, Theme};

/// One base mode's worth of tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// The page and window background.
    pub paper: Color,
    /// Primary text and icon strokes.
    pub ink: Color,
    /// Secondary text, disabled strokes.
    pub muted: Color,
    /// Raised chrome: bars, cards, hover washes.
    pub surface: Color,
    /// Hairlines and borders.
    pub line: Color,
    /// The one accent: selection, active states, links.
    pub accent: Color,
    /// The accent's soft bed: pills, badges, suggestion highlights.
    pub accent_soft: Color,
    /// The current search match — deliberately NOT derived from the accent,
    /// because every other match already wears the accent and the current
    /// one must stay separable at a glance.
    pub match_current: Color,
}

fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        f32::from(((rgb >> 16) & 0xFF) as u8) / 255.0,
        f32::from(((rgb >> 8) & 0xFF) as u8) / 255.0,
        f32::from((rgb & 0xFF) as u8) / 255.0,
    )
}

impl Tokens {
    /// `:root` — the light base.
    pub fn light() -> Self {
        Self {
            paper: hex(0xff_ff_ff),
            ink: hex(0x1f_29_37),
            muted: hex(0x6b_72_80),
            surface: hex(0xf3_f4_f6),
            line: hex(0xe5_e7_eb),
            accent: hex(0x25_63_eb),
            accent_soft: hex(0xdb_ea_fe),
            match_current: hex(0xf5_9e_0b),
        }
    }

    /// `:root[data-base="dark"]`.
    pub fn dark() -> Self {
        Self {
            paper: hex(0x131316),
            ink: hex(0xe5_e7_eb),
            muted: hex(0x9c_a3_af),
            surface: hex(0x1a_1a_1e),
            line: hex(0x2b_2b_31),
            accent: hex(0x60_a5_fa),
            accent_soft: hex(0x1d_2b_3a),
            // Lifted and warmed for the dark canvases, where a screen wash
            // pulls a mid amber toward the paper colour.
            match_current: hex(0xfb_bf_24),
        }
    }

    /// `:root[data-base="dim"]` — NOT an inverted theme: it dims the page
    /// and keeps the document's own colours.
    pub fn dim() -> Self {
        Self {
            paper: hex(0x1a_1c_1f),
            ink: hex(0xc3_c6_cb),
            muted: hex(0x8b_8f_96),
            surface: hex(0x20_23_28),
            line: hex(0x2e_32_38),
            accent: hex(0x7a_9b_d4),
            accent_soft: hex(0x23_2b_36),
            match_current: hex(0xf5_9e_0b),
        }
    }

    /// The tokens for a system light/dark answer, until the appearance
    /// system takes the choice over.
    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }
}

/// The iced theme built from a token set. Widget-level styling reads the
/// tokens directly (the chrome styles take `Tokens`); the palette carries
/// them into the stock widgets' defaults.
pub fn build(tokens: Tokens, dark: bool) -> Theme {
    Theme::custom(
        if dark { "Mareader Dark" } else { "Mareader Light" },
        Palette {
            background: tokens.paper,
            text: tokens.ink,
            primary: tokens.accent,
            success: hex(0x16_a3_4a),
            warning: tokens.match_current,
            danger: hex(0xdc_26_26),
        },
    )
}

/// The application backdrop: the paper the whole window is written on.
pub fn application_style(tokens: Tokens) -> Style {
    Style { background_color: tokens.paper, text_color: tokens.ink }
}

/// Fade a colour toward transparent — the titlebar's reveal paints every
/// one of its colours through it.
pub fn fade(color: Color, factor: f32) -> Color {
    Color { a: color.a * factor.clamp(0.0, 1.0), ..color }
}
