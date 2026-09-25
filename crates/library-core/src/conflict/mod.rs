//! Name collisions on a level: does the shelf this is going to already hold a
//! book called that?
//!
//! Nothing else about duplicates is a dialog. A second copy of one file is a
//! naming problem with a naming answer; a reader who wants a pointer rather
//! than a copy gets a [`crate::book::Row::Link`].

mod arrival;
mod ask;
mod names;
mod placement;

pub use arrival::Arrival;
pub use ask::PlacementAsk;
pub use names::{collide, collide_shelf, next_name, next_shelf_name, same_name};
pub use placement::{Placement, Scope};

#[cfg(test)]
mod kit;
