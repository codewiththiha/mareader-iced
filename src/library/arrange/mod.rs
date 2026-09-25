//! The arrangement rules: where a book goes when it is filed, moved, or
//! dropped, and the order a level keeps.

mod file;
mod order;
mod tree;

pub use file::{file_many, move_many_to_shelf, unfile_books};
pub use order::{nest_many, nest_shelf, reorder_root, reorder_shelves_to_anchor};
pub use tree::{dismantle};

#[cfg(test)]
mod tests;
