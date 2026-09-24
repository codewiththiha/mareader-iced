//! Every event the app answers: one enum, and the small types its variants
//! carry so a message stays the whole of what happened.
use std::path::PathBuf;

use iced::time::Instant;
use iced::widget::scrollable;
use iced::{window, Point};
use library_core::book::Fingerprint;
use library_core::conflict::Placement;
use library_core::folder::FolderMode;
use library_core::scan::FoundFile;
use library_core::sort::SortKey;
use library_core::view::{CoverFit, LibraryLayout};
use library_core::wire::{ImportProgress, PathCheck, StoreResult};
use reader_core::format::Format;

use crate::chrome::titlebar::{self};
use crate::library::departure::CopyAnswer;
use crate::library::drag::Band;
use crate::reader;
use super::sheets::MovedAsk;

/// Which of the bar's panels is open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// The add door: pickers for files and folders.
    Add,
    /// The shelf view menu: layouts, columns, covers, sorting.
    View,
    /// The shelf's own menu, hung off the last crumb: rename, remove.
    Shelf,
}

/// The thing a right-click asked about: one of the level's rows, or one of
/// its folder shelves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTarget {
    /// A book or a link.
    Row(String),
    /// A folder shelf.
    Folder(String),
    /// The whole chosen set: the right-click on a member while choosing.
    Selection,
    /// The level's own floor.
    Level,
}

/// One right-click, answered: what was asked about, and where the pointer
/// stood when it asked.
#[derive(Debug, Clone)]
pub(super) struct ContextRequest {
    pub(super) target: ContextTarget,
    pub(super) at: Point,
}

