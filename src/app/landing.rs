//! The answers landing: how a placement, a link, a merge or a restore
//! becomes rows, ledger rows and stored bytes.
use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use iced::time::Instant;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::conflict::Placement;
use library_core::folder::{self as folder_ops};
use library_core::folder::Tombstone;
use library_core::ledger::{self, Recovered};
use library_core::scan::FoundFile;
use library_core::shelf::{self, ALL_SHELF};
use library_core::wire::BookFileRequest;
use library_core::{paths, text as lib_text};

use crate::chrome::icons::IconName;
use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::library::departure::{self, RowMove};
use crate::library::menus;
use crate::platform::{now_ms, progress, store};
use crate::ui::toast::Tone;
use super::Mareader;
use super::copies::{CopiesDest, PendingCopy};
use super::message::Message;
use super::moves::{DepartLand, DepartWork};
use super::message::MovedAsk;
use super::walk::{Asked, Claim, FilesPlan, FsRun, RootPlan, Stage};

/// The removed row's second line: when the reader took the book out, and —
/// remembered — how big the promise is.
pub(super) fn removed_sublabel(entry: &Tombstone, stamp: u64) -> String {
    let age = lib_text::human_age(entry.removed_ms, stamp);
    match entry.fp.mtime_ms {
        0 => format!("removed {age}"),
        _ => format!("removed {age} · {}", lib_text::human_size(entry.fp.size)),
    }
}

impl Mareader {
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

