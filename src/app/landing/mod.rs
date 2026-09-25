//! The conflict's answers: what each placement does to the shelf or the row it
//! names, and the renamed copies a kept-both leaves behind.

use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::library::departure::{self, RowMove};
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::book;
use library_core::conflict::Placement;
use library_core::ledger;
use library_core::paths;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::BookFileRequest;
use std::path::PathBuf;
use super::Mareader;
use super::copies::CopiesDest;
use super::message::Message;
use super::moves::{DepartLand, DepartWork};
use super::walk::{Asked, Claim, RootPlan, Stage};

mod moved;
mod rows;

impl Mareader {
    /// The replace's seat: the row's own move, then the shelves the
    /// displaced one held. `departed` is the replace's own copy of the
    /// gate's answer, and it travels because a departure must not bind the
    /// moved-out log it just wrote.
    pub(super) fn seat_replace(
        &mut self,
        moved_id: &str,
        shelf_id: &str,
        index: Option<usize>,
        inherited: &[String],
        departed: bool,
    ) {
        self.move_row(moved_id, shelf_id, index, departed);
        conflicts::file_on_all(&mut self.library.shelves, moved_id, inherited);
    }

    /// "As new": a moved row is renamed and then moved — the rename is
    /// what frees the collision, and a move that did not rename would ask
    /// the same question again on the way in. An import's file answer rides
    /// the single-file copy: made and measured before the row is promised,
    /// minted under the name the sheet showed.
    pub(super) fn as_new(&mut self, ask: &ConflictAsk) -> Task<Message> {
        let name = conflicts::minted_name(&self.library.books, &self.library.shelves, ask);
        if let Some(row_id) = ask.arrival.moving.clone() {
            conflicts::rename_row(&mut self.library.books, &row_id, &name);
            self.move_row(&row_id, &ask.arrival.shelf_id, ask.arrival.index, false);
            return self.persist_library();
        }
        let Some(file) = ask.arrival.file.clone() else {
            return Task::none();
        };
        self.land_stored_copy(
            file,
            Some(name),
            ask.arrival.shelf_id.clone(),
            ask.arrival.index,
            None,
        )
    }

    /// The *replace* answer: the books the level's shelf holds leave the
    /// library through the removal's own sweep, and the folder's copies
    /// take the shelf. The claim check and the sweep run in one step: a run
    /// already walking the ground refuses before anything is removed.
    pub(super) fn replace_with_tree(&mut self, ask: ShelfConflictAsk) -> Task<Message> {
        let root_str = ask.root.clone();
        if self.root_claim(&root_str) != Claim::Free {
            self.toasts.show(
                Tone::Info,
                format!("{} is already being imported.", paths::dir_label(&root_str)),
                Instant::now(),
            );
            return Task::none();
        }
        let own_in_place = ask.own
            && self
                .library
                .folders
                .iter()
                .any(|f| f.root == root_str && f.mode().reads_in_place());
        if own_in_place {
            // The tree's linked rows go out through the same pass a removal
            // walks: the copies land in the names the shelves showed, and a
            // copy that came home to one of its shelves stays.
            let placed = self
                .library
                .folders
                .iter()
                .find(|f| f.root == root_str && f.mode().reads_in_place())
                .map(|f| f.placed.clone())
                .unwrap_or_default();
            let doomed = if placed.is_empty() {
                Vec::new()
            } else {
                ledger::linked_rows_of(&self.library.books, &placed)
            };
            for id in doomed {
                self.purge_row(&id);
            }
            return self.begin_folder_walk(
                PathBuf::from(&root_str),
                ask.opts.clone(),
                Asked::Explicitly,
                RootPlan::default(),
            );
        }
        let doomed: Vec<String> =
            shelf::members_of(&self.library.books, &self.library.shelves, &ask.existing_id)
                .into_iter()
                .map(str::to_string)
                .collect();
        for id in doomed {
            self.purge_row(&id);
        }
        if ask.opts.mode().copies_files()
            && self.library.folders.iter().any(|f| f.root == root_str && f.mode().reads_in_place())
        {
            // The unbound run files the copies into the shelf the sweep
            // just emptied and leaves the tree's ledger alone.
            return self.begin_copies_run(
                PathBuf::from(&root_str),
                ask.opts.clone(),
                CopiesDest::Into { shelf_id: ask.existing_id.clone() },
            );
        }
        self.begin_folder_walk(
            PathBuf::from(&root_str),
            ask.opts.clone(),
            Asked::Explicitly,
            RootPlan { rename: None, into: Some(ask.existing_id.clone()), ..RootPlan::default() },
        )
    }

