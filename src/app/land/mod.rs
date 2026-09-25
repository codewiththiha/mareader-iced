//! Landing a folder walk: the rungs it claimed become shelves, the files it found
//! become rows, and the ledger keeps its account of both.

use crate::app::Mareader;
use crate::app::copies::CopyMap;
use crate::app::message::Message;
use crate::app::sheets::{LitNote, Sheet};
use crate::app::walk::{Asked, WalkPlan, chain_for, write_folder_row};
use crate::library::conflicts;
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Origin};
use library_core::ledger;
use library_core::shelf::{self, ALL_SHELF};
use library_core::{text as lib_text};

mod loose;

impl Mareader {
    /// The diff's answer, written to the live lists: relinks first, then
    /// the mints — each wearing the shelf its rung names, minting the whole
    /// chain between the folder's root and the file's own subfolder — then
    /// the rehang the fresh map owes, the row written back whole, and the
    /// news the reader is told.
    #[allow(clippy::too_many_lines)]
    pub(super) fn land_folder_walk(
        &mut self,
        plan: WalkPlan,
        copies: Option<CopyMap>,
    ) -> Task<Message> {
        let WalkPlan {
            mut folder,
            asked,
            continuation,
            root_name,
            planned_root,
            adds,
            relinks,
            asks,
            replacements,
            copy_paths,
            represented,
        } = plan;
        let stamp = now_ms();
        let mut placed = 0usize;
        let mut relinked = 0usize;
        let mut healed = 0usize;
        let mut refused = 0usize;

        for (book_id, to) in relinks {
            if ledger::relink(&mut self.library.books, &book_id, &to) {
                relinked += 1;
            }
        }

        let mut new_shelves: Vec<shelf::Shelf> = Vec::new();
        let mut placements: Vec<(String, String)> = Vec::new();
        let root = folder.root.clone();
        let folder_id = folder.id.clone();

        for (book_id, file) in adds {
            // A file this run owes a copy of is the library's second instance
            // beside the row that already reads the address: never a heal.
            let own_copy = copy_paths.contains(&file.path);
            // A file at an address the library already reads is that book,
            // whatever the two fingerprints say: a migrated row's
            // placeholder identity is healed by the ground it stands on.
            if !own_copy
                && let Some(existing) =
                    book::book_rows_mut(&mut self.library.books).find(|b| b.path() == file.path)
            {
                existing.heal(file.fp);
                folder.mark_placed(file.fp);
                healed += 1;
                continue;
            }
            let origin = match &copies {
                None => Origin::Linked { src: file.path.clone() },
                Some(map) => match map.get(&book_id) {
                    Some((store_path, _)) => Origin::Stored {
                        src: Some(file.path.clone()),
                        store: store_path.clone(),
                    },
                    // The store refused this file; the rest of the batch
                    // still lands, and the failure is counted for the toast.
                    None => {
                        refused += 1;
                        continue;
                    }
                },
            };
            // A file that came back is the file that left: the name the
            // removal logged rides home with it.
            let title = ledger::find_tombstone(&folder, &file.fp).and_then(|s| s.title.clone());
            let mut minted =
                Book::new(book_id.clone(), file.fp, file.admitted_format(), origin, stamp);
            minted.title = title;
            if let Some(map) = &copies {
                // The copy's own measurement becomes the row's identity, so
                // the source's fingerprint stays free for the folder that
                // reads it.
                minted.adopt_measurement(map.get(&book_id).and_then(|(_, m)| *m));
            }
            // A copy-list file becomes a book of its own beside the linked
            // book the tree keeps, and so does an in-place file whose address
            // another row already reads as a STORED copy: `add_book`'s
            // one-row-per-fingerprint rule is right for a walk and wrong for
            // the second instance the library just asked for.
            if own_copy {
                minted.independent = true;
            }
            let beside_its_own_copy = copies.is_none()
                && book::book_rows(&self.library.books).any(|b| {
                    !b.independent && b.fp == file.fp && b.origin.is_store_copy_of(&file.path)
                });
            let placed_id = if own_copy || beside_its_own_copy {
                let id = minted.id.clone();
                self.library.books.push(library_core::book::Row::Book(minted));
                id
            } else {
                book::add_book(&mut self.library.books, minted)
            };
            // The whole chain, not the leaf: importing "1" containing "2"
            // and four books has to produce "1" at the root with them
            // inside.
            let key = folder.shelf_key(&file);
            let shelf_id =
                chain_for(&mut folder, &key, stamp, planned_root.as_deref(), &root, &mut new_shelves);
            placements.push((placed_id, shelf_id));
            folder.mark_placed(file.fp);
            // The landing spends the removal that was holding the file out.
            ledger::restore_deleted(&mut folder, &file.fp);
            placed += 1;
        }

        // The rows a planned tree re-seats: the row follows its rung onto the
        // tree the answer named, which is what makes *merge into it* a move
        // rather than a second copy of every book the folder already had.
        for (row_id, file) in replacements {
            let key = folder.shelf_key(&file);
            let shelf_id = chain_for(
                &mut folder,
                &key,
                stamp,
                planned_root.as_deref(),
                &root,
                &mut new_shelves,
            );
            placements.push((row_id, shelf_id));
        }

        // The shelves the mints reported, added unless already standing: a
        // second walk can name a rung the first minted, and the id is what
        // makes a rung the same rung.
        for minted in new_shelves {
            if !self.library.shelves.iter().any(|each| each.id == minted.id) {
                self.library.shelves.push(minted);
            }
        }
        for (book_id, shelf_id) in &placements {
            if let Some(home) = shelf::find_mut(&mut self.library.shelves, shelf_id) {
                shelf::shelf_add(home, book_id);
            }
        }
        // The disk's shape wins over the tree's own hanging, for every rung
        // no hand has moved.
        for (id, want) in shelf::rehang_moves(&self.library.shelves, &folder_id) {
            if let Some(moved) = shelf::find_mut(&mut self.library.shelves, &id) {
                moved.parent = want;
            }
        }

        folder.scanned_ms = stamp;
        write_folder_row(&mut self.library.folders, folder);
        let persist = self.persist_library();

        // The represented rows are books the reader got back without a copy:
        // they belong in the count the import reports.
        let total = placed + relinked + healed + represented.len();
        match asked {
            Asked::Explicitly if total > 0 => self.toasts.show(
                Tone::Info,
                format!("Imported {} from “{root_name}”", lib_text::plural(total, "book", "books")),
                Instant::now(),
            ),
            Asked::OnFocus if placed > 0 => self.toasts.show(
                Tone::Info,
                format!("{} in “{root_name}”", lib_text::plural(placed, "new book", "new books")),
                Instant::now(),
            ),
            _ => {}
        }
        if refused > 0 {
            self.toasts.show(
                Tone::Error,
                format!(
                    "Could not copy {}; the rest of the folder landed",
                    lib_text::plural(refused, "file", "files")
                ),
                Instant::now(),
            );
        }
        // The merge's questions are raised over the landed tree, never
        // beside ghosts. A covered walk's nothing-new note waits on them:
        // a question on screen is answered before the note its ground saw.
        let asks_empty = asks.is_empty();
        if !asks.is_empty() {
            self.raise_conflict(asks);
        }
        // The covered walk's answer to its own pick: nothing new is the
        // note, its close the shelf's light; something new goes to where it
        // stands, beside it.
        if asked == Asked::Explicitly
            && let Some(continuation) = continuation
        {
            if total == 0 && asks_empty {
                self.sheet = Some(Sheet::AlreadyImported {
                    note: LitNote {
                        shelf_id: continuation.shelf_id.clone(),
                        name: continuation.name.clone(),
                        kind: conflicts::NoteKind::NothingNew,
                    },
                });
            } else if total > 0 {
                return Task::batch([persist, self.reveal_shelf(&continuation.shelf_id)]);
            }
        }
        persist
    }

    /// The folder the level on screen belongs to, when it is a watched
    /// folder's shelf — the add menu's in-folder doors answer for it.
    pub(super) fn standing_folder_id(&self) -> Option<String> {
        if self.shelf == ALL_SHELF {
            return None;
        }
        shelf::find(&self.library.shelves, &self.shelf)
            .and_then(|shelf| shelf.kind.folder_id().map(str::to_string))
    }
}
