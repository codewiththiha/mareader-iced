//! Taking the reader to a row or a shelf — the level to stand on, the
//! offset the scroll owes, and what to light once the way is done.
//!
//! The RULE is here: the level a reveal navigates to and the offset its
//! scroll owes are pure readings of the library and the layout's own
//! constants, so the grid, the list and the app all read one reckoning of
//! where a card stands. The web page scrolled the DOM to the card and let
//! a stylesheet flash it; the native page owes the offset its own
//! arithmetic and a flash of its own hand, and both read from here.

use library_core::shelf::{self, Shelf, ALL_SHELF};

use super::card::COVER_RATIO;

/// The one write a reveal is: what to light, and the nonce that makes a
/// second reveal of the SAME thing a second reveal — a reader who opens
/// the one they already have twice walks to it twice, and the second flash
/// must not die with the first's clock.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reveal {
    pub id: String,
    pub nonce: u64,
}

/// The level a row's card stands on: the first shelf in shelf order, so
/// the answer is the same every time; the root when the book is on no
/// shelf.
pub fn level_of_row(shelves: &[Shelf], row_id: &str) -> String {
    shelf::containing(shelves, row_id)
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_else(|| ALL_SHELF.to_string())
}

/// The shelf half of the light, and where a folder link's tap goes: the
/// level that holds the shelf, because the reader's eye stays outside it.
pub fn level_of_shelf(shelves: &[Shelf], shelf_id: &str) -> String {
    shelf::find(shelves, shelf_id)
        .and_then(|s| s.parent.clone())
        .unwrap_or_else(|| ALL_SHELF.to_string())
}

// The layout's own constants, mirrored from the shelf's own view: the
// grid's cell is one cover plus its caption, with a 32px gap between runs,
// the list's row is the thumbnail's own row with a hairline between rows,
// and the frame keeps 32px of air above the first run. Scrolling to a
// cell is asking these to stay true; the view lives by them, and a cell
// they misjudge is a cell the light sits a row beside — the kind of wrong
// a reader sees at once.
const GRID_GAP_Y: f32 = 32.0;
const GRID_META_H: f32 = 44.0;
const LIST_ROW_H: f32 = 76.0;
const CONTENT_TOP: f32 = 32.0;

/// The grid's own one-cell height: the 3:4 cover plus the caption's pair
/// of lines and their padding. The caption's exact cut comes from the
/// card itself; the constant stands where the arithmetic lives so both
/// stay beside each other.
pub fn grid_cell_h(cell_w: f32) -> f32 {
    cell_w * COVER_RATIO + GRID_META_H
}

/// The offset the reveal owes for an item standing `index` items into the
/// level, so the card's middle ends up in the viewport's middle — the
/// scrollIntoView the web's sheet called, with the native layout's own
/// numbers. `tracks` is the grid's own column count; the list's rows are
/// tracks of one. `None` for an item that would make no sense to chase —
/// an empty level's zero is a scroll of nothing.
pub fn grid_offset(index: usize, tracks: usize, cell_w: f32, viewport_h: f32) -> f32 {
    if tracks == 0 {
        return 0.0;
    }
    let cell_h = grid_cell_h(cell_w);
    let run = (index / tracks) as f32;
    offset_for(CONTENT_TOP + run * (cell_h + GRID_GAP_Y), cell_h, viewport_h)
}

/// The list's own answer to the same question: rows at the thumbnail's
/// own height, one hairline between.
pub fn list_offset(index: usize, viewport_h: f32) -> f32 {
    let top = CONTENT_TOP + index as f32 * LIST_ROW_H;
    offset_for(top, LIST_ROW_H, viewport_h)
}

/// Centre the cell in the viewport, clamped so the scroll never owes the
/// content a row above its first.
fn offset_for(top: f32, cell_h: f32, viewport_h: f32) -> f32 {
    (top + cell_h / 2.0 - viewport_h / 2.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::shelf::ShelfKind;

    fn shelf_owned(id: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: parent.map(str::to_string),
            manual_parent: false,
        }
    }

    #[test]
    fn the_level_a_row_reveals_onto_is_the_first_shelf_holding_it() {
        let mut second = shelf_owned("Reading", None, &["e1"]);
        second.books.push("e3".to_string());
        let shelves = vec![second, shelf_owned("Later", None, &["e2"])];
        assert_eq!(level_of_row(&shelves, "e2"), "Later");
        assert_eq!(level_of_row(&shelves, "e1"), "Reading", "the FIRST shelf in shelf order");
        assert_eq!(level_of_row(&shelves, "gone"), ALL_SHELF, "no shelf is the library itself");
    }

    #[test]
    fn a_shelf_reveals_from_outside_itself() {
        let shelves = vec![
            shelf_owned("Reading", None, &[]),
            shelf_owned("Fic", Some("Reading"), &[]),
            shelf_owned("Root", None, &[]),
        ];
        assert_eq!(level_of_shelf(&shelves, "Fic"), "Reading");
        assert_eq!(level_of_shelf(&shelves, "Reading"), ALL_SHELF);
        assert_eq!(level_of_shelf(&shelves, "gone"), ALL_SHELF);
    }

    #[test]
    fn the_grid_offset_centres_the_cell_its_run_holds() {
        // One run of one column: the first cell of the grid scrolls
        // nowhere past its own centreing.
        let cell_h = grid_cell_h(180.0);
        let at = grid_offset(0, 1, 180.0, 800.0);
        assert_eq!(at, (CONTENT_TOP + cell_h / 2.0 - 400.0).max(0.0));
        // The second run carries one cell's height and one gap more.
        let run_two = grid_offset(4, 2, 180.0, 0.0);
        let expected = CONTENT_TOP + 2.0 * (cell_h + GRID_GAP_Y) + cell_h / 2.0;
        assert_eq!(run_two, expected, "the fourth item of two tracks is the third run's first cell");
    }

    #[test]
    fn offsets_never_owe_a_row_above_the_first() {
        assert_eq!(grid_offset(0, 4, 180.0, 2000.0), 0.0, "a cell above the fold scrolls nowhere");
        assert_eq!(list_offset(0, 2000.0), 0.0);
        assert_eq!(grid_offset(3, 0, 180.0, 500.0), 0.0, "no tracks is no scroll at all");
    }

    #[test]
    fn the_list_offset_counts_rows_at_their_own_height() {
        let at = list_offset(3, 0.0);
        assert_eq!(at, CONTENT_TOP + 3.0 * LIST_ROW_H + LIST_ROW_H / 2.0);
    }
}
