//! The choosing mode: the press that starts it, the hold that arms it, the
//! drag session, the drop and the membership edits that land.
use std::path::PathBuf;

use iced::time::Instant;
use iced::{Point, Task};
use library_core::book::{self};
use library_core::shelf::{self, ALL_SHELF};

use crate::chrome::desktop;
use crate::library::drag::{
    drop_effect, fold_items, fold_preview, Band, DragPayload, DropEffect, DropQuery, DropTargetKind, FoldPreview,
};
use crate::library::{self};
use super::Mareader;
use super::message::Message;


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

/// Which family of hover an exit arrived from: an exit only clears a hot
/// of its own family, the same rule [`Hot`] exists for.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Family {
    Card,
    Crumb,
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

impl Mareader {
    /// What a tap means: a membership while choosing, an open otherwise.
    /// The flag a finished hold leaves behind swallows this one tap — the
    /// release still belongs to the cell's button, and the choice the hold
    /// just started must not immediately toggle the cell back out.
    pub(super) fn card_tap(&mut self, id: &str) -> Task<Message> {
        if self.tap_swallow.take().is_some_and(|swallowed| swallowed == id) {
            return Task::none();
        }
        if self.selecting {
            self.toggle_selected(id);
            return Task::none();
        }
        self.menu = None;
        self.context = None;
        // Resolved to an owned answer first: the resolve borrows the blob,
        // and the acts that follow borrow the app.
        enum Tap {
            Open { id: String, path: PathBuf },
            Shelf(String),
            Nothing,
        }
        let tap = match book::find_row(&self.library.books, id) {
            Some(book::Row::Book(book)) => Tap::Open {
                id: book.id.clone(),
                path: PathBuf::from(book.path()),
            },
            // A link opens onto the shelf it points at — when it still
            // points at one.
            Some(book::Row::Link { target, .. }) if library_core::id::is_shelf(target) => {
                Tap::Shelf(target.clone())
            }
            _ => {
                if shelf::find(&self.library.shelves, id).is_some() {
                    Tap::Shelf(id.to_string())
                } else {
                    Tap::Nothing
                }
            }
        };
        match tap {
            Tap::Open { id, path } => self.open_row(Some(id), path),
            Tap::Shelf(shelf) => {
                self.navigate_to(shelf);
                Task::none()
            }
            Tap::Nothing => Task::none(),
        }
    }

    /// The hold machine's beat: a press that travelled past the drag's
    /// threshold is no hold (the drag session that lands later owns the
    /// gesture from there); a press that stayed quiet long enough starts
    /// the choice and marks the coming tap swallowed.
    pub(super) fn on_hold_tick(&mut self, at: Instant) {
        let Some(press) = &self.press else { return };
        let moved = (self.cursor.x - press.at.x).hypot(self.cursor.y - press.at.y);
        if moved > DRAG_THRESHOLD_PX {
            // The press became a drag: the hold machine hands the cell over
            // and stops counting — the drag's own clock, the dwell, starts
            // when its hot target does.
            let id = press.id.clone();
            self.press = None;
            self.begin_drag(&id, at);
            return;
        }
        if at.duration_since(press.started).as_millis() >= SELECT_PRESS_MS && moved <= SELECT_SLOP_PX
        {
            let id = press.id.clone();
            self.press = None;
            self.tap_swallow = Some(id.clone());
            self.enter_selection(&id);
        }
    }

    /// The hold's answer: the mode is on and the cell held is its first
    /// member.
    pub(super) fn enter_selection(&mut self, id: &str) {
        self.selecting = true;
        self.selected.insert(id.to_string());
    }

    /// The movement's answer: the press becomes a drag. The payload is the
    /// whole set when the pressed cell is in it — in the page's own order,
    /// the same payload the bar's filings carry — else the one cell. The
    /// source is the shelf that rendered it: a drag lifted inside a shelf
    /// is a move out of that shelf, and reading the open level instead
    /// would unfile a book from the shelf it was showing in. The swallow
    /// flag is set the way the hold sets it — the release still belongs to
    /// the cell's button and must not open what was just picked up.
    fn begin_drag(&mut self, id: &str, at: Instant) {
        let source = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        let payload = if self.selecting && self.selected.contains(id) {
            let (books, folders) = self.split_selection();
            DragPayload { books, folders, source }
        } else if self.library.shelves.iter().any(|shelf| shelf.id == id) {
            DragPayload { books: Vec::new(), folders: vec![id.to_string()], source }
        } else {
            DragPayload { books: vec![id.to_string()], folders: Vec::new(), source }
        };
        if payload.is_empty() {
            return;
        }
        self.tap_swallow = Some(id.to_string());
        self.select_pop = false;
        self.drag = Some(Drag {
            payload,
            band: Band::Middle,
            dwell_armed: false,
            sunk: None,
            hot_started: at,
            last_hot: self.hovered_card.clone().map(Hot::Card),
        });
    }

