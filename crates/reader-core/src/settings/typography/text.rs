//! The text setting itself: its defaults, its column alignment, and the clamp a
//! loaded blob passes through.

use serde::{Deserialize, Serialize};

use super::choice::FontChoice;

/// Where the reading column sits inside the viewport while a reflowable
/// document streams continuously. The text keeps its natural alignment — this
/// positions the COLUMN, the way a narrower book page sits left, centre or
/// right on a desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextColumnAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextColumnAlign {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }

    /// The stylesheet class that positions the stream's reading column
    /// (defined in `styles/text.css` beside the stream itself).
    pub fn container_class(&self) -> &'static str {
        match self {
            Self::Left => "tx-align-left",
            Self::Center => "tx-align-center",
            Self::Right => "tx-align-right",
        }
    }
}

/// The persisted typography of the reflowable formats. Every knob is
/// independent and additive; a blob missing any of them loads the defaults.
/// The ranges are enforced by [`sanitize`], which the app runs on load AND on
/// every write path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSettings {
    /// Render pages as an open book: a gutter margin faces the spine, and
    /// in the two-up modes the pair reads as facing pages. Off, every page
    /// carries symmetric margins.
    pub book_layout: bool,
    /// Space under a paragraph, in ems of the body font size.
    pub paragraph_margin: f64,
    /// Line height, as a unitless multiple of the font size.
    pub line_height: f64,
    /// Extra space between words, in CSS px (at scale 1). May be negative,
    /// to tighten.
    pub word_spacing: f64,
    /// Extra space between letters, in ems. May be negative, to tighten.
    pub letter_spacing: f64,
    /// First-line indent of paragraphs, in ems.
    pub text_indent: f64,
    /// Stretch each line to both margins.
    pub justify: bool,
    /// Let the shaper break words at line ends (needs a language-aware
    /// hyphenator; the reader marks its text as English).
    pub hyphenation: bool,
    /// Body font size in CSS px at scale 1.
    pub font_size: f64,
    /// Body font weight, 100..=900.
    pub font_weight: u16,
    /// The font body text renders in. `Default` resolves to the serif
    /// reading stack.
    pub default_font: FontChoice,
    /// Override for the serif family. `Default` keeps the family's natural
    /// stack.
    pub serif_font: FontChoice,
    /// Override for the sans family. `Default` keeps the family's natural
    /// stack.
    pub sans_font: FontChoice,
    /// Override for the monospace family (code). `Default` keeps the
    /// family's natural stack.
    pub mono_font: FontChoice,
    /// Where the reading column sits in the viewport while a text document
    /// streams continuously. The paginated modes ignore it — their pages
    /// centre themselves the way every fixed sheet does.
    pub column_align: TextColumnAlign,
    /// Body-ink intensity, 0–100. 100 is the theme's full ink; below that the
    /// ink mixes toward the paper colour. A comfort dial for long reading, not
    /// a tint — the paper stays whatever the theme says.
    pub ink_contrast: f64,
}

/// Clamp every field into its supported range. Runs on load and on every
impl Default for TextSettings {
    fn default() -> Self {
        Self {
            book_layout: false,
            paragraph_margin: DEFAULT_PARAGRAPH_MARGIN,
            line_height: DEFAULT_LINE_HEIGHT,
            word_spacing: 0.0,
            letter_spacing: 0.0,
            text_indent: 0.0,
            justify: false,
            hyphenation: false,
            font_size: DEFAULT_FONT_SIZE,
            font_weight: 400,
            default_font: FontChoice::Default,
            serif_font: FontChoice::Default,
            sans_font: FontChoice::Default,
            mono_font: FontChoice::Default,
            column_align: TextColumnAlign::default(),
            ink_contrast: DEFAULT_INK_CONTRAST,
        }
    }
}

/// write; idempotent.
pub fn sanitize(s: &mut TextSettings) {
    s.paragraph_margin = s.paragraph_margin.clamp(0.0, 3.0);
    s.line_height = s.line_height.clamp(1.0, 3.0);
    s.word_spacing = s.word_spacing.clamp(-2.0, 10.0);
    s.letter_spacing = s.letter_spacing.clamp(-0.02, 0.3);
    s.text_indent = s.text_indent.clamp(0.0, 4.0);
    s.font_size = s.font_size.clamp(10.0, 32.0);
    s.font_weight = s.font_weight.clamp(100, 900);
    s.ink_contrast = s.ink_contrast.clamp(0.0, 100.0);
}

/// Default body size in CSS px at scale 1.
pub const DEFAULT_FONT_SIZE: f64 = 17.0;

/// The reader's idea of a neutral paragraph: 1em of space under it.
pub const DEFAULT_PARAGRAPH_MARGIN: f64 = 1.0;

/// Default line height (unitless multiple of the font size).
pub const DEFAULT_LINE_HEIGHT: f64 = 1.7;

/// Default body-ink intensity: the theme's full ink.
pub const DEFAULT_INK_CONTRAST: f64 = 100.0;
