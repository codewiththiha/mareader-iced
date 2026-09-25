//! What the reader does to one row: removing it, importing into it, watching its
//! ground, and lighting it up when a walk arrives.

mod imports;
mod remove;
mod reveal;
mod watch;

/// The covered walk's own seat: the rung shelf the ground named, and the
/// tree that walks it.
pub(in crate::app) struct Covered {
    pub(in crate::app) tree_root: String,
    pub(in crate::app) shelf_id: String,
    pub(in crate::app) shelf_name: String,
}
