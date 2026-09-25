//! The persisted typography of the reflowable formats — the schema half. The field
//! names here ARE the storage contract: additive only, every field defaulted, so a
//! blob saved before a field existed loads with that field's default.

mod choice;
mod fonts;
mod text;

pub use choice::FontChoice;
pub use fonts::{BuiltInFont, SystemFont, TextFamily, builtin_fonts};
pub use text::{
    DEFAULT_FONT_SIZE, DEFAULT_INK_CONTRAST, DEFAULT_LINE_HEIGHT, DEFAULT_PARAGRAPH_MARGIN,
    TextColumnAlign, TextSettings, sanitize,
};

#[cfg(test)]
mod tests;
