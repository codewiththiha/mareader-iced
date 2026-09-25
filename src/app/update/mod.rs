//! The dispatcher: one arm per message, each handing the work to the subject
//! that answers it.
mod add;
mod bar;
mod reading;
mod rows;
mod selection;
mod sheets;
mod shell;
mod view;

use iced::time::Instant;
use iced::Task;

use crate::app::message::Message;
use crate::app::sheets::RenameKind;
use crate::app::Mareader;
use crate::chrome::titlebar;
use crate::reader;

impl Mareader {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let now = Instant::now();
        match message {
            Message::WindowDiscovered(id) => self.window_found(id),
            Message::WindowEvent(id, event) => self.window_event(id, event),
            Message::Reader(reader::Message::Close) => self.close_document(now),
            Message::Reader(message) => self.reader_message(message, now),
            Message::Maximized(maximized) => self.window_maximized(maximized),
            Message::ScaleFactor(factor) => self.scale_factor_changed(factor),
            Message::Cursor(position) => self.cursor_moved(position, now),
            Message::Tick(at) => self.tick(at),
            Message::ShelfViewport(viewport) => self.shelf_viewport(viewport),
            Message::Chrome(titlebar::Message::TogglePin) => self.chrome_toggle_pin(now),
            Message::Chrome(titlebar::Message::Window(action)) => self.chrome_window_action(action),
            Message::Navigate(shelf) => {
                self.navigate_to(shelf);
                Task::none()
            }
            Message::Query(terms) => self.query_changed(terms),
            Message::ToggleMenu(kind) => self.toggle_menu(kind),
            Message::CloseMenu => self.close_menu(),
            Message::EscapePressed => self.escape_pressed(),
            Message::StartRename => self.start_rename(),
            Message::RenameDraft(text) => self.rename_draft(text),
            Message::CommitRename => self.commit_rename(),
            Message::RemoveShelf => {
                let id = self.shelf.clone();
                self.remove_shelf_with(&id)
            }
            Message::SelectRow(id) => self.select_row(id),
            Message::DuplicateRow(id) | Message::DuplicateShelf(id) => self.duplicate(id),
            Message::DuplicateSelection => self.duplicate_selection(),
            Message::ContextMenu(target) => self.context_menu(target),
            Message::RevealRow(id) => self.reveal_row(id),
            Message::AskRenameRow(id) => self.ask_rename(RenameKind::Row, id),
            Message::AskRenameShelf(id) => self.ask_rename(RenameKind::Shelf, id),
            Message::AskRemoveRow(id) => self.ask_remove_row(id),
            Message::NewShelfInside(parent_id) => self.create_shelf_in(Some(parent_id)),
            Message::TakeApart(id) => self.take_apart(id),
            Message::SheetDraft(text) => self.sheet_draft(text),
            Message::SheetCancel => self.sheet_cancel(),
            Message::CloseAlreadyImported => self.close_already_imported(),
            Message::AnswerCopy(answer) => self.answer_copy(answer),
            Message::AnswerPlacement(choice, all) => self.answer_placement(choice, all),
            Message::AnswerShelf(placement) => self.answer_shelf(placement),
            Message::ToggleApplyAll => self.toggle_apply_all(),
            Message::SheetSave => self.save_sheet(),
            Message::SheetImportFormat(format) => self.sheet_import_format(format),
            Message::SheetImportInclude(include) => self.sheet_import_include(include),
            Message::SheetImportSize(delta) => self.sheet_import_size(delta),
            Message::SheetImportMode(mode) => self.sheet_import_mode(mode),
            Message::SheetImportGroups(groups) => self.sheet_import_groups(groups),
            Message::SetLayout(layout) => self.set_layout(layout),
            Message::SetCover(cover) => self.set_cover(cover),
            Message::SetSort(key) => self.set_sort(key),
            Message::SetSortAsc(ascending) => self.set_sort_asc(ascending),
            Message::StepColumns(delta) => self.step_columns(delta),
            Message::AutoColumns => self.auto_columns(),
            Message::CreateShelf => self.create_shelf(),
            Message::Reload => self.reload_library(now),
            Message::OpenBook(id) => self.open_library_book(id),
            Message::CardHover(hovered) => self.card_hover(hovered, now),
            Message::DragBand(id, band) => self.drag_band(id, band, now),
            Message::CrumbHover(hovered) => self.crumb_hover(hovered, now),
            Message::EllipsisHover(over) => self.ellipsis_hover(over, now),
            Message::EllipsisPressed => self.ellipsis_pressed(),
            Message::PanelCrumb(id) => {
                self.navigate_to(id);
                Task::none()
            }
            Message::CardTap(id) => self.card_tap(&id),
            Message::PressStarted => self.press_started(now),
            Message::PressEnded => self.press_ended(),
            Message::FloorPressed => self.floor_pressed(),
            Message::SelectAll => self.select_all(),
            Message::ToggleSelectPop => self.toggle_select_pop(),
            Message::FileSelection(shelf_id) => self.file_selection_to(Some(shelf_id)),
            Message::FileSelectionOnNewShelf => self.file_selection_to(None),
            Message::AskRemoveSelection => self.ask_remove_selection(),
            Message::ClearSelection => self.clear_selection(),
            Message::EnterPressed => self.enter_pressed(),
            Message::ShiftEnter => self.shift_enter(),
            Message::KeepSelection => Task::none(),
            Message::PickFiles => self.pick_files(),
            Message::FilesPicked(picked) => self.import_files(picked),
            Message::PickFolder => self.pick_folder(),
            Message::FolderPicked(picked) => self.folder_picked(picked),
            Message::ScanDone(task, result) => self.scan_done(task, result, now),
            Message::CopiesScanned(task, result) => self.copies_scanned(task, result, now),
            Message::CopiesDone(task, results) => self.copies_done(task, results),
            Message::FilesChecked(task, checks) => self.files_checked(task, checks),
            Message::FilesCopied(task, results) => self.files_copied(task, results),
            Message::ChecksDone(checks) => self.checks_done(checks),
            Message::ToggleWatch(shelf_id) => self.toggle_watch(&shelf_id),
            Message::PickFilesInFolder(root) => self.pick_files_in_folder(root),
            Message::RestoreChecked(checks) => self.restore_checked(checks),
            Message::RestoreDeleted(folder_id, fp) => self.restore_deleted(folder_id, fp),
            Message::RestoreMeasured(task, checks) => self.restore_measured(task, checks),
            Message::RestoreCopied(task, results) => self.restore_copied(task, results),
            Message::ConfirmMoved(ask) => self.confirm_moved(ask),
            Message::AlsoShow(book_id, shelf_id) => self.also_show(&book_id, &shelf_id),
            Message::GoAndLook(book_id) => self.go_and_look(&book_id),
            Message::MenuBack => self.menu_back(),
            Message::ImportProgress(beat) => self.import_progress(beat),
            Message::CycleAppearance => self.cycle_appearance(),
        }
    }
}