    /// A drag's hot target changed: the band falls back to the middle,
    /// both dwells' clock restarts, a rest that was counting is forgotten
    /// and a ghost that was parked grows back. The same fact the cells
    /// dress by — the hover truth — decides when the clocks restart, so a
    /// pointer that circles inside one card keeps its rest and a pointer
    /// that crosses a seam does not.
    pub(super) fn on_hot_change(&mut self, next: Option<Hot>, at: Instant) {
        let Some(drag) = &mut self.drag else { return };
        if drag.last_hot == next {
            return;
        }
        drag.last_hot = next;
        drag.band = Band::Middle;
        drag.dwell_armed = false;
        drag.sunk = None;
        drag.hot_started = at;
    }

    /// An exit's clear, guarded: it lands only when the session's hot
    /// still belongs to the exiting family, because the bar's messages
    /// queue after the level's and the enter of the new hot arrives
    /// before the exit of the old one.
    pub(super) fn on_hot_exit(&mut self, family: Family, at: Instant) {
        let owns = self
            .drag
            .as_ref()
            .and_then(|drag| drag.last_hot.as_ref())
            .is_some_and(|hot| hot.family() == family);
        if owns {
            self.on_hot_change(None, at);
        }
    }

    /// The panel shut, wholesale: a chain that changed (the fold is an
    /// answer about levels that no longer show) or a drag that ended (the
    /// web intent closes with the session) both land here.
    pub(super) fn close_ellipsis(&mut self) {
        self.ellipsis_open = false;
        self.ellipsis_close_at = None;
    }

    /// The dwells' beat: a rest over a crumb parks the ghost there (the
    /// sink), and a rest over a book the table reads as a landing — the
    /// answer, not only the kind — arms the fold, so from the next tick
    /// the same rest answers the shelf the drop will make.
    pub(super) fn on_drag_tick(&mut self, at: Instant) {
        let Some(drag) = self.drag.as_ref() else { return };
        let rested = at.duration_since(drag.hot_started).as_millis();
        if self.hovered_crumb.is_some() {
            // A crumb's answer is a filing and never a refusal, so the web
            // session's sinkable check — a Shelf target the table does not
            // refuse — is the hover alone.
            if drag.sunk.is_none()
                && rested >= SINK_DWELL_MS
                && let Some(drag) = &mut self.drag
            {
                drag.sunk = Some(self.cursor);
            }
            return;
        }
        if drag.dwell_armed || rested < FOLD_DWELL_MS {
            return;
        }
        // An InsertBefore is the table's own proof the hot target is an
        // unheld book: no other target kind answers with one.
        let Some((effect, _)) = self.drag_answer() else { return };
        if !matches!(effect, DropEffect::InsertBefore { .. }) {
            return;
        }
        if let Some(drag) = &mut self.drag {
            drag.dwell_armed = true;
        }
    }

