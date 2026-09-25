//! The shelf: the library's surface, from the level it is standing on down to
//! the cells it paints. The bar's slots live in [`bar`] and [`menus`]; the cells
//! in [`card`] and [`list`].

pub mod arrange;
pub mod bar;
pub mod card;
pub mod conflicts;
pub mod departure;
pub mod drag;
pub mod duplicate;
pub mod facts;
pub mod fold;
pub mod list;
pub mod menus;
pub mod reveal;

mod metrics;
mod rows;
mod view;

use metrics::{CONTENT_TOP, ROW_GAP};

pub use metrics::{content_metrics, report_fit};
pub use rows::{DragFacts, SelectionFacts, level_folders, level_rows};
pub use view::view;
