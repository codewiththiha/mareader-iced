//! The rescan ledger: what a scan of a watched folder does.
//!
//! A rescan runs on every window focus, so a wrong answer is not a crash — it is
//! a deleted book coming back, or a duplicate added on every click. The subjects
//! below split it by the question it answers: what the folder's last run knew,
//! what the scan decided, what its stones remember, and what a removal can still
//! give back.
mod recover;
mod registry;
mod scan;
mod stones;

pub use recover::*;
pub use registry::*;
pub use scan::*;
pub use stones::*;

#[cfg(test)]
mod tests;
