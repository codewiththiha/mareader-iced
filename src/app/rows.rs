//! The row asks: renames, removals, the watch toggle, and the reveal a
//! landing scrolls the shelf to.
use std::path::PathBuf;

use iced::Task;
use iced::time::Instant;
use iced::widget::{operation, scrollable};
use library_core::book::{self};
use library_core::folder::{self as folder_ops, FolderOpts, Tombstone};
use library_core::governance::Governance;
use library_core::ledger::{self};
use library_core::shelf::{self, ALL_SHELF};
use library_core::{paths, text as lib_text};

use crate::chrome::icons::IconName;
use crate::library::departure::{self};
use crate::library::reveal::{self, Reveal};
use crate::library::{self, menus};
use crate::platform::{fs, now_ms, progress};
use crate::ui::toast::Tone;
use super::message::Message;
use super::sheets::Sheet;
use super::walk::{Asked, FsRun, GroundWatch, RootPlan, ShelfWatch, Stage};
use super::{LIBRARY_SCROLL, Mareader};

/// The covered walk's own seat: the rung shelf the ground named, and the
/// tree that walks it.
pub(in crate::app) struct Covered {
    pub(in crate::app) tree_root: String,
    pub(in crate::app) shelf_id: String,
    pub(in crate::app) shelf_name: String,
}

