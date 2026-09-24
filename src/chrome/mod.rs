//! The window chrome: everything the app draws that is not a document or a
//! shelf — the titlebar family, the per-OS caption clusters, the icon
//! sprite and the bar's own numbers.
//!
//! The web app kept these in its `app-chrome` crate, format-agnostic and
//! reusable; the split is the same here, one directory down.

pub mod captions;
pub mod desktop;
pub mod icons;
pub mod titlebar;
