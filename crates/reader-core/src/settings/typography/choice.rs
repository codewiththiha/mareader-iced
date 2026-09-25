//! The reader's font choice, and the storage spelling each variant round-trips
//! through.

use serde::{Deserialize, Serialize};

use super::fonts::SystemFont;

/// One font choice, as persisted and as the pickers express it. The string
/// encoding is the storage contract: `default` resolves through the
/// surrounding context (the reading font for the body slot, the family's own
/// stack for a family slot); `system:<id>` a [`SystemFont`]; `builtin:<id>` a
/// [`BuiltInFont`] (future: fonts shipped in the app).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FontChoice {
    /// Follow the context: the reader's default font in the body slot, or
    /// the family's natural stack in a family slot.
    #[default]
    Default,
    System(SystemFont),
    BuiltIn(String),
}

impl FontChoice {
    /// Parse the persisted form. Unknown ids fall back to [`FontChoice::Default`]
    /// rather than failing the whole settings blob.
    pub(super) fn from_storage(s: &str) -> FontChoice {
        match s {
            "default" | "" => FontChoice::Default,
            rest => {
                if let Some(id) = rest.strip_prefix("system:") {
                    SystemFont::from_id(id)
                        .map(FontChoice::System)
                        .unwrap_or_default()
                } else if let Some(id) = rest.strip_prefix("builtin:") {
                    if id.is_empty() {
                        FontChoice::Default
                    } else {
                        FontChoice::BuiltIn(id.to_string())
                    }
                } else {
                    FontChoice::Default
                }
            }
        }
    }

    /// Produce the persisted form.
    pub(super) fn to_storage(&self) -> String {
        match self {
            FontChoice::Default => "default".to_string(),
            FontChoice::System(f) => format!("system:{}", f.id()),
            FontChoice::BuiltIn(id) => format!("builtin:{id}"),
        }
    }
}

impl Serialize for FontChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_storage())
    }
}

impl<'de> Deserialize<'de> for FontChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(FontChoice::from_storage(&s))
    }
}