impl Mareader {
    /// The removal, and everything the library holds about the row going
    /// out: the memberships, the links that pointed at it, the tombstone
    /// that keeps a watched folder's rescan from putting the book straight
    /// back, and — for a copy the library owns — the byte in the store.
    pub(super) fn remove_row(&mut self, id: &str) -> Task<Message> {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return Task::none();
        };
        let name = row.display_name();
        let was_book = row.book().is_some();
        if !self.purge_row(id) {
            return Task::none();
        }
        let line = if was_book {
            format!("Removed “{name}” from the library")
        } else {
            format!("Removed the link “{name}”")
        };
        self.toasts.show(Tone::Info, line, Instant::now());
        self.persist_library()
    }

    /// The removal sheet's own door: a shelf coming off the list that reads
    /// books in place buys their copies first, and a removal with nothing to
    /// copy runs at once.
    pub(super) fn remove_entries(&mut self, purge: Vec<String>, shelves: Vec<String>) -> Task<Message> {
        let ask = departure::ask_of_removal(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            &purge,
            &shelves,
        );
        match ask {
            Some(ask) => {
                self.sheet = Some(Sheet::Copy { ask });
                Task::none()
            }
            None => {
                if self.remove(purge, shelves) {
                    self.persist_library()
                } else {
                    Task::none()
                }
            }
        }
    }

    /// The removal, whole: the books go out of the library the way one does —
    /// the ledger's tombstone, the swept copy, the memberships, the links —
    /// and the shelves come off the list deepest first, because a shelf
    /// dissolved first is a shelf no sweep reaches. One receipt; the persist
    /// is the caller's, so a removal that rides a copy run is one write with
    /// it.
    pub(super) fn remove(&mut self, purge: Vec<String>, shelves: Vec<String>) -> bool {
        let removed_books = purge.iter().filter(|id| self.purge_row(id)).count();
        let mut going: Vec<(usize, String)> = shelves
            .into_iter()
            .map(|id| (shelf::ancestors(&self.library.shelves, &id).len(), id))
            .collect();
        going.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        let removed_shelves = going.into_iter().filter(|(_, id)| self.dismantle_shelf(id)).count();
        if removed_books == 0 && removed_shelves == 0 {
            return false;
        }
        let mut parts: Vec<String> = Vec::new();
        if removed_books > 0 {
            parts.push(lib_text::plural(removed_books, "book", "books"));
        }
        if removed_shelves > 0 {
            parts.push(lib_text::plural(removed_shelves, "shelf", "shelves"));
        }
        let line = format!("Removed {} from the library", parts.join(" and "));
        self.toasts.show(Tone::Info, line, Instant::now());
        true
    }

    /// What the menus call: a rung holding books read in place asks first,
    /// and every other shelf comes apart at once, because nothing about it
    /// is a question.
    pub(super) fn ask_shelf_apart(&mut self, id: &str) -> Task<Message> {
        let ask = departure::ask_of_rung(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            id,
        );
        match ask {
            Some(ask) => {
                self.sheet = Some(Sheet::Copy { ask });
                Task::none()
            }
            None => self.remove_shelf_with(id),
        }
    }

    /// A row leaving the library, receipt aside: the tombstone the ledger
    /// needs, the copy swept when no twin reads the byte, the memberships,
    /// the links that pointed at it. True when the id named a row.
    pub(super) fn purge_row(&mut self, id: &str) -> bool {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return false;
        };
        let doomed = row.book().cloned();
        if let Some(book) = &doomed {
            // Read the world before writing any of it: the tombstone needs
            // the folder that placed this book and the shelf it was filed
            // on.
            let placed_by = self
                .library
                .folders
                .iter()
                .find(|folder| folder.placed.contains(&book.fp))
                .map(|folder| folder.id.clone());
            let home = placed_by
                .as_deref()
                .and_then(|folder_id| {
                    departure::folder_shelf_of(&self.library.shelves, folder_id, &book.id)
                });
            let entry = Tombstone::of(book, home, now_ms());
            ledger::tombstone(&mut self.library.folders, &entry);
        }
        self.unlist_row(id);
        if let Some(book) = &doomed {
            self.sweep_row_bytes(book);
        }
        true
    }

    /// The pickers' and the drop's answer: measure the files, then land
    /// them the way the library holds loose arrivals — as its own stored
    /// copies, except the ones a read-at-place tree answers for, which come
    /// back as that tree's linked books.
    pub(super) fn import_files(&mut self, picked: Option<Vec<PathBuf>>) -> Task<Message> {
        self.menu = None;
        let Some(paths) = picked else { return Task::none() };
        if paths.is_empty() {
            return Task::none();
        }
        let addresses: Vec<String> =
            paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        let label = if addresses.len() == 1 {
            paths::file_name(&addresses[0])
        } else {
            format!("{} files", addresses.len())
        };
        // The level the pick lands its books on, captured at the ask: a
        // reader who steps away during the copy does not move the landing.
        let target = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink,
            rx,
            latest: None,
            stage: Stage::Measuring { target },
        });
        Task::perform(
            async move { fs::check_paths(&addresses) },
            move |checks| Message::FilesChecked(task, checks),
        )
    }

    /// Open the import sheet for a picked or dropped folder, seeded from
    /// the ground: a folder a tree governs opens with the tree's own
    /// answers and the rung's watch, so the sheet asks nothing the tree
    /// already answered. A ground no tree governs keeps the last answers,
    /// the way the open floor does.
    pub(super) fn open_import_sheet(&mut self, dir: PathBuf) {
        let ground = self.ground_tracking(&dir.to_string_lossy());
        if let Some(watch) = &ground {
            self.import_opts = FolderOpts { watch: watch.on, ..watch.opts.clone() };
        }
        self.sheet = Some(Sheet::Import { root: dir, ground });
    }

    /// Which tree governs a picked ground, and the rung the ground names
    /// in it: the covering tree's own seat when a rung shelf stands, else
    /// the family the directory's address belongs to — a rung whose shelf
    /// was deleted or departed as a copy still seeds from its family's
    /// answers. `None` for a ground no tree owns.
    fn ground_tracking(&self, root: &str) -> Option<GroundWatch> {
        let governance = Governance::new(&self.library.folders, &self.library.shelves);
        let (tree_id, rung) = match governance.covering(root) {
            Some(covered) => (covered.folder_id, covered.rel),
            None => shelf::family_for(&self.library.folders, &self.library.shelves, root)?,
        };
        let row = folder_ops::find(&self.library.folders, &tree_id)?;
        let mut opts = row.opts.clone();
        // The structure answer is the rung's own, not the tree's root:
        // per-rung shapes are the whole of what a subfolder import asks.
        opts.groups = row.shape_at(&rung);
        let on = row.tracks_rung(&rung);
        Some(GroundWatch { tree_id, rung, on, opts })
    }

    /// The sheet's watch answer, written onto the rung it belongs to —
    /// never onto the tree's root — and persisted when it moved.
    pub(super) fn write_rung_tracking(&mut self, watch: &GroundWatch) -> Task<Message> {
        let moved = match folder_ops::find_mut(&mut self.library.folders, &watch.tree_id) {
            Some(folder) if folder.tracks_rung(&watch.rung) != watch.on => {
                folder.set_tracking(&watch.rung, watch.on);
                true
            }
            _ => false,
        };
        if moved {
            self.persist_library()
        } else {
            Task::none()
        }
    }

    /// The covered walk's seat: the rung shelf the ground names, its own
    /// name, and the tree's root — everything the landing needs to answer
    /// the pick back to the shelf it meant.
    pub(super) fn covered_shelf(&self, root: &str) -> Option<Covered> {
        let governance = Governance::new(&self.library.folders, &self.library.shelves);
        let covered = governance.covering(root)?;
        let tree_root =
            folder_ops::find(&self.library.folders, &covered.folder_id)?.root.clone();
        let shelf_name = shelf::find(&self.library.shelves, &covered.shelf_id)
            .map(|each| each.name.clone())
            .unwrap_or_else(|| paths::dir_label(&tree_root));
        Some(Covered {
            tree_root,
            shelf_id: covered.shelf_id,
            shelf_name,
        })
    }

    /// The watch seat a folder shelf answers for. `None` when the shelf is
    /// not a folder's seat — the toggle row the folder context menu shows
    /// only for one that is.
    fn shelf_watch(&self, shelf_id: &str) -> Option<ShelfWatch> {
        let seat =
            Governance::new(&self.library.folders, &self.library.shelves).seat_of(shelf_id)?;
        let folder = folder_ops::find(&self.library.folders, &seat.folder_id)?;
        let rung_label = (!seat.rung.is_empty())
            .then(|| paths::dir_label(&folder_ops::dir_of_rung(&folder.root, &seat.rung)));
        Some(ShelfWatch {
            on: folder.tracks_rung(&seat.rung),
            label: paths::dir_label(&folder.root),
            rung_label,
            folder_id: seat.folder_id,
            rung: seat.rung,
        })
    }

    /// The folder context menu's watch row, in the seat's own words. A
    /// deep seat answers for its rung alone; a root seat answers for the
    /// whole tree — and the glyph flips with the answer, because "stop
    /// watching" and "watch" are two different promises.
    pub(super) fn watch_facts(&self, shelf_id: &str) -> Option<menus::WatchFacts> {
        let watch = self.shelf_watch(shelf_id)?;
        let deep = watch.rung_label.is_some();
        let (glyph, label) = match (watch.on, deep) {
            (true, true) => (IconName::EyeOff, "Stop watching this subfolder"),
            (true, false) => (IconName::EyeOff, "Stop watching for new books"),
            (false, true) => (IconName::Eye, "Watch this subfolder for new books"),
            (false, false) => (IconName::Eye, "Watch for new books"),
        };
        let sublabel = match &watch.rung_label {
            Some(rung) => format!("Only “{rung}” and the folders inside it"),
            None => format!("The whole “{}” folder", watch.label),
        };
        Some(menus::WatchFacts {
            icon: glyph,
            label: label.to_string(),
            sublabel,
            message: Message::ToggleWatch(shelf_id.to_string()),
        })
    }

    /// The watch row's click: flip the seat's rung — a root seat flips the
    /// whole tree — persist, tell the reader what the answer now is, and,
    /// turning the watch on, walk the tree now rather than at the next
    /// focus: a promise made is a promise kept from the moment it is made.
    pub(super) fn toggle_watch(&mut self, shelf_id: &str) -> Task<Message> {
        let Some(watch) = self.shelf_watch(shelf_id) else {
            return Task::none();
        };
        let on = !watch.on;
        let Some((root, opts)) = folder_ops::find_mut(&mut self.library.folders, &watch.folder_id)
            .map(|folder| {
                if watch.rung.is_empty() {
                    folder.set_tracking_whole(on);
                } else {
                    folder.set_tracking(&watch.rung, on);
                }
                (folder.root.clone(), folder.opts.clone())
            })
        else {
            return Task::none();
        };
        let ground = match &watch.rung_label {
            Some(rung) => format!("“{rung}” in {}", watch.label),
            None => watch.label,
        };
        let persisted = self.persist_library();
        let line = if on {
            format!("Watching {ground} for new books.")
        } else {
            format!("{ground} is no longer watched for new books.")
        };
        self.toasts.show(Tone::Info, line, Instant::now());
        if on {
            Task::batch([
                persisted,
                self.begin_folder_walk(PathBuf::from(root), opts, Asked::OnFocus, RootPlan::default()),
            ])
        } else {
            persisted
        }
    }

    /// The one write a reveal is: what to light, and a new beat of its own
    /// — a second reveal of the same thing is a second reveal, so the
    /// clock the flash dies on starts over.
    fn light(&mut self, id: &str) {
        let nonce = match &self.reveal {
            Some((revealed, _)) => revealed.nonce + 1,
            None => 1,
        };
        self.reveal = Some((Reveal { id: id.to_string(), nonce }, Instant::now()));
    }

    /// The flash's own offset: where the grid or the list owes a card a
    /// light, off the same facts the views read.
    fn reveal_scroll(&self, id: &str) -> Task<Message> {
        let rows = library::level_rows(&self.library, &self.shelf, &self.query);
        let folders = library::level_folders(&self.library, &self.shelf, &self.query);
        let folders_len = folders.len();
        let index = folders
            .iter()
            .position(|each| each.id == id)
            .or_else(|| rows.iter().position(|row| row.id() == id).map(|ix| ix + folders_len));
        let Some(index) = index else {
            return Task::none();
        };
        let y = if self.library.view.is_list() {
            reveal::list_offset(index, self.shelf_viewport_h)
        } else {
            let (tracks, cell) =
                library::content_metrics(self.viewport.width, self.library.view.columns);
            reveal::grid_offset(index, tracks, cell, self.shelf_viewport_h)
        };
        operation::scroll_to(LIBRARY_SCROLL, scrollable::AbsoluteOffset { x: None, y: Some(y) })
    }

    /// The web's full reveal: navigate, scroll to the card, and light the
    /// answer where the landing put it — the web page called it the
    /// scroll-into-view and the flash, and the native floor pays both from
    /// one write.
    pub(super) fn reveal_shelf(&mut self, shelf_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_shelf(&self.library.shelves, shelf_id);
        self.light(shelf_id);
        self.reveal_scroll(shelf_id)
    }

    /// The row half of the same light: the level the card lives on, then
    /// the scroll and the ring — in that order, because the reveal's whole
    /// story is that the way and the light never disagree.
    pub(super) fn reveal_book(&mut self, book_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_row(&self.library.shelves, book_id);
        self.light(book_id);
        self.reveal_scroll(book_id)
    }

    /// "Already imported": the whole of the reveal — no row, and the
    /// reader lands where the answer is. From 3g's own answers onward
    /// every way out of that question ends on the same light.
    pub(super) fn reveal_existing(&mut self, book_id: &str) -> Task<Message> {
        self.reveal_book(book_id)
    }
}
