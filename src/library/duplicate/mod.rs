//! The shelf's "Duplicate": a second instance of one thing, asked for by name.
//! A duplicate is the app's own object from the moment it exists — nothing about
//! it is shared with what it came from — and the one thing that stays a pointer is
//! a link at a shelf, which held no bytes to copy. What a run reports back to the
//! shelf lives here, with the plans and the landings beside it.
mod land;
mod plan;
mod tree;

pub use land::*;
pub use plan::*;
pub use tree::*;

#[cfg(test)]
mod tests;

/// One landing, for the report the whole run ends with.
#[derive(Debug)]
pub struct Duplicated {
    pub name: String,
    pub shelf: bool,
}

/// The run's closing sentence: one landing is named, a batch is counted by
/// the kinds it landed.
pub fn report(landed: &[Duplicated]) -> String {
    if landed.len() == 1 {
        return format!("Duplicated as “{}”.", landed[0].name);
    }
    let shelves = landed.iter().filter(|one| one.shelf).count();
    let books = landed.len() - shelves;
    let noun = match (books, shelves) {
        (_, 0) => "books",
        (0, _) => "shelves",
        _ => "shelves and books",
    };
    format!("Duplicated {} {noun}.", landed.len())
}
