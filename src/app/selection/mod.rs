//! The choosing set and the pointer's own machines: the vocabulary the app
//! shares about what is hot, held and chosen.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::library::drag::{Band, DragPayload};
use crate::library;
use iced::time::Instant;
use iced::{Point, Task};

/// A rest over a card brews the fold: long enough that a reorder crossing it
/// never mints a shelf, short enough that nobody waits.
const FOLD_DWELL_MS: u128 = 650;

/// Shorter than the fold's: a full-size ghost hides the very crumb being aimed
/// at, so a rest this long parks it, shrunk.
const SINK_DWELL_MS: u128 = 420;

/// A hold in flight: the cell the press landed on, where it landed, and
/// when it started. A press that moves past the drag's threshold stops
/// being a hold — the gesture that arrives with the drag session owns the
/// same fields from there.
pub(super) struct Press {
    pub(super) id: String,
    pub(super) at: Point,
    pub(super) started: Instant,
}

/// The session's hot target's identity: which cell or crumb the pointer
/// is on, in the family it arrived from. An exit only clears a hot of its
/// own family — the bar rides above the level in the tree, so a move from
/// a crumb onto a card queues the card's enter BEFORE the crumb's exit,
/// and an unguarded exit would wipe the hot that just arrived.
#[derive(Clone, PartialEq)]
pub(super) enum Hot {
    Card(String),
    Crumb(String),
    /// The fold's ellipsis: a hover target, never a drop — it stands for
    /// several levels and cannot say which one the hold would choose.
    Ellipsis,
}

impl Hot {
    fn family(&self) -> Family {
        match self {
            Hot::Card(_) => Family::Card,
            Hot::Crumb(_) => Family::Crumb,
            Hot::Ellipsis => Family::Ellipsis,
        }
    }
}

/// Which family of hover an exit arrived from: an exit only clears a hot
/// of its own family, the same rule [`Hot`] exists for.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Family {
    Card,
    Crumb,
    Ellipsis,
}

/// A drag in flight: what it holds, the band the sensors last reported,
/// the fold dwell's clock and the crumb sink's spot. The hot target is the
/// shelf's hover truth — the same fact the press machine reads — so the
/// grid needs no sensor of its own, and a release over nothing is the
/// level's answer.
pub(super) struct Drag {
    pub(super) payload: DragPayload,
    /// The band the hot cell's sensor zones last reported; the middle
    /// until one does.
    pub(super) band: Band,
    /// Whether the rest over the hot target has passed the fold's dwell.
    pub(super) dwell_armed: bool,
    /// Where the ghost parked when the rest over a crumb passed the sink's
    /// dwell: from here the ghost reads the target rather than the hand,
    /// until the hot changes and it grows back.
    pub(super) sunk: Option<Point>,
    /// When the hot target became hot — both dwells' clock.
    pub(super) hot_started: Instant,
    /// The hot target the clocks are counting for.
    pub(super) last_hot: Option<Hot>,
}

/// The web gesture's tuning, carried over whole (long_press.rs): the hold
/// decides at 450ms, a press that travelled more than 8px is no hold, and
/// a press that travelled more than 6px is a drag — the drag's threshold
/// is inside the hold's slop, so a press that moved enough to drag can
/// never also decide to hold.
const SELECT_PRESS_MS: u128 = 450;

const SELECT_SLOP_PX: f32 = 8.0;

const DRAG_THRESHOLD_PX: f32 = 6.0;

const _: () = assert!(DRAG_THRESHOLD_PX < SELECT_SLOP_PX);

/// The selection bar's popover width.
pub(super) const SELECT_POP_W: f32 = 240.0;

mod drops;
mod hover;

impl Mareader {
    /// A tap while choosing: in out, out in. An empty set keeps the mode —
    /// leaving is Done's, Escape's, or the floor's answer, never a count's.
    pub(super) fn toggle_selected(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
    }

    /// Every exit goes through here — Done, Escape, a press on the floor,
    /// an action that consumed the set, leaving the page.
    pub(super) fn exit_selection(&mut self) {
        self.selecting = false;
        self.selected.clear();
        self.select_pop = false;
    }

    /// The set split into its two kinds, in the level's own order: the
    /// folders the level renders, then the rows it renders. A payload for
    /// a filing keeps the page's order, because putting the set back in
    /// the list's order would be an act that quietly shuffled the hand.
    pub(super) fn split_selection(&self) -> (Vec<String>, Vec<String>) {
        let books = library::level_rows(&self.library, &self.shelf, &self.query)
            .iter()
            .map(|row| row.id().to_string())
            .filter(|id| self.selected.contains(id))
            .collect();
        let shelves = library::level_folders(&self.library, &self.shelf, &self.query)
            .into_iter()
            .map(|shelf| shelf.id)
            .filter(|id| self.selected.contains(id))
            .collect();
        (books, shelves)
    }

    /// The bar's two filing answers: onto the shelf named, or onto a shelf
    /// minted for the occasion. Books go as memberships, folders go as
    /// nestings, and one persist covers the batch. A folder that cannot be
    /// nested onto the target — it would end up inside itself — stays
    /// where it is rather than failing the batch.
    pub(super) fn file_selection_to(&mut self, target: Option<String>) -> Task<Message> {
        self.context = None;
        let (book_ids, folder_ids) = self.split_selection();
        let target = match target {
            Some(id) => id,
            None => self.mint_shelf(None),
        };
        // Books go through the filing's own gate — the level's name screen
        // and the return's bind — and folders, whose names no level
        // collides with, ride the shelf move's own screen: a rung dropped
        // off the seat its tree names asks before it goes.
        let mut moved = self.gated_file(&book_ids, &target);
        let clean = self.screened_shelf_moves(&folder_ids, Some(&target), None);
        moved |= library::arrange::nest_many(&mut self.library.shelves, &clean, &target);
        self.exit_selection();
        if moved {
            return self.persist_library();
        }
        Task::none()
    }
}