    /// One file's ride through the store: a copy, a measurement, a landing
    /// — the answers that promise a library copy of their own ride this, so
    /// a copy that fails leaves the shelf untouched and the ledger
    /// unmarked.
    fn land_stored_copy(
        &mut self,
        file: FoundFile,
        name: Option<String>,
        shelf_id: String,
        index: Option<usize>,
        settle: Option<(String, Fingerprint)>,
    ) -> Task<Message> {
        let stamp = now_ms();
        let book_id = library_core::id::next_id(stamp);
        let label = paths::file_name(&file.path);
        let request = BookFileRequest { from: file.path.clone(), id: book_id.clone() };
        let plan = FilesPlan {
            target: (shelf_id != ALL_SHELF).then_some(shelf_id),
            pending: vec![PendingCopy { book_id, file, title: name }],
            restored: 0,
            asks: Vec::new(),
            // An answer's own landing represents nothing: the question was
            // about this one file.
            represented: Vec::new(),
            settle,
            index,
        };
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink: Arc::clone(&sink),
            rx,
            latest: None,
            stage: Stage::Copying { plan: Box::new(plan) },
        });
        let task_name = task.to_string();
        let requests = vec![request];
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::FilesCopied(task, results),
        )
    }

    /// A linked row at the file's own address: what a read-at-place folder
    /// lands for an answered file, on the shelf and in the slot the gesture
    /// meant, wearing the name the answer minted when it minted one. The
    /// web's kept reading data riding a returning file waits on the marks
    /// store.
    fn land_file(
        &mut self,
        file: &FoundFile,
        name: Option<String>,
        shelf_id: &str,
        index: Option<usize>,
    ) -> String {
        let stamp = now_ms();
        // A row already reading the source address makes this row its own
        // book: independent, with its own marks and place.
        let independent =
            book::book_rows(&self.library.books).any(|each| each.path() == file.path);
        let mut minted = Book::new(
            library_core::id::next_id(stamp),
            file.fp,
            file.admitted_format(),
            Origin::Linked { src: file.path.clone() },
            stamp,
        );
        minted.title = name;
        minted.independent = independent;
        let placed = minted.id.clone();
        self.library.books.push(book::Row::Book(minted));
        // The root has no member list, so the placement write is skipped.
        if shelf_id != ALL_SHELF
            && let Some(home) = shelf::find_mut(&mut self.library.shelves, shelf_id)
        {
            shelf::place(&mut home.books, &placed, index);
        }
        placed
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

    /// Record the placement and spend the removal that was holding the
    /// file out — the two ledger writes a walk makes for its files, made
    /// here because an answer landed this one after a walk raised the
    /// question.
    pub(super) fn settle_ledger(&mut self, folder_id: Option<&str>, fp: Fingerprint) {
        let Some(folder_id) = folder_id else {
            return;
        };
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &fp);
            folder.mark_placed(fp);
        }
    }

    /// The answered file's landing: a read-at-place folder's answer links
    /// the file where the gesture meant it, and a copying folder's answer
    /// rides the single-file copy, settling the folder's ledger when that
    /// copy comes home.
    fn land_answer_file(
        &mut self,
        ask: &ConflictAsk,
        file: FoundFile,
        name: Option<String>,
        index: Option<usize>,
    ) -> Task<Message> {
        let (mode, folder_id) = match &ask.kind {
            conflicts::AskKind::FolderMerge { mode, folder_id } => {
                (*mode, Some(folder_id.as_str()))
            }
            _ => return Task::none(),
        };
        let shelf_id = ask.arrival.shelf_id.clone();
        if mode.reads_in_place() {
            self.land_file(&file, name, &shelf_id, index);
            self.settle_ledger(folder_id, file.fp);
            // The web's cover backfill waits on the engines, documented
            // where the covers land.
            return self.persist_library();
        }
        let settle = folder_id.map(|id| (id.to_string(), file.fp));
        self.land_stored_copy(file, name, shelf_id, index, settle)
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

    /// The sheet already withholds *keep both* from a twin; this is the
    /// write side of the same rule, because apply-to-all can carry an
    /// answer to a question whose sheet never offered it.
    fn withhold_keep_both_from_a_twin(
        &self,
        ask: &ConflictAsk,
        answer: Placement,
    ) -> Placement {
        if answer != Placement::KeepBoth {
            return answer;
        }
        if ask.kind.reads_in_place()
            && let Some(file) = ask.arrival.file.as_ref()
            && self.is_the_same_file(&ask.existing_id, &file.path)
        {
            Placement::Merge
        } else {
            answer
        }
    }

    /// Whether the row the question found reads the very file that is
    /// arriving.
    fn is_the_same_file(&self, existing_id: &str, path: &str) -> bool {
        book::find_by_id(&self.library.books, existing_id).is_some_and(|each| each.path() == path)
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

    /// The whole of a removal that is NOT a sweep: no tombstone, no store
    /// byte. One spelling, because the half of it that is easy to forget is
    /// the expensive one.
    pub(super) fn unlist_row(&mut self, id: &str) {
        book::remove_row(&mut self.library.books, id);
        book::drop_dangling_links(&mut self.library.books);
        shelf::forget_everywhere(&mut self.library.shelves, id);
    }

    /// The row's own byte, once no row reads it — the twin rule's other
    /// half: two rows of one file share an address, and a sweep that forgot
    /// the twin would delete the file the survivor reads. A linked book's
    /// bytes are the reader's and are never touched.
    pub(super) fn sweep_row_bytes(&mut self, book: &Book) {
        let in_use = book::book_rows(&self.library.books)
            .any(|each| each.id != book.id && each.path() == book.path());
        if in_use {
            return;
        }
        if book.origin.is_stored()
            && let Err(error) = store::delete_stored(book.path())
        {
            // The row is gone either way; a byte the host will not release
            // is the log's business, not a second question.
            eprintln!("[library] could not sweep {}: {error}", book.path());
        }
    }

    /// Remove one row, everywhere it is filed, and sweep the byte only it
    /// read — and no tombstone: the conflict sheet's dissolving row is one
    /// whose content stays in the library through the row on the other side
    /// of the question, so a rescan that re-found the file would resolve to
    /// that row, and a tombstone for a fingerprint the library still holds
    /// is noise in the folder's restore menu until the next scan prunes it.
    fn drop_row(&mut self, id: &str) -> bool {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return false;
        };
        let doomed = row.book().cloned();
        self.unlist_row(id);
        if let Some(book) = &doomed {
            self.sweep_row_bytes(book);
        }
        true
    }

    /// The files a folder's moved-out log already answers for: a log bound to
    /// a LIVING row says the file's book is still here — the row a shelf move
    /// or a link-making answer left standing for it — so an import of that
    /// file succeeds by lighting the row up rather than landing a second book
    /// beside it. `scope` narrows the search to one folder's own log; the
    /// loose run asks every folder at once.
    pub(super) fn take_represented(&self, scope: Option<&str>, found: &mut Vec<FoundFile>) -> Vec<String> {
        let mut represented = Vec::new();
        found.retain(|file| {
            let row_id = self
                .library
                .folders
                .iter()
                .filter(|folder| scope.is_none_or(|id| folder.id == id))
                .find_map(|folder| {
                    ledger::find_tombstone(folder, &file.fp)
                        .and_then(|entry| entry.returned_row.clone())
                });
            let Some(row_id) = row_id else {
                return true;
            };
            // A log bound to a row that went is a log whose file is free to
            // land again.
            if book::find_row(&self.library.books, &row_id).is_none() {
                return true;
            }
            represented.push(row_id);
            false
        });
        represented
    }

    /// Spend the removal that was holding this content out of any folder's
    /// walk, and answer with the name it remembered. The match is the
    /// fingerprint, not the address: a file removed from a folder, moved
    /// across the disk and dropped back into the library is the same file
    /// the log was written for.
    pub(super) fn lift_stone_for(&mut self, fp: &Fingerprint) -> Option<String> {
        let owner = self
            .library
            .folders
            .iter()
            .find(|folder| ledger::find_tombstone(folder, fp).is_some())
            .map(|folder| folder.id.clone())?;
        let folder = folder_ops::find_mut(&mut self.library.folders, &owner)?;
        ledger::restore_deleted(folder, fp).and_then(|stone| stone.title)
    }

    /// The add menu's folder-side facts: the in-folder picker's door, the
    /// Restore section's rows, and the confirm face when a Moved row was
    /// asked. The labels are computed here — the menu lays out, the app
    /// knows.
    pub(super) fn add_facts(&self) -> menus::AddFacts {
        let confirm = self.menu_confirm.as_ref().map(|ask| self.confirm_face(ask));
        let mut from_folder = None;
        let mut restore = Vec::new();
        if let Some(folder_id) = self.standing_folder_id()
            && let Some(folder) = folder_ops::find(&self.library.folders, &folder_id)
        {
            from_folder = Some(menus::MenuLine {
                icon: IconName::Drop,
                label: "Choose files from this folder".to_string(),
                sublabel: Some(paths::dir_label(&folder.root)),
                message: Some(Message::PickFilesInFolder(folder.root.clone())),
            });
            let index = ledger::index_by_fp(&self.library.books);
            let stamp = now_ms();
            for item in ledger::recoverables(folder, &index, &self.library.shelves) {
                restore.push(match &item {
                    Recovered::Deleted(entry) => {
                        let gone = self.restore_gone.contains(&entry.last_path);
                        menus::MenuLine {
                            icon: IconName::Undo,
                            label: entry.label(),
                            sublabel: Some(if gone {
                                "not there any more".to_string()
                            } else {
                                removed_sublabel(entry, stamp)
                            }),
                            message: (!gone)
                                .then(|| Message::RestoreDeleted(folder_id.clone(), entry.fp)),
                        }
                    }
                    Recovered::Moved { book_id, title, path, home_shelf } => menus::MenuLine {
                        icon: IconName::Next,
                        label: lib_text::display_or_stem(title.as_deref(), path),
                        sublabel: Some(match home_shelf {
                            Some(name) => format!("now on “{name}”"),
                            None => "in the library, on no shelf".to_string(),
                        }),
                        message: Some(Message::ConfirmMoved(MovedAsk {
                            book_id: book_id.clone(),
                            title: title.clone(),
                            path: path.clone(),
                            home_shelf: home_shelf.clone(),
                        })),
                    },
                });
            }
        }
        menus::AddFacts { from_folder, restore, confirm }
    }

    /// The two-choice face a Moved row swaps the panel into: show the book
    /// here as well — one book, two shelves, nothing copied — or close the
    /// menu and go look at where it went.
    fn confirm_face(&self, ask: &MovedAsk) -> menus::ConfirmFace {
        let label = lib_text::display_or_stem(ask.title.as_deref(), &ask.path);
        let go_label = match &ask.home_shelf {
            Some(name) => format!("Show it in “{name}”"),
            None => "Show it in Home".to_string(),
        };
        menus::ConfirmFace {
            back: menus::MenuLine {
                icon: IconName::Prev,
                label: "Back".to_string(),
                sublabel: None,
                message: Some(Message::MenuBack),
            },
            question: format!("“{label}” is on another shelf."),
            also: menus::MenuLine {
                icon: IconName::Plus,
                label: "Also show it here".to_string(),
                sublabel: Some("One book, two shelves — nothing is copied".to_string()),
                // The offer is this level's: standing on the library's own
                // floor, there is no "here" to also show the book in.
                message: (self.shelf != ALL_SHELF)
                    .then(|| Message::AlsoShow(ask.book_id.clone(), self.shelf.clone())),
            },
            go: menus::MenuLine {
                icon: IconName::Next,
                label: go_label,
                sublabel: Some("Closes this menu and takes you to it".to_string()),
                message: Some(Message::GoAndLook(ask.book_id.clone())),
            },
        }
    }

    /// One book, two memberships, nothing copied: the folder's ledger is
    /// untouched — the book stays placed where it was placed, which keeps
    /// the next rescan quiet about it. The level's name screen rides the
    /// filing's own gate, as it does for every second membership.
    pub(super) fn also_show(&mut self, book_id: &str, shelf_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        let one = [book_id.to_string()];
        if self.gated_file(&one, shelf_id) {
            return self.persist_library();
        }
        Task::none()
    }

    /// The confirm face's second answer: close the menu and take the
    /// reader to the shelf the book is on — the first membership in shelf
    /// order, or the library's own floor when it is on none.
    pub(super) fn go_and_look(&mut self, book_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        self.context = None;
        self.shelf = shelf::containing(&self.library.shelves, book_id)
            .first()
            .map(|shelf| shelf.id.clone())
            .unwrap_or_else(|| ALL_SHELF.to_string());
        Task::none()
    }
}
