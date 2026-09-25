//! The book schema: [`Fingerprint`], [`Origin`], [`Book`] and the [`Row`] a
//! library list holds. The rules that act on rows live one module per question —
//! [`read`], [`merge`], [`check`], [`sanitize`], [`naming`], [`query`] — with the
//! cases for all of it in `tests.rs`.

mod schema;

pub mod check;
pub mod merge;
pub mod naming;
pub mod query;
pub mod read;
pub mod sanitize;
pub use check::{add_book, apply_check, drop_dangling_links, drop_dead_shelf_links, remove_row};
pub use merge::{fold_books, further_point};
pub use naming::{duplicate_title, stem_of};
pub use query::{find_book_mut, find_by_id, find_by_path, index_by_id, resume_point};
pub use schema::{BOOKS_CAP, Fingerprint, Origin, Book, Row, book_rows, book_rows_mut, find_row, find_row_mut};
pub use read::{ReadPoint, record_read, record_read_row, rows_for_read};
pub use sanitize::{sanitize};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod kit;
