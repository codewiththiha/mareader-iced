//! Resolving the reader's typography into the faces to paint with.
//!
//! The settings themselves (`reader_core::settings::typography`) are the
//! persisted schema; this module turns one into an ordered font stack and,
//! for pagination, the body font's average glyph advance — which is why the
//! height estimate and the rendered text cannot drift apart on the font.
//!
//! The schema types are re-exported so a component that reads a knob and
//! paints it imports from one crate.

pub use reader_core::settings::typography::{
    TextFamily, TextSettings, builtin_fonts, FontChoice,
};

pub use reader_core::settings::typography::{
    BuiltInFont, DEFAULT_FONT_SIZE, DEFAULT_INK_CONTRAST, DEFAULT_LINE_HEIGHT,
    DEFAULT_PARAGRAPH_MARGIN, SystemFont, TextColumnAlign, sanitize,
};

/// The serif stack the body falls back to when no default font is chosen:
/// the classic book-reading faces, in availability order.
const SERIF_STACK: &str =
    "Charter, \"Bitstream Charter\", \"Iowan Old Style\", Georgia, \"Times New Roman\", serif";
const SANS_STACK: &str =
    "ui-sans, -apple-system, \"Segoe UI\", Helvetica, Arial, sans-serif";
const MONO_STACK: &str =
    "ui-mono, Menlo, Consolas, \"Liberation Mono\", \"Courier New\", monospace";

fn family_default_stack(family: TextFamily) -> &'static str {
    match family {
        TextFamily::Serif => SERIF_STACK,
        TextFamily::SansSerif => SANS_STACK,
        TextFamily::Monospace => MONO_STACK,
    }
}

/// Resolve one font choice to a CSS stack.
///
/// * `Default` in a FAMILY slot resolves to that family's natural stack.
/// * `Default` in the BODY slot (`family == None`) reads in the SERIF
///   face — whatever the Serif slot currently resolves to, not the bare
///   constant — so the Serif picker is what shapes default body text, and
///   the Default picker is the override that takes body away from it.
/// * A bundled font resolves to its own stack; a bundled id the build does
///   not (yet) ship falls back the same way as `Default`, so a saved choice
///   never renders nothing.
fn resolve_stack(settings: &TextSettings, choice: &FontChoice, family: Option<TextFamily>) -> String {
    match choice {
        FontChoice::Default => match family {
            Some(family) => family_default_stack(family).to_string(),
            None => family_stack(settings, TextFamily::Serif),
        },
        FontChoice::System(f) => f.stack(),
        FontChoice::BuiltIn(id) => builtin_fonts()
            .iter()
            .find(|f| f.id == id.as_str())
            .map(|f| f.stack.to_string())
            .unwrap_or_else(|| {
                family
                    .map(family_default_stack)
                    .unwrap_or(SERIF_STACK)
                    .to_string()
            }),
    }
}

/// The stack body text renders in: the Default picker's choice, or the
/// Serif slot when that choice is `Default` (see [`resolve_stack`]).
pub fn body_stack(settings: &TextSettings) -> String {
    resolve_stack(settings, &settings.default_font, None)
}

/// The stack a family renders in, honouring its override slot.
pub fn family_stack(settings: &TextSettings, family: TextFamily) -> String {
    let choice = match family {
        TextFamily::Serif => &settings.serif_font,
        TextFamily::SansSerif => &settings.sans_font,
        TextFamily::Monospace => &settings.mono_font,
    };
    resolve_stack(settings, choice, Some(family))
}

/// Average glyph advance (fraction of the font size) for the body font —
/// the pagination estimate's per-character width.
pub fn body_char_width(settings: &TextSettings) -> f64 {
    match &settings.default_font {
        FontChoice::System(f) => f.avg_char_width(),
        FontChoice::BuiltIn(id) => builtin_fonts()
            .iter()
            .find(|f| f.id == id.as_str())
            .map(|f| match f.family {
                TextFamily::Monospace => 0.6,
                TextFamily::Serif => 0.5,
                TextFamily::SansSerif => 0.52,
            })
            .unwrap_or(0.5),
        FontChoice::Default => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_resolve_through_every_slot() {
        let mut s = TextSettings::default();
        // Body default: the serif reading stack.
        assert_eq!(body_stack(&s), SERIF_STACK);
        // Body default FOLLOWS the Serif slot...
        s.serif_font = FontChoice::System(SystemFont::Georgia);
        assert_eq!(body_stack(&s), "Georgia, serif");
        // ...unless the body itself picks a face, which wins outright.
        s.default_font = FontChoice::System(SystemFont::Verdana);
        assert_eq!(body_stack(&s), "Verdana, sans-serif");
        s.default_font = FontChoice::Default;
        s.serif_font = FontChoice::Default;
        // Family slots: Default keeps the natural stack...
        assert_eq!(family_stack(&s, TextFamily::SansSerif), SANS_STACK);
        assert_eq!(family_stack(&s, TextFamily::Monospace), MONO_STACK);
        // ...an override replaces it.
        s.mono_font = FontChoice::System(SystemFont::Consolas);
        assert_eq!(family_stack(&s, TextFamily::Monospace), "Consolas, monospace");
        // An unshipped bundled font falls back to the natural stack.
        s.serif_font = FontChoice::BuiltIn("not-shipped-yet".into());
        assert_eq!(family_stack(&s, TextFamily::Serif), SERIF_STACK);
    }
}