    /// One spelling for the answers that leave a link behind: the name is
    /// the whole of what makes the row recognisable beside the book it
    /// points at, and an empty one means the target went while the sheet
    /// was up — the arrival's own name stands in.
    fn add_link_at_target(&mut self, ask: &ConflictAsk, target: &str) -> bool {
        let name = book::find_row(&self.library.books, target)
            .map(|row| row.display_name())
            .unwrap_or_default();
        let name = if name.trim().is_empty() { ask.arrival.name.clone() } else { name };
        let now = now_ms();
        let link_id = library_core::id::next_id(now);
        self.library.books.push(book::Row::link(
            link_id.clone(),
            name,
            target.to_string(),
            now,
        ));
        if ask.arrival.shelf_id != ALL_SHELF
            && let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &ask.arrival.shelf_id)
        {
            shelf::shelf_add(shelf, &link_id);
        }
        true
    }

    /// The compact per-file answers: keep the row that is here, seat this
    /// file in its place, or keep both under a name of its own — *go and
    /// look* is not among them, because the reader is importing the folder,
    /// so going to look is not an answer to a file inside it.
    pub(super) fn apply_folder_merge(&mut self, ask: &ConflictAsk, answer: Placement) -> Task<Message> {
        let answer = self.withhold_keep_both_from_a_twin(ask, answer);
        // Every answer this sheet offers is about one arriving file.
        let Some(file) = ask.arrival.file.clone() else {
            return Task::none();
        };
        match answer {
            Placement::KeepBoth => {
                let name =
                    conflicts::minted_name(&self.library.books, &self.library.shelves, ask);
                self.land_answer_file(ask, file, Some(name), None)
            }
            Placement::Replace => {
                let slot = conflicts::member_slot(
                    &self.library.shelves,
                    &ask.arrival.shelf_id,
                    &ask.existing_id,
                );
                self.purge_row(&ask.existing_id);
                self.land_answer_file(ask, file, None, slot)
            }
            // The measurement only travels with the answer when the
            // arriving file IS the row's file, which a re-import of one
            // folder always is. A different folder's namesake is another
            // content wearing one name.
            Placement::Merge => {
                if self.is_the_same_file(&ask.existing_id, &file.path)
                    && let Some(book::Row::Book(existing)) =
                        book::find_row_mut(&mut self.library.books, &ask.existing_id)
                {
                    existing.heal(file.fp);
                }
                self.settle_ledger(ask.kind.folder_id(), file.fp);
                self.persist_library()
            }
            Placement::Open | Placement::LinkOnly => Task::none(),
        }
    }

    /// The pointer answer: one row at the root that says where the folder
    /// already is, named for the level's own truth.
    pub(super) fn link_to_existing(&mut self, ask: &ShelfConflictAsk) -> Task<Message> {
        let stamp = now_ms();
        self.library.books.push(book::Row::Link {
            id: library_core::id::next_id(stamp),
            name: ask.existing_name.clone(),
            target: ask.existing_id.clone(),
            added_ms: stamp,
        });
        self.toasts.show(
            Tone::Info,
            format!("Linked to {}.", ask.existing_name),
            Instant::now(),
        );
        self.persist_library()
    }

    /// The *as new* answer: a fresh named tree of one's own. Ground a tree
    /// still reads is the one case the bound walk cannot take — the copies
    /// are unbound, filed beside the tree rather than into its ledger.
    pub(super) fn copies_beside_tree_run(&mut self, ask: ShelfConflictAsk) -> Task<Message> {
        // The library names the shelf something new, once, at the click the
        // sheet promised: the name the reader does not take on faith.
        let name = library_core::conflict::next_shelf_name(
            &self.library.shelves,
            None,
            &ask.incoming_name,
        );
        let stands_in_place =
            self.library.folders.iter().any(|f| f.root == ask.root && f.mode().reads_in_place());
        if !ask.opts.mode().copies_files() || !stands_in_place {
            return self.begin_folder_walk(
                PathBuf::from(&ask.root),
                ask.opts.clone(),
                Asked::Explicitly,
                RootPlan { rename: Some(name), into: None, ..RootPlan::default() },
            );
        }
        self.begin_copies_run(
            PathBuf::from(&ask.root),
            ask.opts.clone(),
            CopiesDest::NewShelf { name, after: Some(ask.existing_id.clone()) },
        )
    }

    /// "Make link": the row the reader was holding goes, the pointers at it
    /// go with it, and — when the survivor is the library's own copy of that
    /// row's file — the folder that placed it takes a moved-out log naming
    /// the survivor. The link wears the target's own name at this moment,
    /// which is what makes the row recognisable beside the book it points
    /// at.
    pub(super) fn link_to_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
        if let Some(gone_id) = ask.arrival.moving.clone() {
            let gone_book = book::find_by_id(&self.library.books, &gone_id).cloned();
            if let Some(book) = &gone_book
                && conflicts::survivor_is_the_copy_of(&self.library.books, &ask.existing_id, book)
            {
                departure::write_moved_stones(
                    &mut self.library.folders,
                    &self.library.shelves,
                    book,
                    Some(&ask.existing_id),
                    now_ms(),
                );
            }
            self.unlist_row(&gone_id);
        }
        self.add_link_at_target(ask, &ask.existing_id);
        self.persist_library()
    }

    /// "Merge": the survivor is the row the reader can already see here, and
    /// its id is what every shelf holding it and every key in storage
    /// already names, so it is the one that stays. The reader's own data
    /// folds while both rows can still be read; a read-at-place book folding
    /// into the library's own copy of ITS content leaves the folder's file
    /// with no row to answer for it, so the folder takes a moved-out log
    /// bound to the survivor; and the survivor takes over every shelf the
    /// dissolving row held except the level the move left — the departure is
    /// the point of the move.
    pub(super) fn merge_into_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
        let survivor = ask.existing_id.clone();
        let Some(gone_id) = ask.arrival.moving.clone() else {
            return Task::none();
        };
        let gone_book = book::find_by_id(&self.library.books, &gone_id).cloned();
        if let Some(gone) = &gone_book
            && let Some(keep) = book::find_book_mut(&mut self.library.books, &survivor)
        {
            book::fold_books(keep, gone);
        }
        if let Some(gone) = &gone_book
            && conflicts::survivor_is_the_copy_of(&self.library.books, &survivor, gone)
        {
            departure::write_moved_stones(
                &mut self.library.folders,
                &self.library.shelves,
                gone,
                Some(&survivor),
                now_ms(),
            );
        }
        let inherited: Vec<String> = conflicts::memberships(&self.library.shelves, &gone_id)
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| ask.arrival.from.as_deref() != Some(id.as_str()))
            .collect();
        conflicts::file_on_all(&mut self.library.shelves, &survivor, &inherited);
        self.drop_row(&gone_id);
        self.persist_library()
    }

    /// "Replace": the arrival takes the displaced row's SLOT and every OTHER
    /// shelf it was filed on — a replace that quietly took a book off
    /// shelves the question never mentioned is a removal the reader did not
    /// ask for. The displaced row goes through the removal's own sweep,
    /// receipt and all: the sheet's note said what this answer costs. A
    /// read-at-place arrival becomes the library's own copy before it is
    /// seated, and the seating waits for the copy.
    pub(super) fn replace_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
        let Some(moved_id) = ask.arrival.moving.clone() else {
            return Task::none();
        };
        // Read the world before writing any of it: the slot and the
        // memberships are about the row that is going.
        let seat = conflicts::member_slot(
            &self.library.shelves,
            &ask.arrival.shelf_id,
            &ask.existing_id,
        );
        let inherited: Vec<String> =
            conflicts::memberships(&self.library.shelves, &ask.existing_id)
                .into_iter()
                .map(|(id, _)| id)
                .collect();
        self.purge_row(&ask.existing_id);
        let shelf_id = ask.arrival.shelf_id.clone();
        let index = seat.or(ask.arrival.index);
        if departure::converts_on_move(
            &self.library.books,
            &self.library.folders,
            &moved_id,
            &shelf_id,
        ) {
            let (label, request) = match book::find_row(&self.library.books, &moved_id) {
                Some(row) => (
                    row.display_name(),
                    row.book().map(|book| BookFileRequest {
                        from: book.path().to_string(),
                        id: moved_id.clone(),
                    }),
                ),
                None => ("1 book".to_string(), None),
            };
            let requests = request.into_iter().collect();
            let hand = RowMove::Replaced { to: shelf_id, index, inherited };
            let work = DepartWork {
                converting: vec![moved_id.clone()],
                landing: DepartLand::Move { ids: vec![moved_id], hand },
            };
            return self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) });
        }
        self.seat_replace(&moved_id, &shelf_id, index, &inherited, false);
        self.persist_library()
    }
}
