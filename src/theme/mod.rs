//! The design tokens, carried over from the web app's `styles/tokens.css`.
//!
//! Seven colour tokens per base mode — paper, ink, muted, surface, line,
//! accent, accent-soft — plus the amber of the current search match. The
//! three bases (light, dark, dim) are the STRUCTURE; the appearance system
//! that arrives in a later phase layers the continuous tint on top of them
//! exactly as `reader_core::appearance` defines. Until then the window
//! follows the system's light/dark answer.

use iced::theme::{Palette, Style};
use iced::{Color, Shadow, Theme, Vector};
use reader_core::appearance::BaseMode;

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

/// The palette's danger red, lifted out so the menus and the sheets can
/// mark the rows that take something away. (The channels are 0xdc2626,
/// spelled out because `f32::from` is not const.)
pub const DANGER: Color = Color::from_rgb(0.8627451, 0.14901961, 0.14901961);

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

    /// The tokens of a base mode — the appearance system's own choice,
    /// read straight off the persisted settings.
    pub fn for_base(base: BaseMode) -> Self {
        match base {
            BaseMode::Light => Self::light(),
            BaseMode::Dark => Self::dark(),
            BaseMode::Dim => Self::dim(),
        }
    }
}

/// The iced theme built from a token set. Widget-level styling reads the
/// tokens directly (the chrome styles take `Tokens`); the palette carries
/// them into the stock widgets' defaults.
pub fn build(tokens: Tokens, base: BaseMode) -> Theme {
    Theme::custom(
        match base {
            BaseMode::Light => "Mareader Light",
            BaseMode::Dark => "Mareader Dark",
            BaseMode::Dim => "Mareader Dim",
        },
        Palette {
            background: tokens.paper,
            text: tokens.ink,
            primary: tokens.accent,
            success: hex(0x16_a3_4a),
            warning: tokens.match_current,
            danger: DANGER,
        },
    )
}

/// The application backdrop: the paper the whole window is written on.
pub fn application_style(tokens: Tokens) -> Style {
    Style { background_color: tokens.paper, text_color: tokens.ink }
}

/// Blend two colours, `t` of the way from `from` to `to` — iced has no
/// `Color::mix`, and the shelf's gradients, seams and washes are all token
/// colours blended by hand (the CSS `color-mix` the web app leaned on).
pub fn mix(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    Color {
        r: lerp(from.r, to.r),
        g: lerp(from.g, to.g),
        b: lerp(from.b, to.b),
        a: lerp(from.a, to.a),
    }
}

/// The app's shadow ladder, lowest rung first: every raised surface in one
/// place, so the ordering between a pill and a popover is a name rather than
/// five numbers retyped at the call site. The numbers are the ones the port
/// carried over from the reference's CSS; each rung keeps its own, because
/// they were tuned separately there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Elevation {
    /// A list row's thumbnail: the quietest bed the app draws.
    Thumb,
    /// The count badge on a cell.
    Badge,
    /// The reader's bottom bar, a step off the page under it.
    Bar,
    /// A row of controls in a pill: the selection bar, the runs dock, a
    /// shelf card at rest.
    Pill,
    /// A shelf card under the pointer.
    Hover,
    /// The reader's sheet of paper — the deepest drop in the app.
    Page,
    /// The ring a selected cell wears, in the colour that marks it.
    Ring(Color),
    /// What floats over the app: a menu, a fold's plate.
    Float,
    /// The ghost fan's plate: the float's drop, a touch darker.
    Plate,
    /// The toaster, over whatever is on screen.
    Toast,
    /// The modal panel: the app's highest surface.
    Sheet,
    /// The titlebar over the shelf, scaled with the bar's own factor so a
    /// pinned, revealed bar keeps its weight.
    Chrome(f32),
}

impl Elevation {
    /// The shadow this rung casts.
    pub fn shadow(self) -> Shadow {
        let (y, blur, color) = match self {
            Self::Thumb => (1.0, 2.0, wash(Color::BLACK, 0.16)),
            Self::Badge => (2.0, 6.0, wash(Color::BLACK, 0.35)),
            Self::Bar => (2.0, 12.0, wash(Color::BLACK, 0.18)),
            Self::Pill => (4.0, 12.0, wash(Color::BLACK, 0.18)),
            Self::Hover => (10.0, 24.0, wash(Color::BLACK, 0.26)),
            Self::Page => (3.0, 16.0, wash(Color::BLACK, 0.22)),
            Self::Ring(mark) => (0.0, 10.0, wash(mark, 0.20)),
            Self::Float => (8.0, 24.0, wash(Color::BLACK, 0.30)),
            Self::Plate => (8.0, 24.0, wash(Color::BLACK, 0.35)),
            Self::Toast => (10.0, 24.0, wash(Color::BLACK, 0.35)),
            Self::Sheet => (12.0, 36.0, wash(Color::BLACK, 0.35)),
            Self::Chrome(factor) => {
                return Shadow {
                    color: Color { a: 0.10 * factor, ..Color::BLACK },
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 12.0 * factor,
                };
            }
        };
        Shadow { color, offset: Vector::new(0.0, y), blur_radius: blur }
    }
}

/// A colour at a fraction of its own alpha — the CSS slash notation
/// (`line/60`, `surface/40`) the web app painted with, and the reveal's own
/// fade. One helper, because the two operations were the same one.
pub fn wash(color: Color, alpha: f32) -> Color {
    Color { a: color.a * alpha.clamp(0.0, 1.0), ..color }
}
