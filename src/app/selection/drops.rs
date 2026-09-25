//! The drag and its drop: the lift, the fold preview the pointer reads, and the
//! answer the release writes.

use super::{Drag, FOLD_DWELL_MS, Hot, SINK_DWELL_MS};
use crate::app::Mareader;
use crate::app::message::Message;
use crate::chrome::desktop;
use crate::library::drag::{Band, DragPayload, DropEffect, DropQuery, DropTargetKind, FoldPreview, drop_effect, fold_items, fold_preview};
use crate::library;
use iced::Task;
use iced::time::Instant;
use library_core::shelf::{self, ALL_SHELF};

impl Mareader {
    /// The movement's answer: the press becomes a drag. The payload is the
    /// whole set when the pressed cell is in it — in the page's own order,
    /// the same payload the bar's filings carry — else the one cell. The
    /// source is the shelf that rendered it: a drag lifted inside a shelf
    /// is a move out of that shelf, and reading the open level instead
    /// would unfile a book from the shelf it was showing in. The swallow
    /// flag is set the way the hold sets it — the release still belongs to
    /// the cell's button and must not open what was just picked up.
    pub(in crate::app) fn begin_drag(&mut self, id: &str, at: Instant) {
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

    /// The dwells' beat: a rest over a crumb parks the ghost there (the
    /// sink), and a rest over a book the table reads as a landing — the
    /// answer, not only the kind — arms the fold, so from the next tick
    /// the same rest answers the shelf the drop will make.
    pub(in crate::app) fn on_drag_tick(&mut self, at: Instant) {
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
    pub(in crate::app) fn drag_answer(&self) -> Option<(DropEffect, Option<FoldPreview>)> {
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
    pub(in crate::app) fn can_sibling_held(&self, held: &DragPayload, anchor: &str) -> bool {
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
    pub(in crate::app) fn release_drag(&mut self) -> Task<Message> {
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
    pub(in crate::app) fn apply_drop(&mut self, effect: DropEffect, payload: DragPayload) -> bool {
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
    pub(in crate::app) fn insert_anchor(
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
}
