//! The shelf bar: the crumbs that lead home, the panel behind the ellipsis,
//! and the rename field.

mod crumbs;
mod overflow;
mod search;

pub use crumbs::{CrumbFacts, breadcrumb};
pub use overflow::{ellipsis_anchor, ellipsis_panel, panel_size};
pub use search::{RENAME_INPUT, search};

#[cfg(test)]
mod tests;
