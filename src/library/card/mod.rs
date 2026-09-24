//! The shelf's cells: one book's card, one folder's plate, one link's tile,
//! and the add card that closes every grid.
//!
//! The grid's geometry is the web app's, carried over whole: a cover frame
//! at A4 portrait (210×297), 6px corners, the paper's own gradient behind a
//! title until real cover art lands with the engines, an info block under
//! the cover (the title on two lines, the author or the page line beneath),
//! and the 3px progress hairline when a book has one. Hover deepens the
//! shadow — the web card also translated up 2px, which a layout cell cannot
//! do natively, so the shadow carries the lift alone.
//!
//! Split by what each piece is for — the kit every cell draws with, the
//! badge, the row parts the list borrows, and one module per cell — while
//! the paths stay put: `card::book_card` still names the card.

mod kit;
mod badge;
mod row;
mod book;
mod folder;
mod link;
mod add;

pub use add::add_card;
pub use badge::badge_chip;
pub use book::book_card;
pub use folder::folder_card;
pub use kit::{
    drag_bands, right_target, sensed, stack_layers, state_fade, COVER_RATIO, META_H,
};
pub use link::link_card;
pub use row::{elide_line, format_chip, list_thumb, row_button_style, ROW_H, ROW_PAD, THUMB_W};

// What only the crate asks for: the seam colour the shelf's bar shares, and
// the cover cap the drag preview counts to.
pub(crate) use folder::plate_seam;
pub(crate) use kit::THUMB_CAP;
