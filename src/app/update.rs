//! The dispatcher: one match over the messages, routing each to the subject
//! that answers it.
use std::path::PathBuf;

use iced::time::{Duration, Instant};
use iced::widget::operation;
use iced::{window, Task};
use library_core::book::{self};
use library_core::conflict::Placement;
use library_core::shelf::{self, ALL_SHELF};
use reader_core::appearance::BaseMode;

use crate::chrome::titlebar::{self};
use crate::library::conflicts::{self};
use crate::library::departure::{CopyAnswer, CopyWork};
use crate::library::{self, bar};
use crate::platform::{dialogs, fs};
use crate::route::Route;
use crate::ui::toast::Tone;
use crate::{reader, storage};
use super::message::{ContextRequest, MenuKind, Message};
use super::selection::{Family, Hot, Press};
use super::sheets::{RenameKind, Sheet};
use super::walk::{Asked, RootPlan};
use super::{FLASH_DWELL, Mareader};

/// The fold panel's close waits out a diagonal crossing of its corner.
const ELLIPSIS_GRACE_MS: u64 = 220;

impl Mareader {
    #[allow(clippy::too_many_lines)]
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        match message {
            Message::WindowDiscovered(id) => {
                if self.window.is_none() {
                    self.window = id;
                }
                // Ask the window for the scale factor it opened with; the
                // grid snaps against it from the first frame. The boot also
                // owes the library its two automatic measurements — the
                // focus half arrives as its own event, and the run claims
                // keep the two from walking twice.
                let scale = match id {
                    Some(id) => window::scale_factor(id).map(Message::ScaleFactor),
                    None => Task::none(),
                };
                Task::batch([scale, self.start_measure_pass()])
            }
            Message::WindowEvent(id, event) => self.window_event(id, event),
            // The reading surface answers everything itself except where the
            // app's own route is concerned: `Close` is the reader letting the
            // document go, and standing back on the shelf is the app's answer
            // to it.
            Message::Reader(reader::Message::Close) => {
                let effects = self.reader.update(reader::Message::Close, now);
                self.route = Route::Library;
                self.menu = None;
                self.apply_reader_effects(effects)
            }
            Message::Reader(message) => {
                let effects = self.reader.update(message, now);
                self.apply_reader_effects(effects)
            }
            Message::Maximized(maximized) => {
                self.maximized = maximized;
                Task::none()
            }
            Message::ScaleFactor(factor) => {
                self.set_scale_factor(factor);
                Task::none()
            }
            Message::Cursor(position) => {
                if let Some(position) = position {
                    self.cursor = position;
                }
                self.titlebar.on_cursor(position, self.route, now);
                Task::none()
            }
            Message::Tick(at) => {
                self.titlebar.on_tick(self.route, at);
                self.toasts.on_tick(at);
                self.on_hold_tick(at);
                self.on_drag_tick(at);
                // The reveal's light stands for a beat on the level the
                // answer opened on, then it wears off on its own clock.
                let reveal_expired = matches!(
                    &self.reveal,
                    Some((_, started)) if at.duration_since(*started) >= FLASH_DWELL
                );
                if reveal_expired {
                    self.reveal = None;
                }
                // The fold panel's close waits out its grace; a pointer
                // that came back cancelled it by clearing the deadline.
                if let Some(due) = self.ellipsis_close_at
                    && at >= due
                {
                    self.ellipsis_close_at = None;
                    self.ellipsis_open = false;
                }
                // A zoom in flight is driven by the app's own frames rather
                // than a clock of the reader's: the subscription is alive
                // exactly while one is — `needs_tick` — so a still reader costs
                // no redraws at all, and a moving one moves with the display's
                // refresh rate.
                let effects = self.reader.update(reader::Message::Tick, at);
                self.apply_reader_effects(effects)
            }
            Message::ShelfViewport(viewport) => {
                self.shelf_viewport_h = viewport.bounds().height;
                Task::none()
            }
            Message::Chrome(titlebar::Message::TogglePin) => {
                self.titlebar.toggle_pin(self.route, now);
                Task::none()
            }
            Message::Chrome(titlebar::Message::Window(action)) => {
                let Some(id) = self.window else { return Task::none() };
                match action {
                    titlebar::WindowAction::Drag => window::drag(id),
                    titlebar::WindowAction::Minimize => window::minimize(id, true),
                    // Query the answer back rather than tracking toggles by
                    // faith: the glyph swap reads what the window says.
                    titlebar::WindowAction::ToggleMaximize => Task::batch([
                        window::toggle_maximize(id),
                        window::is_maximized(id).map(Message::Maximized),
                    ]),
                    titlebar::WindowAction::Close => window::close(id),
                }
            }
            Message::Navigate(shelf) => {
                self.navigate_to(shelf);
                Task::none()
            }
            Message::Query(terms) => {
                self.menu = None;
                self.query = terms;
                Task::none()
            }
            Message::ToggleMenu(kind) => {
                self.menu = if self.menu == Some(kind) { None } else { Some(kind) };
                self.renaming = false;
                self.menu_confirm = None;
                // The add menu's Restore section promises files, and a
                // promise is checked before it is kept: the gone check
                // rides along with the panel opening.
                if self.menu == Some(MenuKind::Add) {
                    return self.check_restore_paths();
                }
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                self.menu_confirm = None;
                self.renaming = false;
                Task::none()
            }
            Message::EscapePressed => {
                // Escape closes the topmost thing: a drag, then a sheet,
                // then a right-click menu, then a bar panel, then a
                // rename. Cancelling a drag is the web cancel: the payload
                // lands nowhere and the choice that lifted it stays
                // exactly as it was.
                if self.drag.is_some() {
                    self.drag = None;
                    self.close_ellipsis();
                    return Task::none();
                }
                if self.sheet.is_some() {
                    self.dismiss_sheet();
                    self.advance_conflict();
                    return Task::none();
                }
                if self.context.is_some() {
                    self.context = None;
                    return Task::none();
                }
                if self.ellipsis_open {
                    self.close_ellipsis();
                    return Task::none();
                }
                if self.renaming {
                    self.renaming = false;
                    return Task::none();
                }
                if self.selecting {
                    self.exit_selection();
                    return Task::none();
                }
                self.menu = None;
                self.menu_confirm = None;
                Task::none()
            }
            Message::StartRename => {
                self.menu = None;
                self.rename_draft = shelf::find(&self.library.shelves, &self.shelf)
                    .map(|shelf| shelf.name.clone())
                    .unwrap_or_default();
                self.renaming = true;
                // The field appears in the next view; focus lands with it.
                operation::focus(bar::RENAME_INPUT)
            }
            Message::RenameDraft(text) => {
                self.rename_draft = text;
                Task::none()
            }
            Message::CommitRename => {
                self.renaming = false;
                let name = self.rename_draft.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &self.shelf) {
                    shelf.name = name;
                }
                self.persist_library()
            }
            Message::RemoveShelf => {
                let id = self.shelf.clone();
                self.remove_shelf_with(&id)
            }
            Message::SelectRow(id) => {
                self.context = None;
                self.enter_selection(&id);
                Task::none()
            }
            Message::DuplicateRow(id) | Message::DuplicateShelf(id) => {
                // The menus close with the ask, the web's own order: the
                // run's card is what the reader watches next.
                self.menu = None;
                self.context = None;
                self.dup_queue.push(id);
                self.pump_dup()
            }
            Message::DuplicateSelection => {
                self.context = None;
                self.dup_queue.extend(self.selected.iter().cloned());
                self.pump_dup()
            }
            Message::ContextMenu(target) => {
                // A drag in flight owns the pointer; a right-click during
                // it is the cancel's business, not a menu's.
                if self.drag.is_some() {
                    return Task::none();
                }
                self.menu = None;
                self.menu_confirm = None;
                self.context = Some(ContextRequest { target, at: self.cursor });
                Task::none()
            }
            Message::RevealRow(id) => {
                self.context = None;
                let path = book::find_row(&self.library.books, &id)
                    .and_then(|row| match row {
                        book::Row::Book(book) => Some(book.path().to_string()),
                        book::Row::Link { .. } => None,
                    });
                match path {
                    Some(address) => {
                        if let Err(error) = fs::reveal(&address) {
                            self.toasts.show(Tone::Error, error, Instant::now());
                        }
                    }
                    None => self.toasts.show(
                        Tone::Info,
                        "A link has no place on disk to show",
                        Instant::now(),
                    ),
                }
                Task::none()
            }
            Message::AskRenameRow(id) => self.ask_rename(RenameKind::Row, id),
            Message::AskRenameShelf(id) => self.ask_rename(RenameKind::Shelf, id),
            Message::AskRemoveRow(id) => {
                self.context = None;
                let name = book::find_row(&self.library.books, &id)
                    .map(|row| match row {
                        book::Row::Book(book) => book.title(),
                        book::Row::Link { name, .. } => name.clone(),
                    })
                    .unwrap_or_default();
                self.sheet = Some(Sheet::Remove { id, name });
                Task::none()
            }
            Message::NewShelfInside(parent_id) => self.create_shelf_in(Some(parent_id)),
            Message::TakeApart(id) => {
                self.context = None;
                self.menu = None;
                self.ask_shelf_apart(&id)
            }
            Message::SheetDraft(text) => {
                if let Some(Sheet::Rename { draft, .. }) = &mut self.sheet {
                    *draft = text;
                }
                Task::none()
            }
            Message::SheetCancel => {
                if let Some(Sheet::AlreadyImported { note }) = self.sheet.take() {
                    self.advance_conflict();
                    return self.reveal_shelf(&note.shelf_id);
                }
                self.dismiss_sheet();
                self.advance_conflict();
                Task::none()
            }
            Message::CloseAlreadyImported => {
                let Some(Sheet::AlreadyImported { note }) = self.sheet.take() else {
                    return Task::none();
                };
                self.advance_conflict();
                self.reveal_shelf(&note.shelf_id)
            }
            Message::AnswerCopy(answer) => {
                let Some(Sheet::Copy { ask }) = self.sheet.take() else {
                    return Task::none();
                };
                match answer {
                    // Nothing moves and nothing copies. A queue behind the
                    // copy sheet shows now.
                    CopyAnswer::Cancel => {
                        self.advance_conflict();
                        Task::none()
                    }
                    // The answer with no copies in it: a shelf with a way
                    // home takes it — the fold or the reseat its folder
                    // names — a removal runs as it always did, because the
                    // sheet's second button promised the folder would make
                    // its level again, and the rows' and the rung's doors
                    // have nothing to do without their copies: the gesture
                    // stays where the folder's tree put it.
                    CopyAnswer::WithoutCopies => {
                        let task = match ask.work {
                            CopyWork::Shelf { returns, .. } => {
                                if self.take_them_home(&returns) {
                                    self.persist_library()
                                } else {
                                    Task::none()
                                }
                            }
                            CopyWork::Removal { purge, shelves } => {
                                if self.remove(purge, shelves) {
                                    self.persist_library()
                                } else {
                                    Task::none()
                                }
                            }
                            CopyWork::Rows { .. } | CopyWork::Rung { .. } => Task::none(),
                        };
                        self.advance_conflict();
                        task
                    }
                    CopyAnswer::Copy => self.copy_and_finish(ask),
                }
            }
            Message::AnswerPlacement(choice, all) => {
                let Some(Sheet::Conflict { mut ask }) = self.sheet.take() else {
                    return Task::none();
                };
                let mut tasks: Vec<Task<Message>> = Vec::new();
                loop {
                    // A move whose arrival names no row has nothing to
                    // write: the row went while the sheet was up.
                    if ask.arrival.moving.is_some() || ask.arrival.is_import() {
                        let task = match &ask.kind {
                            conflicts::AskKind::FolderMerge { .. } => {
                                self.apply_folder_merge(&ask, choice)
                            }
                            _ => self.apply_placement(&ask, choice),
                        };
                        tasks.push(task);
                    }
                    self.advance_conflict();
                    if !all {
                        break;
                    }
                    // The switch's contract: the same answer goes to every
                    // waiting question of this sheet's kind, and a question
                    // of another kind keeps its own sheet and its own
                    // answers.
                    let Some(Sheet::Conflict { ask: next }) = self.sheet.take() else {
                        break;
                    };
                    let same = (ask.kind.is_two_answer() && next.kind.is_two_answer())
                        || (ask.kind.is_folder_merge() && next.kind.is_folder_merge());
                    if !same {
                        self.sheet = Some(Sheet::Conflict { ask: next });
                        break;
                    }
                    ask = next;
                }
                match tasks.len() {
                    0 => Task::none(),
                    1 => tasks.pop().unwrap_or_else(Task::none),
                    _ => Task::batch(tasks),
                }
            }
            Message::AnswerShelf(placement) => {
                let Some(Sheet::ShelfConflict { ask }) = self.sheet.take() else {
                    return Task::none();
                };
                if !conflicts::shelf_offers(&ask).contains(&placement) {
                    return Task::none();
                }
                match placement {
                    Placement::Open => self.reveal_shelf(&ask.existing_id),
                    Placement::LinkOnly => self.link_to_existing(&ask),
                    Placement::Merge => self.begin_folder_walk(
                        PathBuf::from(&ask.root),
                        ask.opts.clone(),
                        Asked::Explicitly,
                        RootPlan { rename: None, into: Some(ask.existing_id.clone()), ..RootPlan::default() },
                    ),
                    Placement::KeepBoth => self.copies_beside_tree_run(ask),
                    Placement::Replace => self.replace_with_tree(ask),
                }
            }
            Message::ToggleApplyAll => {
                self.apply_all = !self.apply_all;
                Task::none()
            }
            Message::SheetSave => self.save_sheet(),
            Message::SheetImportFormat(format) => {
                if !self.import_opts.formats.remove(&format) {
                    self.import_opts.formats.insert(format);
                }
                Task::none()
            }
            Message::SheetImportInclude(include) => {
                self.import_opts.include_selected = include;
                Task::none()
            }
            Message::SheetImportSize(delta) => {
                self.import_opts.step_min_size(delta);
                Task::none()
            }
            Message::SheetImportMode(mode) => {
                // One click writes both switches from the mode picked, so
                // the unofferable pair (a watching copy) is never one the
                // sheet can hold.
                self.import_opts.in_place = mode.reads_in_place();
                self.import_opts.watch = mode.tracks_new_files();
                Task::none()
            }
            Message::SheetImportGroups(groups) => {
                self.import_opts.groups = groups;
                Task::none()
            }
            Message::SetLayout(layout) => {
                if self.library.view.layout == layout {
                    self.menu = None;
                    return Task::none();
                }
                self.library.view.layout = layout;
                self.view_changed()
            }
            Message::SetCover(cover) => {
                if self.library.view.cover == cover {
                    self.menu = None;
                    return Task::none();
                }
                self.library.view.cover = cover;
                self.view_changed()
            }
            Message::SetSort(key) => {
                if self.library.view.sort == key {
                    self.menu = None;
                    return Task::none();
                }
                self.library.view.sort = key;
                self.view_changed()
            }
            Message::SetSortAsc(ascending) => {
                if self.library.view.sort_asc == ascending {
                    self.menu = None;
                    return Task::none();
                }
                self.library.view.sort_asc = ascending;
                self.view_changed()
            }
            Message::StepColumns(delta) => {
                self.library.view.step_columns(delta);
                self.view_changed()
            }
            Message::AutoColumns => {
                if self.library.view.columns.is_none() {
                    self.menu = None;
                    return Task::none();
                }
                self.library.view.auto_columns();
                self.view_changed()
            }
            Message::CreateShelf => self.create_shelf(),
            Message::Reload => {
                self.menu = None;
                self.exit_selection();
                self.library = storage::load_library();
                if self.shelf != ALL_SHELF
                    && shelf::find(&self.library.shelves, &self.shelf).is_none()
                {
                    self.shelf = ALL_SHELF.to_string();
                }
                self.toasts.show(Tone::Info, "Reloaded the library from disk", now);
                Task::none()
            }
            Message::OpenBook(id) => {
                self.menu = None;
                self.context = None;
                self.exit_selection();
                let Some(path) = book::find_by_id(&self.library.books, &id)
                    .map(|book| PathBuf::from(book.path()))
                else {
                    return Task::none();
                };
                // The id rides along: a book of its own resumes where its own
                // reader left off, and the menu row the reader pressed named
                // which book they meant.
                self.open_row(Some(id), path)
            }
            Message::CardHover(hovered) => {
                if let Some(id) = &hovered {
                    self.on_hot_change(Some(Hot::Card(id.clone())), now);
                    self.hovered_crumb = None;
                } else {
                    self.on_hot_exit(Family::Card, now);
                }
                self.hovered_card = hovered;
                Task::none()
            }
            Message::DragBand(id, band) => {
                // The cell's sensor read a finer truth than its hover: the
                // same target, but the band the commit resolves its index
                // from. It arrives after the cell's own CardHover in the
                // same event, so the band it names is the band that stays.
                self.on_hot_change(Some(Hot::Card(id.clone())), now);
                self.hovered_crumb = None;
                self.hovered_card = Some(id);
                if let Some(drag) = &mut self.drag {
                    drag.band = band;
                }
                Task::none()
            }
            Message::CrumbHover(hovered) => {
                if let Some(id) = &hovered {
                    self.on_hot_change(Some(Hot::Crumb(id.clone())), now);
                    self.hovered_card = None;
                } else {
                    self.on_hot_exit(Family::Crumb, now);
                }
                self.hovered_crumb = hovered;
                Task::none()
            }
            Message::EllipsisHover(over) => {
                // The panel opens one beat behind the pointer: the enter
                // opens it, the leave arms the grace, and a return inside
                // the grace cancels the close by clearing the deadline.
                if over {
                    self.ellipsis_open = true;
                    self.ellipsis_close_at = None;
                    self.hovered_crumb = None;
                    self.hovered_card = None;
                    self.on_hot_change(Some(Hot::Ellipsis), now);
                } else {
                    // A leave that lands inside the panel's box is the
                    // ellipsis's exit crossing the panel's enter in the
                    // queue: the pointer never left the intent, so no
                    // close is armed.
                    if !self.pointer_in_panel() {
                        self.ellipsis_close_at =
                            Some(now + Duration::from_millis(ELLIPSIS_GRACE_MS));
                    }
                    self.on_hot_exit(Family::Ellipsis, now);
                }
                Task::none()
            }
            Message::EllipsisPressed => {
                self.ellipsis_open = true;
                self.ellipsis_close_at = None;
                Task::none()
            }
            Message::PanelCrumb(id) => {
                self.navigate_to(id);
                Task::none()
            }
            Message::CardTap(id) => self.card_tap(&id),
            Message::PressStarted => {
                // The next press owns the swallow flag: clearing it here
                // rather than at the release makes the machine's answer
                // independent of the order widget and listener messages
                // queue in.
                self.tap_swallow = None;
                // The hold machine starts on the shelf alone, on a cell
                // alone, and with nothing covering the level.
                if self.route == Route::Library
                    && self.sheet.is_none()
                    && self.menu.is_none()
                    && self.context.is_none()
                    && let Some(id) = self.hovered_card.clone()
                {
                    self.press = Some(Press { id, at: self.cursor, started: now });
                }
                Task::none()
            }
            Message::PressEnded => {
                self.press = None;
                if self.drag.is_some() {
                    return self.release_drag();
                }
                Task::none()
            }
            Message::FloorPressed => {
                // A drag owns this release: the floor must not read the
                // drop's landing as a press on empty ground.
                if self.drag.is_some() {
                    return Task::none();
                }
                self.exit_selection();
                self.context = None;
                Task::none()
            }
            Message::SelectAll => {
                self.context = None;
                let folders = library::level_folders(&self.library, &self.shelf, &self.query);
                let rows = library::level_rows(&self.library, &self.shelf, &self.query);
                self.selecting = true;
                self.selected.extend(folders.into_iter().map(|shelf| shelf.id));
                self.selected.extend(rows.iter().map(|row| row.id().to_string()));
                Task::none()
            }
            Message::ToggleSelectPop => {
                self.select_pop = !self.select_pop;
                Task::none()
            }
            Message::FileSelection(shelf_id) => self.file_selection_to(Some(shelf_id)),
            Message::FileSelectionOnNewShelf => self.file_selection_to(None),
            Message::AskRemoveSelection => {
                self.context = None;
                let (books, shelves) = self.split_selection();
                if books.is_empty() && shelves.is_empty() {
                    return Task::none();
                }
                self.exit_selection();
                self.sheet = Some(Sheet::RemoveMany { books, shelves });
                Task::none()
            }
            Message::ClearSelection => {
                self.context = None;
                self.exit_selection();
                Task::none()
            }
            Message::EnterPressed => {
                if self.route != Route::Library
                    || self.sheet.is_some()
                    || self.context.is_some()
                    || self.menu.is_some()
                {
                    return Task::none();
                }
                let Some(id) = self.hovered_card.clone() else { return Task::none() };
                if self.selecting {
                    self.toggle_selected(&id);
                    Task::none()
                } else {
                    self.card_tap(&id)
                }
            }
            Message::ShiftEnter => {
                if self.route != Route::Library
                    || self.selecting
                    || self.sheet.is_some()
                    || self.context.is_some()
                    || self.menu.is_some()
                {
                    return Task::none();
                }
                let Some(id) = self.hovered_card.clone() else { return Task::none() };
                self.enter_selection(&id);
                Task::none()
            }
            Message::KeepSelection => Task::none(),
            Message::PickFiles => {
                self.menu = None;
                dialogs::pick_files(Message::FilesPicked)
            }
            Message::FilesPicked(picked) => self.import_files(picked),
            Message::PickFolder => {
                self.menu = None;
                dialogs::pick_folder(Message::FolderPicked)
            }
            Message::FolderPicked(picked) => match picked {
                // The walk waits on the sheet: what the import is allowed
                // to be is an answer the reader gives first.
                Some(dir) => {
                    self.open_import_sheet(dir);
                    Task::none()
                }
                None => Task::none(),
            },
            Message::ScanDone(task, result) => self.scan_done(task, result, now),
            Message::CopiesScanned(task, result) => self.copies_scanned(task, result, now),
            Message::CopiesDone(task, results) => self.copies_done(task, results),
            Message::FilesChecked(task, checks) => self.files_checked(task, checks),
            Message::FilesCopied(task, results) => self.files_copied(task, results),
            Message::ChecksDone(checks) => self.checks_done(checks),
            Message::ToggleWatch(shelf_id) => self.toggle_watch(&shelf_id),
            Message::PickFilesInFolder(root) => {
                self.menu = None;
                dialogs::pick_files_in(root, Message::FilesPicked)
            }
            Message::RestoreChecked(checks) => {
                // The answer belongs to the panel that asked: a menu
                // closed while the check was in flight ignores it.
                if self.menu == Some(MenuKind::Add) {
                    self.restore_gone = checks
                        .iter()
                        .filter(|check| !check.exists)
                        .map(|check| check.path.clone())
                        .collect();
                }
                Task::none()
            }
            Message::RestoreDeleted(folder_id, fp) => self.restore_deleted(folder_id, fp),
            Message::RestoreMeasured(task, checks) => self.restore_measured(task, checks),
            Message::RestoreCopied(task, results) => self.restore_copied(task, results),
            Message::ConfirmMoved(ask) => {
                self.menu_confirm = Some(ask);
                Task::none()
            }
            Message::AlsoShow(book_id, shelf_id) => self.also_show(&book_id, &shelf_id),
            Message::GoAndLook(book_id) => self.go_and_look(&book_id),
            Message::MenuBack => {
                self.menu_confirm = None;
                Task::none()
            }
            Message::ImportProgress(beat) => {
                if let Some(run) =
                    self.runs.iter_mut().find(|run| run.task.to_string() == beat.task)
                {
                    run.latest = Some(beat);
                }
                Task::none()
            }
            Message::CycleAppearance => {
                self.settings.appearance.base = match self.settings.appearance.base {
                    BaseMode::Light => BaseMode::Dark,
                    BaseMode::Dark => BaseMode::Dim,
                    BaseMode::Dim => BaseMode::Light,
                };
                // The live look moved, so no saved preset matches it any
                // more; the re-select-when-it-matches rule lands with the
                // appearance system that owns the presets.
                self.settings.active_preset = None;
                self.apply_appearance();
                self.persist_settings()
            }
        }
    }
}
