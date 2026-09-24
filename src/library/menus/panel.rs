//! The vocabulary the menus are written in: the line a described row is made of,
//! and the widths the library's panels are cut to.

use crate::app::Message;
use crate::chrome::icons::IconName;

/// The add menu's panel width.
pub(super) const ADD_W: f32 = 220.0;

/// The view menu's panel width.
pub(super) const VIEW_W: f32 = 264.0;

/// The shelf menu's panel width.
pub(super) const SHELF_W: f32 = 224.0;

/// One computed menu line: what the app knows about a watch seat or a
/// restore candidate, laid out by the menu. `None` for the message renders
/// the row disabled — listed, but not quietly dropped.
pub struct MenuLine {
    pub icon: IconName,
    pub label: String,
    pub sublabel: Option<String>,
    pub message: Option<Message>,
}

/// A context menu's panel width.
pub(super) const CONTEXT_W: f32 = 232.0;