    /// The table's answer for the drag as it stands: the effect a release
    /// right now would commit, and the fold preview the ghost would wear.
    /// Pure — recomputed per tick, per release and per frame rather than
    /// cached, because every input is already in hand.
    pub(super) fn drag_answer(&self) -> Option<(DropEffect, Option<FoldPreview>)> {
        let drag = self.drag.as_ref()?;
        // The ellipsis is a hover target and never a drop: it stands for
        // several levels, and the table's refusal keeps the ghost honest
        // while the panel opens under the rest.
        if self.cursor.y < desktop::TITLE_BAR_H
            && drag.last_hot.as_ref() == Some(&Hot::Ellipsis)
        {
            let query = DropQuery {
                held_books: drag.payload.books.len(),
                held_folders: drag.payload.folders.len(),
                target_kind: DropTargetKind::Ellipsis,
                target_id: "",
                target_is_held: false,
                can_nest: false,
                can_sibling: false,
                band: Band::Middle,
                target_shelf: None,
                dwell_armed: drag.dwell_armed,
            };
            return Some((drop_effect(query), None));
        }
        // The crumbs go first: they live in the titlebar, and the bar's
        // refusal below is about the rest of the chrome. Every crumb is a
        // Shelf target — the way back to a level is also the way to file
        // onto it from anywhere in the library — and Home's empty id is
        // the library's own floor, whose answer takes the hold off the
        // shelf it was dragged out of.
        // The bar can leave the tree without an exit (a reveal that faded
        // with the pointer already gone), so the crumb answer carries the
        // crumb's own geometry check: a crumb lives in the titlebar — the
        // fold panel's crumbs, still Shelf targets, being the one hanging
        // exception, below the bar while the panel is open.
        if (self.cursor.y < desktop::TITLE_BAR_H || self.ellipsis_open)
            && let Some(id) = &self.hovered_crumb
        {
            let query = DropQuery {
                held_books: drag.payload.books.len(),
                held_folders: drag.payload.folders.len(),
                target_kind: DropTargetKind::Shelf,
                target_id: id,
                target_is_held: drag.payload.contains(id),
                can_nest: false,
                can_sibling: false,
                band: Band::Middle,
                target_shelf: None,
                dwell_armed: drag.dwell_armed,
            };
            return Some((drop_effect(query), None));
        }
        // The rest of the bar is not a target: the web registry never held
        // it, and a release over the chrome must not file the hold onto
        // the level.
        if self.cursor.y < desktop::TITLE_BAR_H {
            return None;
        }
        let folders = library::level_folders(&self.library, &self.shelf, &self.query);
        let rows = library::level_rows(&self.library, &self.shelf, &self.query);
        let (kind, target_id) = match &self.hovered_card {
            Some(id) if folders.iter().any(|folder| folder.id == *id) => {
                (DropTargetKind::Folder, id.clone())
            }
            Some(id) if rows.iter().any(|row| row.id() == id.as_str()) => {
                (DropTargetKind::Book, id.clone())
            }
            // A hover the level no longer renders — a scan landed, a query
            // narrowed — is no target at all rather than the level's.
            Some(_) => return None,
            None => (
                DropTargetKind::Level,
                if self.shelf == ALL_SHELF { String::new() } else { self.shelf.clone() },
            ),
        };
        let row_shelf = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        let query = DropQuery {
            held_books: drag.payload.books.len(),
            held_folders: drag.payload.folders.len(),
            target_kind: kind,
            target_id: &target_id,
            target_is_held: drag.payload.contains(&target_id),
            can_nest: kind == DropTargetKind::Folder
                && drag
                    .payload
                    .folders
                    .iter()
                    .all(|held| shelf::can_nest(&self.library.shelves, held, &target_id)),
            can_sibling: kind == DropTargetKind::Folder
                && self.can_sibling_held(&drag.payload, &target_id),
            band: drag.band,
            target_shelf: row_shelf.as_deref(),
            dwell_armed: drag.dwell_armed,
        };
        let effect = drop_effect(query);
        let fold = match &effect {
            DropEffect::CreateFolder { with_book_id } => {
                fold_preview(fold_items(&query), with_book_id)
            }
            _ => None,
        };
        Some((effect, fold))
    }

    /// A root-level seam has no parent to close a loop through, so an
    /// anchor at the top only refuses a shelf asked to sibling itself or
    /// one whose filing the graph would refuse.
    fn can_sibling_held(&self, held: &DragPayload, anchor: &str) -> bool {
        let Some(target) = shelf::find(&self.library.shelves, anchor) else {
            return false;
        };
        held.folders.iter().all(|each| {
            each != anchor
                && target.parent.as_deref().is_none_or(|parent| {
                    shelf::can_nest(&self.library.shelves, each, parent)
                })
        })
    }

