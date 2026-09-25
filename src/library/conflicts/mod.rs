//! The name question: a level already holds a book of the name that is
//! arriving, and the sheet asks which of three things the reader meant.
//!
//! The rule itself is `library_core::conflict`, pure and host-tested; this is
//! the wiring between it and the things a reader can do about it — the ask
//! every placing surface hands its arrivals through, the small readers the
//! app answers them with, and the answers themselves.

pub mod naming;
pub mod planned;
pub mod words;

mod ask;
mod rules;

pub use ask::{AskKind, ConflictAsk, NoteKind, note_sentence};
pub use naming::{describe_shelf, shelf_offers, ShelfConflictAsk};
pub use planned::screen_planned;
pub use rules::{
    clean_move_ids, existing_name_of, file_on_all, member_slot, memberships, minted_name,
    moved_arrivals, offers_for, rename_row, screen, survivor_is_the_copy_of,
};
pub use words::describe;

#[cfg(test)]
mod kit;
#[cfg(test)]
mod tests;
