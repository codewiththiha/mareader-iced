//! The font catalogue: the three families, the system faces that answer them, and
//! the empty slot for faces shipped inside the app.



/// to exactly one of them, and each family has its own override slot in the
/// settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFamily {
    Serif,
    SansSerif,
    Monospace,
}

impl TextFamily {
    /// The generic CSS tail every stack of this family ends in.
    pub fn generic(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::SansSerif => "sans-serif",
            Self::Monospace => "monospace",
        }
    }
}

/// A font face that ships INSIDE the application, rather than one the OS
/// provides. The table is empty today — this type is the seam a future
/// release bolts bundled fonts onto: add a row, and every font picker and
/// every saved `builtin:<name>` choice resolves it. Nothing else moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInFont {
    /// Stable identity — the `builtin:<name>` persisted in settings.
    pub id: &'static str,
    /// What the pickers show.
    pub label: &'static str,
    /// The CSS stack the face renders with (the bundled @font-face first,
    /// then fallbacks).
    pub stack: &'static str,
    /// Which family the face belongs to (picker grouping + fallbacks).
    pub family: TextFamily,
}

/// Every bundled font the reader knows. Empty until fonts ship with the
/// app; see [`BuiltInFont`].
pub fn builtin_fonts() -> &'static [BuiltInFont] {
    &[]
}

/// A system font face: one of the widely available cross-platform faces,
/// plus the four generic stacks. The id is what settings persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFont {
    UiSerif,
    UiSans,
    UiMono,
    Georgia,
    TimesNewRoman,
    Palatino,
    Garamond,
    Baskerville,
    Charter,
    Arial,
    Helvetica,
    Verdana,
    TrebuchetMs,
    Tahoma,
    GillSans,
    CourierNew,
    Menlo,
    Consolas,
    Monaco,
}

impl SystemFont {
    /// Every system face the pickers offer, in display order.
    pub fn all() -> &'static [SystemFont] {
        &[
            Self::UiSerif,
            Self::UiSans,
            Self::UiMono,
            Self::Georgia,
            Self::TimesNewRoman,
            Self::Palatino,
            Self::Garamond,
            Self::Baskerville,
            Self::Charter,
            Self::Arial,
            Self::Helvetica,
            Self::Verdana,
            Self::TrebuchetMs,
            Self::Tahoma,
            Self::GillSans,
            Self::CourierNew,
            Self::Menlo,
            Self::Consolas,
            Self::Monaco,
        ]
    }

    /// Stable identity — the `system:<id>` persisted in settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::UiSerif => "ui-serif",
            Self::UiSans => "ui-sans",
            Self::UiMono => "ui-mono",
            Self::Georgia => "georgia",
            Self::TimesNewRoman => "times-new-roman",
            Self::Palatino => "palatino",
            Self::Garamond => "garamond",
            Self::Baskerville => "baskerville",
            Self::Charter => "charter",
            Self::Arial => "arial",
            Self::Helvetica => "helvetica",
            Self::Verdana => "verdana",
            Self::TrebuchetMs => "trebuchet-ms",
            Self::Tahoma => "tahoma",
            Self::GillSans => "gill-sans",
            Self::CourierNew => "courier-new",
            Self::Menlo => "menlo",
            Self::Consolas => "consolas",
            Self::Monaco => "monaco",
        }
    }

    /// Find a face by its persisted id.
    pub(super) fn from_id(id: &str) -> Option<SystemFont> {
        Self::all().iter().copied().find(|f| f.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::UiSerif => "System Serif",
            Self::UiSans => "System Sans",
            Self::UiMono => "System Mono",
            Self::Georgia => "Georgia",
            Self::TimesNewRoman => "Times New Roman",
            Self::Palatino => "Palatino",
            Self::Garamond => "Garamond",
            Self::Baskerville => "Baskerville",
            Self::Charter => "Charter",
            Self::Arial => "Arial",
            Self::Helvetica => "Helvetica",
            Self::Verdana => "Verdana",
            Self::TrebuchetMs => "Trebuchet MS",
            Self::Tahoma => "Tahoma",
            Self::GillSans => "Gill Sans",
            Self::CourierNew => "Courier New",
            Self::Menlo => "Menlo",
            Self::Consolas => "Consolas",
            Self::Monaco => "Monaco",
        }
    }

    pub fn family(self) -> TextFamily {
        match self {
            Self::UiSerif | Self::Georgia | Self::TimesNewRoman | Self::Palatino
            | Self::Garamond | Self::Baskerville | Self::Charter => TextFamily::Serif,
            Self::UiMono | Self::CourierNew | Self::Menlo | Self::Consolas | Self::Monaco => {
                TextFamily::Monospace
            }
            _ => TextFamily::SansSerif,
        }
    }

    /// The CSS stack for this face: the face itself (quoted where a name
    /// carries spaces) followed by its family's generic tail.
    pub fn stack(self) -> String {
        let head = match self {
            Self::UiSerif => "ui-serif".to_string(),
            Self::UiSans => "ui-sans".to_string(),
            Self::UiMono => "ui-mono".to_string(),
            Self::Georgia => "Georgia".to_string(),
            Self::TimesNewRoman => "\"Times New Roman\"".to_string(),
            Self::Palatino => "Palatino, \"Palatino Linotype\"".to_string(),
            Self::Garamond => "Garamond, \"EB Garamond\"".to_string(),
            Self::Baskerville => "Baskerville, \"Baskerville Old Face\"".to_string(),
            Self::Charter => "Charter, \"Bitstream Charter\"".to_string(),
            Self::Arial => "Arial".to_string(),
            Self::Helvetica => "Helvetica, \"Helvetica Neue\"".to_string(),
            Self::Verdana => "Verdana".to_string(),
            Self::TrebuchetMs => "\"Trebuchet MS\"".to_string(),
            Self::Tahoma => "Tahoma".to_string(),
            Self::GillSans => "\"Gill Sans\", \"Gill Sans MT\"".to_string(),
            Self::CourierNew => "\"Courier New\"".to_string(),
            Self::Menlo => "Menlo".to_string(),
            Self::Consolas => "Consolas".to_string(),
            Self::Monaco => "Monaco".to_string(),
        };
        format!("{}, {}", head, self.family().generic())
    }

    /// Average glyph advance as a fraction of the font size. Feeds the
    /// pagination ESTIMATE (before the DOM measures real heights): a
    /// proportional face packs ~2 glyphs per em, a monospace face exactly
    /// 0.6em per cell.
    pub fn avg_char_width(self) -> f64 {
        match self.family() {
            TextFamily::Monospace => 0.6,
            TextFamily::Serif => 0.5,
            TextFamily::SansSerif => 0.52,
        }
    }
}