    /// The release: one last answer from the table, and — unless it refused
    /// — the commit. A drop that wrote exits the choice the way every act
    /// that consumes the set does; a refusal leaves everything exactly as
    /// it was, which is the cancel the gesture owes a reader who let go
    /// over nothing.
    pub(super) fn release_drag(&mut self) -> Task<Message> {
        let answer = self.drag_answer();
        let Some(drag) = self.drag.take() else { return Task::none() };
        // The session's end is the intent's end: the web panel closes with
        // the drag whether the pointer moved off it or not.
        self.close_ellipsis();
        let wrote = answer.as_ref().is_some_and(|(effect, _)| *effect != DropEffect::Refused);
        let mut moved = false;
        if let Some((effect, _)) = answer {
            moved = self.apply_drop(effect, drag.payload);
        }
        if wrote && self.selecting {
            self.exit_selection();
        }
        if moved {
            return self.persist_library();
        }
        Task::none()
    }

    /// The only place a drop touches library state — the web commit carried
    /// over whole: every move rides the arrange primitives the shelf's
    /// menus already ride, so a dragged book persists exactly as a filed
    /// one. True when anything moved; the caller persists on the answer.
    fn apply_drop(&mut self, effect: DropEffect, payload: DragPayload) -> bool {
        if payload.is_empty() {
            return false;
        }
        // A drag inside a shelf is that shelf's: reading the page's level
        // instead would unfile a book that sits on both, for a reorder
        // that never left the shelf.
        let from = payload.source.clone().filter(|named| named.as_str() != ALL_SHELF);
        let mut moved = false;
        match effect {
            DropEffect::Refused => {}
            DropEffect::InsertBefore { book_id, shelf, after } => {
                // The effect carries the landing facts — container and
                // seam side — rather than this step re-deriving them. Held
                // folders get no position: a level renders its folders
                // before its books.
                let (to, index) = self.insert_anchor(&book_id, shelf.as_deref(), after);
                moved |= self.gated_seat(&payload.books, from.clone(), to.clone(), index, &[]);
                moved |= self.land_folders(&payload.folders, &to);
            }
            DropEffect::ShelfSibling { anchor_id, after } => {
                moved |= self.reorder_shelves(&payload.folders, &anchor_id, after);
            }
            DropEffect::FileToShelf { shelf_id } if shelf_id.is_empty() => {
                match from.as_deref() {
                    Some(shelf) => {
                        moved |= self.gated_unfile(&payload.books, shelf);
                    }
                    None => {
                        moved |= library::arrange::move_many_to_shelf(
                            &mut self.library.shelves,
                            &mut self.library.books,
                            &payload.books,
                            None,
                            ALL_SHELF,
                            None,
                        );
                    }
                }
                moved |= self.land_folders(&payload.folders, ALL_SHELF);
            }
            DropEffect::FileToShelf { shelf_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), shelf_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &shelf_id);
            }
            DropEffect::NestInto { folder_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), folder_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &folder_id);
            }
            DropEffect::CreateFolder { with_book_id } => {
                // The fold lands on the level the drag was standing on —
                // the same shelf the web's create-here mints under — and
                // the row it brewed around joins the payload's books.
                let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
                let shelf_id = self.mint_shelf(parent);
                let mut books = payload.books;
                if !books.contains(&with_book_id) {
                    books.push(with_book_id);
                }
                moved |= self.gated_seat(&books, from.clone(), shelf_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &shelf_id);
            }
        }
        moved
    }

    /// The index is the anchor's position in its container, not a count of
    /// what is on screen — and only a view that reorders by drag asks for
    /// one at all (the web commit's `insert_anchor`).
    fn insert_anchor(
        &self,
        book_id: &str,
        shelf: Option<&str>,
        after: bool,
    ) -> (String, Option<usize>) {
        let reorder = self.library.view.drag_reorders();
        let container: Option<String> = match shelf {
            Some(named) => (named != ALL_SHELF).then(|| named.to_string()),
            None => (self.shelf != ALL_SHELF).then(|| self.shelf.clone()),
        };
        let step = usize::from(after && reorder);
        match container {
            Some(id) => {
                let index = reorder.then(|| {
                    shelf::find(&self.library.shelves, &id)
                        .and_then(|each| each.books.iter().position(|member| member == book_id))
                        .map_or(0, |at| at + step)
                });
                (id, index)
            }
            None => {
                let index = reorder.then(|| {
                    self.library
                        .books
                        .iter()
                        .position(|row| row.id() == book_id)
                        .map_or(0, |at| at + step)
                });
                (ALL_SHELF.to_string(), index)
            }
        }
    }

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