/// Everything the application can be told.
#[derive(Debug, Clone)]
pub enum Message {
    /// The main window's identity, from the boot query.
    WindowDiscovered(Option<window::Id>),
    /// A window lifecycle event: opened, rescaled, resized, focused, a file
    /// dropped.
    WindowEvent(window::Id, window::Event),
    /// The window answered a maximize query.
    Maximized(bool),
    /// The window answered a scale-factor query.
    ScaleFactor(f32),
    /// The pointer moved (window coordinates), or left the window.
    Cursor(Option<Point>),
    /// One animation frame, subscribed to only while something is in
    /// motion: the reveal animating, a hide waiting out its grace, or a
    /// toast waiting out its stamp.
    Tick(Instant),
    /// The titlebar's own business.
    Chrome(titlebar::Message),
    /// Everything the reading surface can be told.
    Reader(reader::Message),
    /// Stand on another level of the library.
    Navigate(String),
    /// The search pill's text changed.
    Query(String),
    /// Open (or toggle shut) one of the bar's panels.
    ToggleMenu(MenuKind),
    /// Dismiss the open panel — the scrim, the Escape key.
    CloseMenu,
    /// The Escape key, when nothing else owns it.
    EscapePressed,
    /// Turn the last crumb into a rename field for the shelf it names.
    StartRename,
    /// The rename field's text changed.
    RenameDraft(String),
    /// Commit the rename field's text as the shelf's name.
    CommitRename,
    /// Take the shelf the reader stands on apart.
    RemoveShelf,
    /// A right-click asked about a row or a folder shelf.
    ContextMenu(ContextTarget),
    /// Show a row's place on disk in the OS file manager.
    RevealRow(String),
    /// Ask the rename sheet for a row's new name.
    AskRenameRow(String),
    /// Ask the rename sheet for a shelf's new name.
    AskRenameShelf(String),
    /// Ask the remove sheet before a row leaves the library.
    AskRemoveRow(String),
    /// Mint a shelf inside another shelf and step into it.
    NewShelfInside(String),
    /// Take any shelf apart — the folder card's door to it.
    TakeApart(String),
    /// The rename sheet's field changed.
    SheetDraft(String),
    /// The sheet's affirmative button.
    SheetSave,
    /// The sheet's scrim, its Cancel button, or Escape.
    SheetCancel,
    /// The import sheet: flip one format in or out of the walk's reach.
    SheetImportFormat(Format),
    /// The import sheet: the selected formats are what the walk admits —
    /// or what it refuses.
    SheetImportInclude(bool),
    /// The import sheet: step the size threshold up or down.
    SheetImportSize(i32),
    /// The import sheet: how the books are held — copied, read at place,
    /// or read at place and watched.
    SheetImportMode(FolderMode),
    /// The import sheet: a shelf per subfolder, or one shelf for the whole
    /// tree.
    SheetImportGroups(bool),
    /// The view's layout: grid or list.
    SetLayout(LibraryLayout),
    /// The covers' fit.
    SetCover(CoverFit),
    /// The level's sort key.
    SetSort(SortKey),
    /// The sort's direction.
    SetSortAsc(bool),
    /// Pin the column count one step up or down.
    StepColumns(i32),
    /// Hand the column count back to auto-fit.
    AutoColumns,
    /// Mint a shelf at the level on screen and step into it.
    CreateShelf,
    /// Re-read the library from disk.
    Reload,
    /// Open a book from the shelf.
    OpenBook(String),
    /// The card the pointer entered or left.
    CardHover(Option<String>),
    /// Which part of a cell the pointer is on, while a drag is live: the
    /// band its sensor zones read — halves for a book, and a folder's
    /// middle band is the nest its edges are not.
    DragBand(String, Band),
    /// The crumb the pointer entered or left — `Some("")` for Home, the
    /// library's spelling of "no shelf". While a drag is live the crumbs
    /// are the bar's hot truth: Shelf targets, and the sink's subject.
    CrumbHover(Option<String>),
    /// The pointer entered or left the ellipsis — or the panel hanging off
    /// it, whose hover keeps it open the same way.
    EllipsisHover(bool),
    /// The ellipsis was pressed: the panel opens outright.
    EllipsisPressed,
    /// A crumb inside the fold's panel was pressed: the way back.
    PanelCrumb(String),
    /// The context menu's "Select": the choosing set begins at this row.
    SelectRow(String),
    /// A row's "Duplicate": a second copy of the book, the library's own,
    /// filed beside the row the reader pointed at.
    DuplicateRow(String),
    /// A shelf's "Duplicate": a second tree holding fresh copies of its
    /// books — asked from the breadcrumb's menu or a folder card's.
    DuplicateShelf(String),
    /// The set's "Duplicate": every chosen entry, in turn.
    DuplicateSelection,
    /// The copy sheet's own answer: buy the copies and finish the gesture,
    /// finish it without them, or leave everything as it is.
    AnswerCopy(CopyAnswer),
    /// The question sheet's own answer: which of the offered placements
    /// the reader meant, and whether the switch behind the answers sends
    /// the same one to every waiting question of this sheet's kind.
    AnswerPlacement(Placement, bool),
    /// The folder question's own answer: which of the offers decides what
    /// the walk is for.
    AnswerShelf(Placement),
    /// The apply-to-all switch's own click.
    ToggleApplyAll,
    /// The ask a read-at-place re-import leaves behind: closing the note
    /// is the answer, because the light stands where the sheet said it
    /// would.
    CloseAlreadyImported,
    /// A cell was tapped. One message for every cell: the app decides what
    /// a tap means — a membership while choosing, an open otherwise.
    CardTap(String),
    /// The shelf scroll's own report: the viewport the reveal centres on.
    ShelfViewport(scrollable::Viewport),
    /// The left button went down somewhere in the window: the hold
    /// machine's starting gun. It decides for itself whether the press
    /// landed on a cell it may hold.
    PressStarted,
    /// The left button came up: the hold machine stops counting.
    PressEnded,
    /// A press on the level's floor that no cell claimed.
    FloorPressed,
    /// Choose everything on screen.
    SelectAll,
    /// The selection bar's shelf popover: open or close.
    ToggleSelectPop,
    /// File the chosen set onto the shelf that was named.
    FileSelection(String),
    /// Mint a new shelf and file the chosen set onto it.
    FileSelectionOnNewShelf,
    /// Ask the remove sheet about the whole chosen set.
    AskRemoveSelection,
    /// Leave the choosing mode, keeping nothing.
    ClearSelection,
    /// Enter on the shelf, when no field owns the key: a tap by keyboard.
    EnterPressed,
    /// Shift+Enter on the shelf: the keyboard's hold.
    ShiftEnter,
    /// A press on the selection bar's own chrome — captured so it cannot
    /// fall through to the floor, and answered with nothing.
    KeepSelection,
    /// The shelf asked for the multi-file picker.
    PickFiles,
    /// The picker answered: paths, or nothing when dismissed.
    FilesPicked(Option<Vec<PathBuf>>),
    /// The shelf asked for the folder picker.
    PickFolder,
    /// The folder picker answered.
    FolderPicked(Option<PathBuf>),
    /// A folder walk finished: the run's id, and the documents it found or
    /// the advice the walk answers with.
    ScanDone(u64, Result<Vec<FoundFile>, String>),
    /// The copy run's scan answer: what the ground held.
    CopiesScanned(u64, Result<Vec<FoundFile>, String>),
    /// A folder import's store batch finished: one answer per requested
    /// copy.
    CopiesDone(u64, Vec<StoreResult>),
    /// The loose-file run's measurement finished.
    FilesChecked(u64, Vec<PathCheck>),
    /// The loose-file run's store batch finished.
    FilesCopied(u64, Vec<StoreResult>),
    /// The boot-and-focus measure pass finished: one check per address the
    /// library holds.
    ChecksDone(Vec<PathCheck>),
    /// The folder context menu's watch row: flip the seat's rung.
    ToggleWatch(String),
    /// The add menu's in-folder door: browse the watched folder's own root
    /// for documents.
    PickFilesInFolder(String),
    /// The add menu's gone check answered: which of the logged addresses
    /// are no longer on disk.
    RestoreChecked(Vec<PathCheck>),
    /// A restore row was clicked: give this removed book back.
    RestoreDeleted(String, Fingerprint),
    /// A restore's one-file measurement finished.
    RestoreMeasured(u64, Vec<PathCheck>),
    /// A restore's store copy finished.
    RestoreCopied(u64, Vec<StoreResult>),
    /// A Moved row was clicked: swap the panel into the two-choice face.
    ConfirmMoved(MovedAsk),
    /// The confirm face's first answer: one book, two shelves, nothing
    /// copied.
    AlsoShow(String, String),
    /// The confirm face's second answer: close the menu and go look at the
    /// shelf the book is on.
    GoAndLook(String),
    /// The confirm face's Back row: the panel becomes the menu again.
    MenuBack,
    /// One progress beat from a run in flight.
    ImportProgress(ImportProgress),
    /// Cycle the appearance base and persist the settings.
    CycleAppearance,
}
