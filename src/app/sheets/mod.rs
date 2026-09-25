//! The sheets: every question the app raises as a modal, the slot the
//! questions queue in, and the two dispatchers that draw them and write
//! their answers back.

use std::path::PathBuf;
use iced::widget::Id;
use iced::{Element, Task};
use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::library::departure::CopyAsk;
use crate::ui::sheet;
use super::Mareader;
use super::message::Message;
use super::walk::GroundWatch;

mod conflict;
mod copy;
mod import;
mod remove;
mod rename;

/// The sheet's rename field's identity — focus lands on it the moment the
/// sheet appears.
const SHEET_INPUT: Id = Id::new("sheet-input");

/// What the two sheets rename, so one sheet shape serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenameKind {
    Row,
    Shelf,
}

/// The question on screen, if one: a modal the shelf waits on.
#[derive(Debug, Clone)]
pub(super) enum Sheet {
    /// A name being written: rename a row or a shelf.
    Rename { kind: RenameKind, id: String, draft: String },
    /// A row about to leave the library.
    Remove { id: String, name: String },
    /// The chosen set about to leave the library: its books, and the
    /// folder shelves themselves.
    RemoveMany { books: Vec<String>, shelves: Vec<String> },
    /// A folder about to be imported: the options sheet decides what the
    /// walk admits, and how the books are held. `ground` is the tree the
    /// pick belongs to, when one governs it — the sheet seeds its answers
    /// from the tree's own, and its Import writes the watch answer back
    /// onto the rung the pick names.
    Import { root: PathBuf, ground: Option<GroundWatch> },
    /// A move about to store copies: the departure's question, carrying the
    /// gesture it interrupted.
    Copy { ask: CopyAsk },
    /// A level that already holds the arriving name: the question, and the
    /// arrival the answer places.
    Conflict { ask: ConflictAsk },
    /// A folder whose name the root level already holds: asked before the
    /// walk, because the answer decides what the walk is for.
    ShelfConflict { ask: ShelfConflictAsk },
    /// Not a question — an answer: a folder the library already reads in
    /// place is named, and closing the note is every way out's promise: the
    /// shelf lights up.
    AlreadyImported { note: LitNote },
}

#[derive(Clone, Debug)]
pub(super) struct LitNote {
    pub(super) shelf_id: String,
    pub(super) name: String,
    pub(super) kind: conflicts::NoteKind,
}

impl Mareader {
    /// The sheet's affirmative answer, carried out.
    pub(super) fn save_sheet(&mut self) -> Task<Message> {
        let Some(sheet) = self.sheet.take() else {
            return Task::none();
        };
        match sheet {
            Sheet::Rename { kind, id, draft } => self.save_rename(kind, id, draft),
            Sheet::Remove { id, .. } => self.remove_row(&id),
            Sheet::RemoveMany { books, shelves } => self.remove_entries(books, shelves),
            // The copy sheet's buttons carry their own answers; its
            // affirmative one — the sheet's "Save", an Enter on the panel —
            // is the ask's primary: buy the copies and finish the gesture.
            Sheet::Copy { ask } => self.copy_and_finish(ask),
            // The name question has no default yes: its answers are the
            // placements, and its one close skips the queue with it.
            Sheet::Conflict { .. } => {
                self.conflict_waiting.clear();
                Task::none()
            }
            Sheet::ShelfConflict { .. } => Task::none(),
            Sheet::AlreadyImported { note } => self.reveal_shelf(&note.shelf_id),
            Sheet::Import { root, ground } => self.save_import(root, ground),
        }
    }

    /// What dropping this question means: the conflict sheet's close — its
    /// Cancel, its scrim, its Escape — skips the question on screen and
    /// every one behind it; placements already answered keep their answers.
    pub(super) fn dismiss_sheet(&mut self) {
        if matches!(self.sheet, Some(Sheet::Conflict { .. })) {
            self.conflict_waiting.clear();
        }
        self.sheet = None;
    }

    /// The modal question in flight: the rename sheet or the remove sheet.
    pub(super) fn sheet_layer(&self) -> Option<Element<'_, Message>> {
        let sheet_state = self.sheet.as_ref()?;
        let panel: Element<'_, Message> = match sheet_state {
            Sheet::Rename { draft, .. } => self.rename_panel(draft),
            Sheet::Remove { name, .. } => self.remove_panel(name),
            Sheet::RemoveMany { books, shelves } => self.remove_many_panel(books, shelves),
            Sheet::Import { root, ground } => self.import_panel(root, ground),
            // The departure's question: the action as the heading, the subject
            // and the cost as the body's own lines, and one button per answer
            // the ask was raised with — Cancel always first, the way the web
            // sheet's footer stood them.
            Sheet::Copy { ask } => self.copy_panel(ask),
            // The name question: the arriving name as the heading, where the
            // collision is — and what waits behind it — under that, the
            // question in one sentence, and the answers as rows that each
            // promise what choosing them does. Cancel means the same thing on
            // every sheet: leave the shelf as it is, and skip the queue.
            Sheet::Conflict { ask } => self.conflict_panel(ask),
            Sheet::ShelfConflict { ask } => self.shelf_conflict_panel(ask),
            Sheet::AlreadyImported { note } => self.note_panel(note),
        };
        Some(sheet::overlay(panel, Message::SheetCancel))
    }
}
