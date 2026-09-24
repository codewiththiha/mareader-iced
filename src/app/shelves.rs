//! The shelf's own structure: minting, dismantling, reordering, and the
//! folder rungs that seat them.
use std::collections::HashSet;

use iced::Task;
use library_core::book::{self};
use library_core::shelf::{self, ALL_SHELF};
use library_core::text as lib_text;

use crate::library::departure::{self, ShelfSeam};
use crate::library::{self};
use crate::platform::now_ms;
use super::Mareader;
use super::message::Message;
use super::moves::DepartLand;
use super::sheets::Sheet;

impl Mareader {
    pub(super) fn create_shelf(&mut self) -> Task<Message> {
        let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        self.create_shelf_in(parent)
    }

    /// Mint a shelf under an explicit parent — `None` hangs it at the top
    /// level — and step into it.
    pub(super) fn create_shelf_in(&mut self, parent: Option<String>) -> Task<Message> {
        let id = self.mint_shelf(parent);
        self.menu = None;
        self.context = None;
        self.shelf = id;
        self.persist_library()
    }

    /// Mint a shelf and hand back its id, without stepping into it: the
    /// selection's answers file onto a shelf the reader never leaves the
    /// level for. The first one is simply "New shelf"; the counter starts
    /// only once that name is taken.
    pub(super) fn mint_shelf(&mut self, parent: Option<String>) -> String {
        let id = library_core::id::next_shelf_id(now_ms());
        let in_use: HashSet<String> =
            self.library.shelves.iter().map(|shelf| shelf.name.clone()).collect();
        let name = if in_use.contains("New shelf") {
            book::duplicate_title("New shelf", &in_use)
        } else {
            "New shelf".to_string()
        };
        self.library
            .shelves
            .push(shelf::Shelf::virtual_shelf(id.clone(), name, parent));
        id
    }

    /// Take a shelf apart the way the web app takes a reader-made shelf
    /// apart, without asking: it holds no copies, so nothing is at risk —
    /// its books come up a level (unfiled, or still on the shelves that
    /// also name them), its children re-hang on its parent, and it goes. A
    /// folder rung's map pointer dies with it and the next walk prunes it;
    /// the copy question a rung's departure asks lands with the arrange
    /// work that owns it.
    pub(super) fn remove_shelf_with(&mut self, id: &str) -> Task<Message> {
        self.menu = None;
        self.context = None;
        if !self.dismantle_shelf(id) {
            return Task::none();
        }
        self.persist_library()
    }

    /// A shelf taken apart, receipt and navigation aside: the primitive is
    /// the tree's whole business — the children re-hang on its parent, the
    /// folder's own rungs re-hang the way its next scan would hang them, the
    /// folder lets the rung go in its map, and the books come up exactly one
    /// level — and a reader standing ON the level steps out to the level it
    /// hung from. True when the id named a shelf.
    pub(super) fn dismantle_shelf(&mut self, id: &str) -> bool {
        let was_inside = self.shelf == id;
        let Some(step_out) = library::arrange::dismantle(
            &mut self.library.shelves,
            &mut self.library.folders,
            &mut self.library.books,
            id,
        ) else {
            return false;
        };
        if was_inside {
            self.shelf = step_out;
        }
        true
    }

    /// One spelling for the three shelf hand-moves, because a drag, a bulk
    /// filing and a sibling reorder are one rule and one question. The rule
    /// is the core's own `departing_moves` — pure and host-tested — and the
    /// shelves that owe a departure ride the copy sheet, coming back through
    /// its own answer; the clean ones move now.
    pub(super) fn screened_shelf_moves(
        &mut self,
        ids: &[String],
        parent: Option<&str>,
        seam: Option<ShelfSeam>,
    ) -> Vec<String> {
        let (clean, departing) =
            shelf::departing_moves(&self.library.shelves, &self.library.folders, ids, parent);
        if !departing.is_empty() {
            let ask = departure::ask_of_shelf(
                &self.library.books,
                &self.library.shelves,
                &self.library.folders,
                departing,
                parent.map(str::to_string),
                seam,
            );
            if let Some(ask) = ask {
                self.sheet = Some(Sheet::Copy { ask });
            }
        }
        clean
    }

    /// What a drag onto a shelf row's edge commits — the sibling seam the
    /// list layout draws — behind the departure's screen, because the seam
    /// can stand on another level than the mover's seat.
    pub(super) fn reorder_shelves(&mut self, ids: &[String], anchor: &str, after: bool) -> bool {
        if ids.is_empty() {
            return false;
        }
        let parent = shelf::find(&self.library.shelves, anchor).and_then(|s| s.parent.clone());
        let seam = ShelfSeam { anchor_id: anchor.to_string(), after };
        let clean = self.screened_shelf_moves(ids, parent.as_deref(), Some(seam.clone()));
        if clean.is_empty() {
            return false;
        }
        library::arrange::reorder_shelves_to_anchor(&mut self.library.shelves, &clean, anchor, after)
    }

    /// Where held folders land after a drop: onto the root they re-hang with
    /// no parent, one reparent each because the root has no member list to
    /// batch into; onto a shelf they nest as a batch — behind the shelf
    /// move's own screen.
    pub(super) fn land_folders(&mut self, folders: &[String], to: &str) -> bool {
        if folders.is_empty() {
            return false;
        }
        let parent = (to != ALL_SHELF).then_some(to);
        let clean = self.screened_shelf_moves(folders, parent, None);
        if clean.is_empty() {
            return false;
        }
        let mut moved = false;
        for folder in &clean {
            moved |= library::arrange::nest_shelf(&mut self.library.shelves, folder, parent);
        }
        moved
    }

    /// The shelf door's copies: the rule is asked again — the sheet was up
    /// while the library went on living — a shelf that no longer owes a
    /// departure is skipped silently, and every departing shelf's books ride
    /// ONE batch, the way the whole gesture is one thing the reader asked
    /// for.
    pub(super) fn copy_shelves(
        &mut self,
        ids: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
    ) -> Task<Message> {
        let level = departure::landing_level(&self.library.shelves, &target, seam.as_ref());
        let deps = departure::shelf_departures(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            &ids,
            level.as_deref(),
        );
        if deps.is_empty() {
            self.advance_conflict();
            return Task::none();
        }
        let mut books: Vec<String> = Vec::new();
        for dep in &deps {
            for id in &dep.books {
                if !books.contains(id) {
                    books.push(id.clone());
                }
            }
        }
        let label = match deps.len() {
            1 => format!("“{}”", deps[0].name),
            n => lib_text::plural(n, "shelf", "shelves"),
        };
        let landing = DepartLand::ShelfMove { deps, level, seam };
        self.begin_depart_run(label, books, landing)
    }
}
