//! The library's menus: the bar's ＋ and view panels, and the menus a
//! right-click opens on a book, a folder, a shelf, a level or a selection. Each
//! builder returns the panel with the size its rows add up to, so placement can
//! clamp against what the panel really occupies before layout exists.
mod add;
mod book;
mod folder;
mod panel;
mod selection;
mod shelves;
mod view;

pub use add::*;
pub use book::*;
pub use folder::*;
pub use panel::*;
pub use selection::*;
pub use shelves::*;
pub use view::*;

#[cfg(test)]
mod tests;
