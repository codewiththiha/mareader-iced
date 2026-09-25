//! The titlebar search: which books a query keeps, which shelves, and which
//! books the bar suggests while the reader is still typing.

mod matching;
mod ranking;

pub use matching::{is_active, matches, matches_terms};
pub use ranking::{SUGGEST_LIMIT, Suggestion, filter, suggest};

#[cfg(test)]
mod tests;
