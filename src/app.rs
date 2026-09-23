//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine, the theme the tokens resolved to, the two
//! persisted blobs, the shelf's navigation facts (the level it stands on,
//! the query narrowing it, the menu it holds open), the toast slot and the
//! filesystem runs in flight. Every event is a message, every message
//! returns at most a task, and the view is a pure read of the state — the
//! Elm order the web app's `AppState` kept.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::time::{Duration, Instant};
use iced::widget::{
    button, column, container, mouse_area, operation, row, stack, text, text_input, Column, Id,
    Row, Space, Stack,
};
use iced::border::Radius;
use iced::{
    event, keyboard, mouse, window, Alignment, Background, Border, Color, Element, Length,
    Padding, Point, Shadow, Size, Subscription, Task, Theme, Vector,
};

use library_core::blob::LibraryBlob;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::folder::{
    self as folder_ops, rel_under, FolderMode, FolderOpts, Tombstone, WatchedFolder,
    MIN_SIZE_CEIL, MIN_SIZE_FLOOR,
};
use library_core::governance::Governance;
use library_core::ledger::{self, Recovered, ScanAction};
use library_core::paths;
use library_core::scan::{selectable_formats, FoundFile};
use library_core::shelf::{self, Shelf, ALL_SHELF};
use library_core::sort::SortKey;
use library_core::text as lib_text;
use library_core::view::{CoverFit, LibraryLayout};
use library_core::wire::{BookFileRequest, ImportPhase, ImportProgress, PathCheck, StoreResult};
use reader_core::appearance::BaseMode;
use reader_core::format::{is_supported_path, Format};
use reader_core::settings::Settings;

use crate::chrome::icons::{icon, IconName};
use crate::chrome::platform::{self, Os};
use crate::chrome::titlebar::{self, Titlebar};
use crate::library::card::{plate_seam, THUMB_CAP};
use crate::library::fold::{self, FoldPlan};
use crate::library::drag::{
    drop_effect, fold_items, fold_preview, Band, DragPayload, DropEffect, DropQuery,
    DropTargetKind, FoldPreview,
};
use crate::library::departure::{self, CopyAnswer, CopyAsk, CopyWork, RowMove};
use crate::library::duplicate::{self, BookCopy, Duplicated, DupPlan, TreePlan};
use crate::library::{self, bar, menus};
use crate::platform::{dialogs, fs, progress, store};
use crate::route::Route;
use crate::storage;
use crate::theme::{self, fade, mix, wash, Tokens};
use crate::ui::menu as popover;
use crate::ui::sheet;
use crate::ui::toast::{ToastHost, Tone};

/// The sheet's rename field's identity — focus lands on it the moment the
/// sheet appears.
const SHEET_INPUT: Id = Id::new("sheet-input");

/// Runs the reader.
pub fn run() -> iced::Result {
    iced::application(Mareader::boot, Mareader::update, Mareader::view)
        .title(Mareader::title)
        .theme(Mareader::theme)
        .style(Mareader::style)
        .subscription(Mareader::subscription)
        .window(window_settings())
        .run()
}

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
struct ContextRequest {
    target: ContextTarget,
    at: Point,
}

/// A hold in flight: the cell the press landed on, where it landed, and
/// when it started. A press that moves past the drag's threshold stops
/// being a hold — the gesture that arrives with the drag session owns the
/// same fields from there.
struct Press {
    id: String,
    at: Point,
    started: Instant,
}

/// The session's hot target's identity: which cell or crumb the pointer
/// is on, in the family it arrived from. An exit only clears a hot of its
/// own family — the bar rides above the level in the tree, so a move from
/// a crumb onto a card queues the card's enter BEFORE the crumb's exit,
/// and an unguarded exit would wipe the hot that just arrived.
#[derive(Clone, PartialEq)]
enum Hot {
    Card(String),
    Crumb(String),
    /// The fold's ellipsis: a hover target, never a drop — it stands for
    /// several levels and cannot say which one the hold would choose.
    Ellipsis,
}

/// Which family of hover an exit arrived from: an exit only clears a hot
/// of its own family, the same rule [`Hot`] exists for.
#[derive(Clone, Copy, PartialEq)]
enum Family {
    Card,
    Crumb,
    Ellipsis,
}

impl Hot {
    fn family(&self) -> Family {
        match self {
            Hot::Card(_) => Family::Card,
            Hot::Crumb(_) => Family::Crumb,
            Hot::Ellipsis => Family::Ellipsis,
        }
    }
}

/// A drag in flight: what it holds, the band the sensors last reported,
/// the fold dwell's clock and the crumb sink's spot. The hot target is the
/// shelf's hover truth — the same fact the press machine reads — so the
/// grid needs no sensor of its own, and a release over nothing is the
/// level's answer.
struct Drag {
    payload: DragPayload,
    /// The band the hot cell's sensor zones last reported; the middle
    /// until one does.
    band: Band,
    /// Whether the rest over the hot target has passed the fold's dwell.
    dwell_armed: bool,
    /// Where the ghost parked when the rest over a crumb passed the sink's
    /// dwell: from here the ghost reads the target rather than the hand,
    /// until the hot changes and it grows back.
    sunk: Option<Point>,
    /// When the hot target became hot — both dwells' clock.
    hot_started: Instant,
    /// The hot target the clocks are counting for.
    last_hot: Option<Hot>,
}

/// The web gesture's tuning, carried over whole (long_press.rs): the hold
/// decides at 450ms, a press that travelled more than 8px is no hold, and
/// a press that travelled more than 6px is a drag — the drag's threshold
/// is inside the hold's slop, so a press that moved enough to drag can
/// never also decide to hold.
const SELECT_PRESS_MS: u128 = 450;
const SELECT_SLOP_PX: f32 = 8.0;
const DRAG_THRESHOLD_PX: f32 = 6.0;
const _: () = assert!(DRAG_THRESHOLD_PX < SELECT_SLOP_PX);

/// How long a rest over a book must last before the fold is offered: long
/// enough that a reorder crossing the card never brews a shelf, short
/// enough that the reader is not left waiting on the answer (the web
/// session's own 650ms). The crumb sink's 420ms rides with the crumb
/// targets, which arrive with the bar's drag geometry.
const FOLD_DWELL_MS: u128 = 650;

/// Shorter than the fold's dwell, and for one reason only: the crumb is
/// the one target smaller than the ghost hovering it, and a full-size
/// ghost hides the very name being aimed at — so a rest this long parks
/// the ghost on the crumb, shrunk (the web session's own 420ms).
const SINK_DWELL_MS: u128 = 420;

/// The fold panel's close waits this long behind the pointer's leave — the
/// web intent's own grace, so a diagonal crossing of the panel's corner
/// does not blink it shut.
const ELLIPSIS_GRACE_MS: u64 = 220;

/// The centre pill's usable floor: what the fold reserves for the search
/// before it starts hiding levels, so a crammed bar never squeezes the
/// search box past use.
const CENTER_FLOOR: f32 = 400.0;

/// The sunk ghost's scale, straight off drag.css: a third of its size is
/// what keeps the crumb readable, because the chrome's lane sits below the
/// drag layer and no z-step can put the ghost behind a crumb without
/// putting it behind everything.
const SUNK_SCALE: f32 = 0.38;

/// What the two sheets rename, so one sheet shape serves both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenameKind {
    Row,
    Shelf,
}

/// The question on screen, if one: a modal the shelf waits on.
#[derive(Debug, Clone)]
enum Sheet {
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
}

/// Which question a folder run answers; a boolean at the signature could
/// not say. An ask is the reader's own import; a walk is the app keeping a
/// watched folder's promise to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Explicitly,
    OnFocus,
}

/// What a picked ground belongs to: the tree that governs it, the rung the
/// ground names inside the tree, the watch answer that rung carries, and
/// the tree's own options with the rung's shape answer. The import sheet
/// opens seeded from these — a folder inside a governed tree asks nothing
/// the tree already answered — and its Import writes the watch answer back
/// onto the rung, never onto the tree's root.
#[derive(Debug, Clone)]
struct GroundWatch {
    tree_id: String,
    rung: String,
    on: bool,
    opts: FolderOpts,
}

/// The watch seat a folder shelf answers for: which rung of which tree,
/// what that rung watches now, and the labels the toggle row speaks. The
/// row the folder context menu has and no other menu does, because no
/// other shelf has a rung to answer for.
struct ShelfWatch {
    folder_id: String,
    rung: String,
    on: bool,
    label: String,
    rung_label: Option<String>,
}

/// The Moved row the add menu is confirming: a book still inside the
/// folder on disk but filed on shelves elsewhere. The confirm face offers
/// two answers — show it here as well, or go look at where it went.
#[derive(Debug, Clone)]
pub struct MovedAsk {
    book_id: String,
    title: Option<String>,
    path: String,
    home_shelf: Option<String>,
}

/// One filesystem run and its channel: the id the beats carry, the label
/// the dock pill names it by, the parked receiver the subscription takes
/// on first build, the sender the run's later stages reuse, and the latest
/// beat — the pill's whole input.
struct FsRun {
    task: u64,
    label: String,
    sink: progress::ProgressSink,
    rx: progress::SharedProgress,
    latest: Option<ImportProgress>,
    stage: Stage,
}

/// Where a run is in its life, and the plan its next stage lands. The plans
/// ride in boxes: a run is a small thing the state tree holds many of, and
/// a walk's plan — a whole ledger row among its fields — would otherwise
/// decide the size of every stage.
enum Stage {
    /// A folder walk is in flight.
    Walking { root: String, opts: FolderOpts, asked: Asked },
    /// The store is copying a folder import's additions; the walk's answer
    /// waits in the plan.
    Storing { plan: Box<WalkPlan> },
    /// The picker's files are being measured.
    Measuring { target: Option<String> },
    /// The store is copying the picker's files; the landings wait in the
    /// plan.
    Copying { plan: Box<FilesPlan> },
    /// A restore is measuring the one file its log remembered.
    Restoring { folder_id: String, stone: Box<Tombstone>, opts: FolderOpts },
    /// The store is copying a restore's file; the landing waits in the run.
    RestoreCopying {
        folder_id: String,
        stone: Box<Tombstone>,
        opts: FolderOpts,
        found: Box<FoundFile>,
        book_id: String,
    },
    /// The store is copying a duplicate's bytes; the landing waits in the
    /// work.
    Duplicating { work: Box<DupWork> },
    /// The store is copying a departure's books; the interrupted gesture
    /// waits in the work.
    Departing { work: Box<DepartWork> },
}

/// A duplicate's landing, waiting on its copies: one book filed beside the
/// row the reader pointed at, or a whole fresh subtree spliced in behind the
/// original.
enum DupWork {
    Book(Box<BookCopy>),
    Tree(TreePlan),
}

/// A departure run's landing: the gesture's whole id list, the rows the
/// copies were asked for, and the hand that resumes when they come home.
struct DepartWork {
    ids: Vec<String>,
    converting: Vec<String>,
    hand: RowMove,
}

/// The folder walk's answer, planned against the ledger and waiting for its
/// copies (or landing straight away when the books stay at place).
struct WalkPlan {
    /// The ledger row, resolved against the library as the walk ended —
    /// placed marks, spent tombstones and the shelf map are written onto it
    /// as the landing runs, and it goes back whole at the end.
    folder: WatchedFolder,
    asked: Asked,
    /// The folder's display name, for the toasts.
    root_name: String,
    /// The deduped name a fresh root rung wears; `None` when the map already
    /// holds the root shelf.
    planned_root: Option<String>,
    /// The additions, each wearing the book id it will land as.
    adds: Vec<(String, FoundFile)>,
    /// The moves the ledger healed: book id to its new address.
    relinks: Vec<(String, String)>,
}

/// One loose file queued for the store: the id it lands as, the finding, and
/// the name a spent tombstone remembered.
struct PendingCopy {
    book_id: String,
    file: FoundFile,
    title: Option<String>,
}

/// The loose-file run's answer, waiting for its copies.
struct FilesPlan {
    /// The level the pick lands its books on, when one is standing.
    target: Option<String>,
    pending: Vec<PendingCopy>,
    /// Files the library already held, counted for the toast.
    already: usize,
    /// Covered files restored as their folder's linked books, counted too.
    restored: usize,
}

/// One copy's landing: the address it stored at and the measurement of its
/// own bytes.
type Stored = (String, Option<Fingerprint>);

/// The store batch's answer, keyed: the book id each copy was requested
/// under to the address it landed at and the measurement of its own bytes.
type CopyMap = HashMap<String, Stored>;

/// The whole application state.
pub struct Mareader {
    /// The main window's identity, resolved once at boot — every window
    /// command (drag, minimize, close) is addressed to it.
    window: Option<window::Id>,
    /// The maximize state the caption glyph swaps on.
    maximized: bool,
    /// Which surface is on screen. The document pipeline that arrives with
    /// the engines drives this exactly as the web app's URL did: a document
    /// Ready is the reader, nothing open is the shelf.
    route: Route,
    /// The titlebar's hover machine.
    titlebar: Titlebar,
    /// The tokens of the current base, and the iced theme built from them.
    tokens: Tokens,
    theme: Theme,
    /// The persisted look and its neighbours — loaded at boot, saved on
    /// every change, the same contract the web app's storage layer kept.
    settings: Settings,
    /// The persisted shelf: rows, shelves, watched folders, and the view
    /// that paints them. Loaded at boot, saved at the moment of every
    /// change.
    library: LibraryBlob,
    /// The level the shelf is standing on — a shelf's id, or the library's
    /// own spelling of "no shelf".
    shelf: String,
    /// The query narrowing the level.
    query: String,
    /// The bar's open panel, if any.
    menu: Option<MenuKind>,
    /// The add menu's confirm face: the Moved row the reader asked about,
    /// holding the panel open as a two-choice question.
    menu_confirm: Option<MovedAsk>,
    /// The paths the add menu's gone check answered missing: a restore row
    /// whose file is no longer on disk renders disabled rather than
    /// promising a landing that cannot happen.
    restore_gone: HashSet<String>,
    /// The last known pointer position in window coordinates — the anchor
    /// a menu is placed at.
    cursor: Point,
    /// The window's size — the viewport a menu is clamped into.
    viewport: Size,
    /// The card the pointer is over: the grid's hover truth.
    hovered_card: Option<String>,
    /// Whether the shelf is in the choosing mode a hold starts.
    selecting: bool,
    /// The chosen set: row ids and shelf ids on the level.
    selected: HashSet<String>,
    /// The hold in flight, if the press landed on a cell.
    press: Option<Press>,
    /// The drag in flight, if the press became one.
    drag: Option<Drag>,
    /// The crumb the pointer is over: the bar's hover truth.
    hovered_crumb: Option<String>,
    /// Whether the fold's panel is showing.
    ellipsis_open: bool,
    /// When the panel closes if the pointer stays away: the leave's grace.
    /// A leave that lands inside the panel's own box arms nothing — the
    /// bar's layers queue the panel's enter BEFORE the ellipsis's exit, so
    /// the geometry, not the message order, decides who is right.
    ellipsis_close_at: Option<Instant>,
    /// The duplicate queue's remaining entries: one at a time, because each
    /// counter name counts against the level as the last landing left it.
    dup_queue: Vec<String>,
    /// What the running batch has landed so far, for the report it ends
    /// with.
    dup_landed: Vec<Duplicated>,
    /// The tap a hold swallowed: the release after a hold still belongs to
    /// the cell's button, and this flag tells that one tap to stay quiet.
    /// Cleared at the START of the next press rather than at the release,
    /// so the order of widget and listener messages inside one event can
    /// never matter.
    tap_swallow: Option<String>,
    /// Whether the selection bar's shelf popover is open.
    select_pop: bool,
    /// Whether the last crumb is a rename field right now.
    renaming: bool,
    /// What the rename field holds, mid-typing.
    rename_draft: String,
    /// The right-click menu in flight, if any.
    context: Option<ContextRequest>,
    /// The modal question in flight, if any.
    sheet: Option<Sheet>,
    /// The import sheet's options. They outlive the sheet: a second folder
    /// is usually imported the same way as the first.
    import_opts: FolderOpts,
    /// The app-global toast slot.
    toasts: ToastHost,
    /// The document the reader route is showing for — remembered in the
    /// settings' `last_path` the moment the gate admits it.
    open_document: Option<PathBuf>,
    /// The filesystem runs in flight: folder walks, their store batches,
    /// and the loose-file runs. One dock pill per run.
    runs: Vec<FsRun>,
    /// An explicit import waiting for a focus walk of the same folder to
    /// release it: an ask outranks a rescan, but never races its ledger
    /// write.
    queued_ask: Option<(PathBuf, FolderOpts)>,
    /// Whether the boot-and-focus measure pass is in flight.
    verifying: bool,
    /// The id the next filesystem run wears. Progress beats carry it, so
    /// two runs' answers can never mix.
    next_task: u64,
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
    /// A cell was tapped. One message for every cell: the app decides what
    /// a tap means — a membership while choosing, an open otherwise.
    CardTap(String),
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
    /// The reader route handed the screen back to the shelf.
    BackToShelf,
}

impl Mareader {
    fn boot() -> (Self, Task<Message>) {
        let settings = storage::load_settings();
        let tokens = Tokens::for_base(settings.appearance.base);
        let mut state = Self {
            window: None,
            maximized: false,
            route: Route::Library,
            titlebar: Titlebar::new(),
            theme: theme::build(tokens, settings.appearance.base),
            tokens,
            library: storage::load_library(),
            shelf: ALL_SHELF.to_string(),
            query: String::new(),
            menu: None,
            menu_confirm: None,
            restore_gone: HashSet::new(),
            cursor: Point::new(600.0, 400.0),
            viewport: Size::new(1200.0, 800.0),
            hovered_card: None,
            selecting: false,
            selected: HashSet::new(),
            press: None,
            drag: None,
            hovered_crumb: None,
            ellipsis_open: false,
            ellipsis_close_at: None,
            dup_queue: Vec::new(),
            dup_landed: Vec::new(),
            tap_swallow: None,
            select_pop: false,
            renaming: false,
            rename_draft: String::new(),
            context: None,
            sheet: None,
            import_opts: FolderOpts::default(),
            settings,
            toasts: ToastHost::default(),
            open_document: None,
            runs: Vec::new(),
            queued_ask: None,
            verifying: false,
            next_task: 0,
        };
        // The grid reports its first fit against the window the app opens
        // in; the Resized event confirms it.
        state.report_auto_fit();
        (state, window::oldest().map(Message::WindowDiscovered))
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Message) -> Task<Message> {
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
                // The fold panel's close waits out its grace; a pointer
                // that came back cancelled it by clearing the deadline.
                if let Some(due) = self.ellipsis_close_at
                    && at >= due
                {
                    self.ellipsis_close_at = None;
                    self.ellipsis_open = false;
                }
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
                    self.sheet = None;
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
                self.remove_shelf_with(&id)
            }
            Message::SheetDraft(text) => {
                if let Some(Sheet::Rename { draft, .. }) = &mut self.sheet {
                    *draft = text;
                }
                Task::none()
            }
            Message::SheetCancel => {
                self.sheet = None;
                Task::none()
            }
            Message::AnswerCopy(answer) => {
                let Some(Sheet::Copy { ask }) = self.sheet.take() else {
                    return Task::none();
                };
                match answer {
                    // Nothing moves and nothing copies. The rows' door has no
                    // without-copies answer of its own — the gesture simply
                    // stays where the folder's tree put it; the shelf's and
                    // the removal's doors arrive with the systems that own
                    // them.
                    CopyAnswer::Cancel | CopyAnswer::WithoutCopies => Task::none(),
                    CopyAnswer::Copy => self.copy_and_finish(ask),
                }
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
                self.open_document_path(path)
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
            Message::BackToShelf => {
                self.route = Route::Library;
                self.menu = None;
                Task::none()
            }
        }
    }

    fn window_event(&mut self, id: window::Id, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Opened { .. } => {
                if self.window.is_none() {
                    self.window = Some(id);
                }
                Task::none()
            }
            window::Event::Rescaled(factor) => {
                self.set_scale_factor(factor);
                Task::none()
            }
            window::Event::Resized(size) => {
                self.viewport = size;
                self.report_auto_fit();
                Task::none()
            }
            // A regained focus owes the library its two automatic
            // measurements — unless the focus is the app's own picker
            // closing, which the measure pass itself screens out.
            window::Event::Focused => self.start_measure_pass(),
            // A drop on the shelf is an arrival: a folder opens the import
            // sheet, documents join the library. A drop on the reader is an
            // open request — the same door the picker uses.
            window::Event::FileDropped(path) => match self.route {
                Route::Reader => self.open_document_path(path),
                Route::Library => {
                    if path.is_dir() {
                        self.open_import_sheet(path);
                        Task::none()
                    } else {
                        self.import_files(Some(vec![path]))
                    }
                }
            },
            _ => Task::none(),
        }
    }

    /// Feed the window's live scale factor to the device-pixel grid the
    /// page geometry snaps to — at open and on every rescale.
    fn set_scale_factor(&mut self, factor: f32) {
        pdf_core::pixel_grid::set_device_pixel_ratio(f64::from(factor));
    }

    /// A view fact changed: close the menu that changed it and persist the
    /// library.
    fn view_changed(&mut self) -> Task<Message> {
        self.menu = None;
        self.persist_library()
    }

    /// Mint a shelf at the level on screen and step into it — the web app's
    /// create-and-enter, one message.
    /// Stand on another level: the panels close, the rename and the choice
    /// in flight belong to the level they started on, and the hover truth
    /// resets — the pointer is over new ground now.
    fn navigate_to(&mut self, shelf: String) {
        self.menu = None;
        self.menu_confirm = None;
        self.context = None;
        // A rename in flight belongs to the shelf it started on;
        // stepping away abandons it rather than carrying the draft.
        self.renaming = false;
        self.hovered_card = None;
        self.hovered_crumb = None;
        self.close_ellipsis();
        self.exit_selection();
        self.shelf = shelf;
    }

    fn create_shelf(&mut self) -> Task<Message> {
        let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        self.create_shelf_in(parent)
    }

    /// Mint a shelf under an explicit parent — `None` hangs it at the top
    /// level — and step into it.
    fn create_shelf_in(&mut self, parent: Option<String>) -> Task<Message> {
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
    fn mint_shelf(&mut self, parent: Option<String>) -> String {
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
    fn remove_shelf_with(&mut self, id: &str) -> Task<Message> {
        self.menu = None;
        self.context = None;
        if !self.dismantle_shelf(id) {
            return Task::none();
        }
        self.persist_library()
    }

    /// A shelf taken apart, receipt aside: the children re-hang on its
    /// parent, it goes, and a reader standing anywhere inside it steps out
    /// to the level it hung from. True when the id named a shelf.
    fn dismantle_shelf(&mut self, id: &str) -> bool {
        let Some(gone) = shelf::find(&self.library.shelves, id) else {
            return false;
        };
        let step_out = gone.parent.clone().unwrap_or_else(|| ALL_SHELF.to_string());
        let gone_id = gone.id.clone();
        let standing_within =
            shelf::subtree_ids(&self.library.shelves, std::slice::from_ref(&gone_id))
                .contains(&self.shelf);
        shelf::lift_children(&mut self.library.shelves, &gone_id);
        self.library.shelves.retain(|shelf| shelf.id != gone_id);
        if standing_within {
            self.shelf = step_out;
        }
        true
    }

    /// What a tap means: a membership while choosing, an open otherwise.
    /// The flag a finished hold leaves behind swallows this one tap — the
    /// release still belongs to the cell's button, and the choice the hold
    /// just started must not immediately toggle the cell back out.
    fn card_tap(&mut self, id: &str) -> Task<Message> {
        if self.tap_swallow.take().is_some_and(|swallowed| swallowed == id) {
            return Task::none();
        }
        if self.selecting {
            self.toggle_selected(id);
            return Task::none();
        }
        self.menu = None;
        self.context = None;
        // Resolved to an owned answer first: the resolve borrows the blob,
        // and the acts that follow borrow the app.
        enum Tap {
            Open(PathBuf),
            Shelf(String),
            Nothing,
        }
        let tap = match book::find_row(&self.library.books, id) {
            Some(book::Row::Book(book)) => Tap::Open(PathBuf::from(book.path())),
            // A link opens onto the shelf it points at — when it still
            // points at one.
            Some(book::Row::Link { target, .. }) if library_core::id::is_shelf(target) => {
                Tap::Shelf(target.clone())
            }
            _ => {
                if shelf::find(&self.library.shelves, id).is_some() {
                    Tap::Shelf(id.to_string())
                } else {
                    Tap::Nothing
                }
            }
        };
        match tap {
            Tap::Open(path) => self.open_document_path(path),
            Tap::Shelf(shelf) => {
                self.navigate_to(shelf);
                Task::none()
            }
            Tap::Nothing => Task::none(),
        }
    }

    /// The hold machine's beat: a press that travelled past the drag's
    /// threshold is no hold (the drag session that lands later owns the
    /// gesture from there); a press that stayed quiet long enough starts
    /// the choice and marks the coming tap swallowed.
    fn on_hold_tick(&mut self, at: Instant) {
        let Some(press) = &self.press else { return };
        let moved = (self.cursor.x - press.at.x).hypot(self.cursor.y - press.at.y);
        if moved > DRAG_THRESHOLD_PX {
            // The press became a drag: the hold machine hands the cell over
            // and stops counting — the drag's own clock, the dwell, starts
            // when its hot target does.
            let id = press.id.clone();
            self.press = None;
            self.begin_drag(&id, at);
            return;
        }
        if at.duration_since(press.started).as_millis() >= SELECT_PRESS_MS && moved <= SELECT_SLOP_PX
        {
            let id = press.id.clone();
            self.press = None;
            self.tap_swallow = Some(id.clone());
            self.enter_selection(&id);
        }
    }

    /// The hold's answer: the mode is on and the cell held is its first
    /// member.
    fn enter_selection(&mut self, id: &str) {
        self.selecting = true;
        self.selected.insert(id.to_string());
    }

    /// The movement's answer: the press becomes a drag. The payload is the
    /// whole set when the pressed cell is in it — in the page's own order,
    /// the same payload the bar's filings carry — else the one cell. The
    /// source is the shelf that rendered it: a drag lifted inside a shelf
    /// is a move out of that shelf, and reading the open level instead
    /// would unfile a book from the shelf it was showing in. The swallow
    /// flag is set the way the hold sets it — the release still belongs to
    /// the cell's button and must not open what was just picked up.
    fn begin_drag(&mut self, id: &str, at: Instant) {
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

    /// A drag's hot target changed: the band falls back to the middle,
    /// both dwells' clock restarts, a rest that was counting is forgotten
    /// and a ghost that was parked grows back. The same fact the cells
    /// dress by — the hover truth — decides when the clocks restart, so a
    /// pointer that circles inside one card keeps its rest and a pointer
    /// that crosses a seam does not.
    fn on_hot_change(&mut self, next: Option<Hot>, at: Instant) {
        let Some(drag) = &mut self.drag else { return };
        if drag.last_hot == next {
            return;
        }
        drag.last_hot = next;
        drag.band = Band::Middle;
        drag.dwell_armed = false;
        drag.sunk = None;
        drag.hot_started = at;
    }

    /// An exit's clear, guarded: it lands only when the session's hot
    /// still belongs to the exiting family, because the bar's messages
    /// queue after the level's and the enter of the new hot arrives
    /// before the exit of the old one.
    fn on_hot_exit(&mut self, family: Family, at: Instant) {
        let owns = self
            .drag
            .as_ref()
            .and_then(|drag| drag.last_hot.as_ref())
            .is_some_and(|hot| hot.family() == family);
        if owns {
            self.on_hot_change(None, at);
        }
    }

    /// The panel shut, wholesale: a chain that changed (the fold is an
    /// answer about levels that no longer show) or a drag that ended (the
    /// web intent closes with the session) both land here.
    fn close_ellipsis(&mut self) {
        self.ellipsis_open = false;
        self.ellipsis_close_at = None;
    }

    /// The dwells' beat: a rest over a crumb parks the ghost there (the
    /// sink), and a rest over a book the table reads as a landing — the
    /// answer, not only the kind — arms the fold, so from the next tick
    /// the same rest answers the shelf the drop will make.
    fn on_drag_tick(&mut self, at: Instant) {
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
    fn drag_answer(&self) -> Option<(DropEffect, Option<FoldPreview>)> {
        let drag = self.drag.as_ref()?;
        // The ellipsis is a hover target and never a drop: it stands for
        // several levels, and the table's refusal keeps the ghost honest
        // while the panel opens under the rest.
        if self.cursor.y < platform::TITLE_BAR_H
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
        if (self.cursor.y < platform::TITLE_BAR_H || self.ellipsis_open)
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
        if self.cursor.y < platform::TITLE_BAR_H {
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
    fn can_sibling_held(&self, held: &DragPayload, anchor: &str) -> bool {
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
    fn release_drag(&mut self) -> Task<Message> {
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
    fn apply_drop(&mut self, effect: DropEffect, payload: DragPayload) -> bool {
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
                moved |= self.gated_seat(&payload.books, from.clone(), to.clone(), index);
                moved |= land_folders(&mut self.library.shelves, &payload.folders, &to);
            }
            DropEffect::ShelfSibling { anchor_id, after } => {
                moved |= library::arrange::reorder_shelves_to_anchor(
                    &mut self.library.shelves,
                    &payload.folders,
                    &anchor_id,
                    after,
                );
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
                moved |= land_folders(&mut self.library.shelves, &payload.folders, ALL_SHELF);
            }
            DropEffect::FileToShelf { shelf_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), shelf_id.clone(), None);
                moved |= land_folders(&mut self.library.shelves, &payload.folders, &shelf_id);
            }
            DropEffect::NestInto { folder_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), folder_id.clone(), None);
                moved |= land_folders(&mut self.library.shelves, &payload.folders, &folder_id);
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
                moved |= self.gated_seat(&books, from.clone(), shelf_id.clone(), None);
                moved |= land_folders(&mut self.library.shelves, &payload.folders, &shelf_id);
            }
        }
        moved
    }

    /// The index is the anchor's position in its container, not a count of
    /// what is on screen — and only a view that reorders by drag asks for
    /// one at all (the web commit's `insert_anchor`).
    fn insert_anchor(
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

    /// A tap while choosing: in out, out in. An empty set keeps the mode —
    /// leaving is Done's, Escape's, or the floor's answer, never a count's.
    fn toggle_selected(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
    }

    /// Every exit goes through here — Done, Escape, a press on the floor,
    /// an action that consumed the set, leaving the page.
    fn exit_selection(&mut self) {
        self.selecting = false;
        self.selected.clear();
        self.select_pop = false;
    }

    /// The set split into its two kinds, in the level's own order: the
    /// folders the level renders, then the rows it renders. A payload for
    /// a filing keeps the page's order, because putting the set back in
    /// the list's order would be an act that quietly shuffled the hand.
    fn split_selection(&self) -> (Vec<String>, Vec<String>) {
        let books = library::level_rows(&self.library, &self.shelf, &self.query)
            .iter()
            .map(|row| row.id().to_string())
            .filter(|id| self.selected.contains(id))
            .collect();
        let shelves = library::level_folders(&self.library, &self.shelf, &self.query)
            .into_iter()
            .map(|shelf| shelf.id)
            .filter(|id| self.selected.contains(id))
            .collect();
        (books, shelves)
    }

    /// The bar's two filing answers: onto the shelf named, or onto a shelf
    /// minted for the occasion. Books go as memberships, folders go as
    /// nestings, and one persist covers the batch. A folder that cannot be
    /// nested onto the target — it would end up inside itself — stays
    /// where it is rather than failing the batch.
    fn file_selection_to(&mut self, target: Option<String>) -> Task<Message> {
        self.context = None;
        let (book_ids, folder_ids) = self.split_selection();
        let target = match target {
            Some(id) => id,
            None => self.mint_shelf(None),
        };
        let mut moved = library::arrange::file_many(&mut self.library.shelves, &book_ids, &target);
        moved |= library::arrange::nest_many(&mut self.library.shelves, &folder_ids, &target);
        // A second membership is an arrival like any other: a stored book
        // landing on a folder's shelf can be the file's return, and the bind
        // is a write the persist has to cover.
        for id in &book_ids {
            moved |= departure::bind_returned(
                &self.library.books,
                &self.library.shelves,
                &mut self.library.folders,
                id,
                &target,
            );
        }
        self.exit_selection();
        if moved {
            return self.persist_library();
        }
        Task::none()
    }

    /// Ask the rename sheet for a name, prefilled with the one on show.
    fn ask_rename(&mut self, kind: RenameKind, id: String) -> Task<Message> {
        self.context = None;
        let draft = match kind {
            RenameKind::Row => book::find_row(&self.library.books, &id)
                .map(|row| match row {
                    book::Row::Book(book) => book.title(),
                    book::Row::Link { name, .. } => name.clone(),
                })
                .unwrap_or_default(),
            RenameKind::Shelf => shelf::find(&self.library.shelves, &id)
                .map(|shelf| shelf.name.clone())
                .unwrap_or_default(),
        };
        self.sheet = Some(Sheet::Rename { kind, id, draft });
        // The field appears in the next view; focus lands with it.
        operation::focus(SHEET_INPUT)
    }

    /// The sheet's affirmative answer, carried out.
    fn save_sheet(&mut self) -> Task<Message> {
        let Some(sheet) = self.sheet.take() else {
            return Task::none();
        };
        match sheet {
            Sheet::Rename { kind, id, draft } => {
                let name = draft.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                match kind {
                    RenameKind::Row => {
                        if let Some(row) = book::find_row_mut(&mut self.library.books, &id) {
                            match row {
                                book::Row::Book(book) => {
                                    book.title = Some(name);
                                    book.title_locked = true;
                                }
                                book::Row::Link { name: own, .. } => *own = name,
                            }
                        }
                    }
                    RenameKind::Shelf => {
                        if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &id) {
                            shelf.name = name;
                        }
                    }
                }
                self.persist_library()
            }
            Sheet::Remove { id, .. } => self.remove_row(&id),
            Sheet::RemoveMany { books, shelves } => self.remove_many(books, shelves),
            // The copy sheet's buttons carry their own answers; its
            // affirmative one — the sheet's "Save", an Enter on the panel —
            // is the ask's primary: buy the copies and finish the gesture.
            Sheet::Copy { ask } => self.copy_and_finish(ask),
            Sheet::Import { root, ground } => {
                let opts = self.import_opts.clone();
                let root_str = root.to_string_lossy().into_owned();
                // The sheet's watch answer belongs to the picked ground's
                // rung — a subfolder's import must not write the tree's
                // root — and only a read-at-place import watches at all.
                let mut persisted = Task::none();
                if opts.mode().reads_in_place()
                    && let Some(mut watch) = ground
                {
                    watch.on = opts.watch;
                    persisted = self.write_rung_tracking(&watch);
                }
                // A covered ground walks the covering tree: a rung cannot
                // mint a second instance of itself, and removed books come
                // back wherever in the tree they stood. The walk's own
                // shelf map seats the pick on the shelf it named.
                let walk_root = if opts.mode().reads_in_place() {
                    self.covered_tree_root(&root_str).map_or(root, PathBuf::from)
                } else {
                    root
                };
                let walk = self.begin_folder_walk(walk_root, opts, Asked::Explicitly);
                Task::batch([persisted, walk])
            }
        }
    }

    /// The removal, and everything the library holds about the row going
    /// out: the memberships, the links that pointed at it, the tombstone
    /// that keeps a watched folder's rescan from putting the book straight
    /// back, and — for a copy the library owns — the byte in the store.
    fn remove_row(&mut self, id: &str) -> Task<Message> {
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

    /// The set's removal, answered: every book leaves the library the way
    /// one does — the ledger's tombstone, the swept copy, the memberships,
    /// the links — and every shelf comes apart the way one does. One
    /// receipt and one persist: a bulk act is one act, not n of them.
    fn remove_many(&mut self, books: Vec<String>, shelves: Vec<String>) -> Task<Message> {
        let removed_books = books.iter().filter(|id| self.purge_row(id)).count();
        let removed_shelves = shelves.iter().filter(|id| self.dismantle_shelf(id)).count();
        if removed_books == 0 && removed_shelves == 0 {
            return Task::none();
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
        self.persist_library()
    }

    /// A row leaving the library, receipt aside: the tombstone the ledger
    /// needs, the copy swept when no twin reads the byte, the memberships,
    /// the links that pointed at it. True when the id named a row.
    fn purge_row(&mut self, id: &str) -> bool {
        let Some(row) = book::find_row(&self.library.books, id) else {
            return false;
        };
        let doomed = row.book().cloned();
        if let Some(book) = &doomed {
            // Read the world before writing any of it: the tombstone needs
            // the folder that placed this book and the shelf it was filed
            // on, and the sweep needs to know no twin still reads the byte.
            let placed_by = self
                .library
                .folders
                .iter()
                .find(|folder| folder.placed.contains(&book.fp))
                .map(|folder| folder.id.clone());
            let home = placed_by
                .as_deref()
                .and_then(|folder_id| folder_shelf_of(&self.library.shelves, folder_id, &book.id));
            let entry = Tombstone::of(book, home, now_ms());
            ledger::tombstone(&mut self.library.folders, &entry);
            let twin_reads = book::book_rows(&self.library.books)
                .any(|each| each.id != book.id && each.path() == book.path());
            if book.origin.is_stored() && !twin_reads
                && let Err(error) = store::delete_stored(book.path())
            {
                // The row is gone either way; a byte the host will not
                // release is the log's business, not a second question.
                eprintln!("[library] could not sweep {}: {error}", book.path());
            }
        }
        book::remove_row(&mut self.library.books, id);
        book::drop_dangling_links(&mut self.library.books);
        shelf::forget_everywhere(&mut self.library.shelves, id);
        true
    }

    /// The pickers' and the drop's answer: measure the files, then land
    /// them the way the library holds loose arrivals — as its own stored
    /// copies, except the ones a read-at-place tree answers for, which come
    /// back as that tree's linked books.
    fn import_files(&mut self, picked: Option<Vec<PathBuf>>) -> Task<Message> {
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
    fn open_import_sheet(&mut self, dir: PathBuf) {
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
    fn write_rung_tracking(&mut self, watch: &GroundWatch) -> Task<Message> {
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

    /// The tree a covered ground belongs to, by root address: an import of
    /// a rung walks the tree, so a rung cannot mint a second instance of
    /// itself and removed books come back wherever in the tree they stood.
    fn covered_tree_root(&self, root: &str) -> Option<String> {
        let governance = Governance::new(&self.library.folders, &self.library.shelves);
        let covered = governance.covering(root)?;
        folder_ops::find(&self.library.folders, &covered.folder_id).map(|row| row.root.clone())
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
    fn watch_facts(&self, shelf_id: &str) -> Option<menus::WatchFacts> {
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
    fn toggle_watch(&mut self, shelf_id: &str) -> Task<Message> {
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
                self.begin_folder_walk(PathBuf::from(root), opts, Asked::OnFocus),
            ])
        } else {
            persisted
        }
    }

    /// Admit a document through the gate, remember it, and route to the
    /// reader — the shape of the web app's open flow, with the reading
    /// surface itself landing alongside the engines.
    fn open_document_path(&mut self, path: PathBuf) -> Task<Message> {
        let address = path.to_string_lossy().into_owned();
        if let Err(error) = fs::ensure_readable_document(&address) {
            self.toasts.show(Tone::Error, error, Instant::now());
            return Task::none();
        }
        self.settings.last_path = Some(address);
        self.open_document = Some(path);
        self.route = Route::Reader;
        self.persist_settings()
    }

    // ── The boot-and-focus measurements ─────────────────────────────────

    /// The library's two automatic measurements, in the one order they owe:
    /// first a pass over every address the library holds — marking books
    /// missing when their file was deleted or moved, and healing the
    /// fingerprints a migration left pending — then the walk of every
    /// watched folder. The walk only ever sees what the measure pass made
    /// legible.
    fn start_measure_pass(&mut self) -> Task<Message> {
        if self.verifying {
            return Task::none();
        }
        let addresses: Vec<String> =
            book::book_rows(&self.library.books).map(|b| b.path().to_string()).collect();
        if addresses.is_empty() {
            return self.run_watched();
        }
        self.verifying = true;
        Task::perform(async move { fs::check_paths(&addresses) }, Message::ChecksDone)
    }

    fn checks_done(&mut self, checks: Vec<PathCheck>) -> Task<Message> {
        self.verifying = false;
        let mut changed = false;
        for check in &checks {
            if !book::apply_check(&mut self.library.books, check).is_empty() {
                changed = true;
            }
        }
        let walks = self.run_watched();
        if changed {
            Task::batch([self.persist_library(), walks])
        } else {
            walks
        }
    }

    /// The walk of every folder that owes one. Held back while a migrated
    /// book still wears a placeholder fingerprint — scanning against
    /// unmeasured identities would re-add every one of them — and while the
    /// focus is the app's own picker closing.
    fn run_watched(&mut self) -> Task<Message> {
        if self.library.awaiting_check() || dialogs::picker_focus() {
            return Task::none();
        }
        let watched: Vec<(String, FolderOpts)> = self
            .library
            .folders
            .iter()
            // Watched anywhere, not only at the root: a tree turned off at
            // the root with one subfolder still on owes the walk, and the
            // ledger's per-rung gate keeps the off rungs quiet inside it.
            .filter(|folder| folder.owes_walk())
            .map(|folder| (folder.root.clone(), folder.opts.clone()))
            .collect();
        let mut walks = Vec::with_capacity(watched.len());
        for (root, opts) in watched {
            walks.push(self.begin_folder_walk(PathBuf::from(root), opts, Asked::OnFocus));
        }
        Task::batch(walks)
    }

    // ── The folder run ──────────────────────────────────────────────────

    /// Kick off a folder walk: one run id, one channel, one subscription
    /// that lives exactly as long as the run. Two walks of one folder are
    /// two snapshots of the same ledger row and two writes back, so the
    /// root is claimed for the length of the run — an ask outranks a
    /// rescan and queues behind it, and a second ask is told the first is
    /// still running.
    fn begin_folder_walk(&mut self, dir: PathBuf, opts: FolderOpts, asked: Asked) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        match self.root_claim(&root) {
            Claim::Free => {}
            Claim::HeldByWalk => {
                if asked == Asked::Explicitly {
                    self.queued_ask = Some((dir, opts));
                }
                return Task::none();
            }
            Claim::HeldByAsk => {
                if asked == Asked::Explicitly {
                    self.toasts.show(
                        Tone::Info,
                        format!("{} is already being imported.", paths::dir_label(&root)),
                        Instant::now(),
                    );
                }
                return Task::none();
            }
        }
        let label = paths::dir_label(&root);
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label,
            sink: Arc::clone(&sink),
            rx,
            latest: None,
            stage: Stage::Walking { root: root.clone(), opts: opts.clone(), asked },
        });
        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::ScanDone(task, result),
        )
    }

    /// Who holds a root right now, if anyone — the runs and the queued ask
    /// behind them.
    fn root_claim(&self, root: &str) -> Claim {
        if self
            .queued_ask
            .as_ref()
            .is_some_and(|(dir, _)| dir.to_string_lossy() == root)
        {
            return Claim::HeldByAsk;
        }
        for run in &self.runs {
            let holds = match &run.stage {
                Stage::Walking { root: held, .. } => held == root,
                Stage::Storing { plan } => plan.folder.root == root,
                Stage::Measuring { .. }
                | Stage::Copying { .. }
                | Stage::Restoring { .. }
                | Stage::RestoreCopying { .. }
                | Stage::Duplicating { .. }
                | Stage::Departing { .. } => false,
            };
            if !holds {
                continue;
            }
            let focus_walk = match &run.stage {
                Stage::Walking { asked, .. } => *asked == Asked::OnFocus,
                Stage::Storing { plan } => plan.asked == Asked::OnFocus,
                Stage::Measuring { .. }
                | Stage::Copying { .. }
                | Stage::Restoring { .. }
                | Stage::RestoreCopying { .. }
                | Stage::Duplicating { .. }
                | Stage::Departing { .. } => false,
            };
            return if focus_walk { Claim::HeldByWalk } else { Claim::HeldByAsk };
        }
        Claim::Free
    }

    /// A run ended: the ask queued behind its root, if one waited, starts
    /// now — the release the queued ask was promised.
    fn release_root(&mut self, root: &str) -> Task<Message> {
        match self.queued_ask.take() {
            Some((dir, opts)) if dir.to_string_lossy() == root => {
                self.begin_folder_walk(dir, opts, Asked::Explicitly)
            }
            Some(other) => {
                self.queued_ask = Some(other);
                Task::none()
            }
            None => Task::none(),
        }
    }

    /// The walk's answer arrived: plan it against the ledger, and either
    /// land it (the books stay at place) or hand the additions to the store
    /// first (the books are copied).
    fn scan_done(
        &mut self,
        task: u64,
        result: Result<Vec<FoundFile>, String>,
        now: Instant,
    ) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let (root, opts, asked) = match &self.runs[ix].stage {
            Stage::Walking { root, opts, asked } => (root.clone(), opts.clone(), *asked),
            _ => return Task::none(),
        };
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.runs.remove(ix);
                // A quiet walk that could not read its folder leaves no
                // trace; an ask answers with the advice.
                if asked == Asked::Explicitly {
                    self.toasts.show(Tone::Error, error, now);
                }
                return self.release_root(&root);
            }
        };
        self.plan_folder_walk(ix, task, &root, opts, asked, found)
    }

    /// The diff stage: everything between the walk's raw findings and the
    /// landing. Decides against the ledger the crate owns — tombstones
    /// first, then the registry, then the rung's tracking answer — and
    /// writes nothing but the folder's own row.
    #[allow(clippy::too_many_lines)]
    fn plan_folder_walk(
        &mut self,
        ix: usize,
        task: u64,
        root: &str,
        opts: FolderOpts,
        asked: Asked,
        found: Vec<FoundFile>,
    ) -> Task<Message> {
        let stamp = now_ms();
        // Importing a folder the library already holds continues that row's
        // `placed` and `ignored` sets — the whole point of them: re-importing
        // is how a reader would otherwise get back every book they deleted
        // last week.
        let standing = self.library.folders.iter().find(|folder| folder.root == root).cloned();
        let mut folder = standing.clone().unwrap_or_else(|| {
            WatchedFolder::new(library_core::id::next_folder_id(stamp), root, opts.clone())
        });
        folder.opts = opts;
        if folder.mode().reads_in_place() {
            if standing.is_none() {
                // A fresh row's watch is the sheet's switch, at the root;
                // `set_tracking` keeps the tree and the legacy flag agreed.
                folder.set_tracking("", folder.opts.watch);
            } else {
                // A continuation keeps the tree's own answers.
                folder.opts.watch = folder.tracking.tracked();
            }
        }
        folder.prune_shelf_map(&self.library.shelves);

        let registry = ledger::registry_of(&self.library.books);
        ledger::prune_tombstones(&mut folder, &registry);
        // Written on every scan, including one that changes nothing: the
        // restore menu's "moved out of this folder" answer is only as fresh
        // as the last walk.
        folder.record_seen(&found);

        let actions = match asked {
            Asked::OnFocus => ledger::diff_folder(&folder, &registry, &found),
            Asked::Explicitly => ledger::diff_import(&folder, &registry, &found),
        };
        let mut adds: Vec<FoundFile> = Vec::new();
        let mut relinks: Vec<(String, String)> = Vec::new();
        for action in actions {
            match action {
                ScanAction::Add(file) => adds.push(file),
                ScanAction::Relink { book_id, to } => relinks.push((book_id, to)),
                ScanAction::Skip => {}
            }
        }
        ledger::keep_healable_relinks(&mut relinks, &self.library.books);
        // One book per fingerprint per scan: two byte-identical files are
        // one book, and copying both would leave an orphan in the store
        // nothing can remove.
        let mut seen: HashSet<Fingerprint> = HashSet::new();
        adds.retain(|file| seen.insert(file.fp));

        let root_name = paths::dir_label(root);
        if adds.is_empty() && relinks.is_empty() {
            // Nothing to do. A quiet walk leaves no trace beyond the row's
            // stamp; an ask still owes the reader the news.
            folder.scanned_ms = stamp;
            let fresh = standing.is_none();
            write_folder_row(&mut self.library.folders, folder);
            let persist = if fresh || asked == Asked::Explicitly {
                self.persist_library()
            } else {
                Task::none()
            };
            self.runs.remove(ix);
            if asked == Asked::Explicitly {
                let line = if found.is_empty() {
                    format!("No documents found in “{root_name}”")
                } else {
                    format!("Everything in “{root_name}” is already in the library")
                };
                self.toasts.show(Tone::Info, line, Instant::now());
            }
            return Task::batch([persist, self.release_root(root)]);
        }

        // The root rung's name, deduped against the shelves standing — two
        // doors of one name are two doors a reader cannot tell apart. A
        // continuation keeps the shelf the map already names.
        let planned_root = (!folder.shelf_map.contains_key("")).then(|| {
            let names: HashSet<String> =
                self.library.shelves.iter().map(|each| each.name.clone()).collect();
            if names.contains(&root_name) {
                book::duplicate_title(&root_name, &names)
            } else {
                root_name.clone()
            }
        });
        let adds: Vec<(String, FoundFile)> = adds
            .into_iter()
            .map(|file| (library_core::id::next_id(stamp), file))
            .collect();
        let plan = WalkPlan { folder, asked, root_name, planned_root, adds, relinks };

        if plan.folder.mode().copies_files() {
            // The store batch rides the run's own channel: the pill the
            // scan lit keeps counting, in its copy phase.
            let requests: Vec<BookFileRequest> = plan
                .adds
                .iter()
                .map(|(id, file)| BookFileRequest { from: file.path.clone(), id: id.clone() })
                .collect();
            let sink = Arc::clone(&self.runs[ix].sink);
            let task_name = task.to_string();
            // The scan's last beat is stale the moment the stage turns:
            // the pill waits on the copy phase's own first beat.
            self.runs[ix].latest = None;
            self.runs[ix].stage = Stage::Storing { plan: Box::new(plan) };
            Task::perform(
                async move { store::store_books(&task_name, &requests, &sink) },
                move |results| Message::CopiesDone(task, results),
            )
        } else {
            self.runs.remove(ix);
            let landed = self.land_folder_walk(plan, None);
            Task::batch([landed, self.release_root(root)])
        }
    }

    /// The store's answer for a folder import: land what copied, count what
    /// the store refused, and let the rest of the batch stand.
    fn copies_done(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        match run.stage {
            Stage::Storing { plan } => {
                let root = plan.folder.root.clone();
                let (copies, failure) = partition_store_results(results);
                let outcome = self.land_folder_walk(*plan, Some(copies));
                if let Some(error) = failure {
                    self.toasts.show(Tone::Error, error, Instant::now());
                }
                let release = self.release_root(&root);
                Task::batch([outcome, release])
            }
            Stage::Duplicating { work } => self.duplicates_done(*work, results),
            Stage::Departing { work } => self.departing_done(*work, results),
            _ => Task::none(),
        }
    }

    /// The store's answer for one duplicate run: land what came home, count
    /// it for the report, toast what the store refused, and let the queue
    /// walk on.
    fn duplicates_done(&mut self, work: DupWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let now = now_ms();
        let recorded = match work {
            DupWork::Book(copy) => copies.get(&copy.new_id).map(|(store, measured)| {
                let title = duplicate::land_book(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    &self.shelf,
                    *copy,
                    store.clone(),
                    *measured,
                    now,
                );
                Duplicated { name: title, shelf: false }
            }),
            DupWork::Tree(plan) => {
                let name = duplicate::land_tree(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    plan,
                    &copies,
                    now,
                );
                Some(Duplicated { name, shelf: true })
            }
        };
        if let Some(one) = recorded {
            self.dup_landed.push(one);
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        self.pump_dup()
    }

    /// The duplicate queue's step: one entry at a time, because each counter
    /// name counts against the level as the last landing left it — the web's
    /// own sequential loop, walked by messages instead of an async fn. The
    /// entries that owe no bytes land on the spot and the walk continues;
    /// the ones that do ride a run, and the queue resumes when its copies
    /// come home.
    fn pump_dup(&mut self) -> Task<Message> {
        while let Some(entry) = self.dup_queue.first().cloned() {
            self.dup_queue.remove(0);
            let now = now_ms();
            let plan =
                duplicate::plan_one(&self.library.books, &self.library.shelves, &entry, now);
            match plan {
                DupPlan::Skip => {}
                DupPlan::Dead(note) => {
                    self.toasts.show(Tone::Error, note, Instant::now());
                }
                DupPlan::ShelfLink { row_id, name, target } => {
                    let title = duplicate::land_shelf_link(
                        &mut self.library.books,
                        &mut self.library.shelves,
                        &self.shelf,
                        &row_id,
                        &name,
                        &target,
                        now,
                    );
                    self.dup_landed.push(Duplicated { name: title, shelf: false });
                }
                DupPlan::Book(copy) => {
                    let label = copy.shown.clone();
                    let requests = vec![BookFileRequest {
                        from: copy.book.path().to_string(),
                        id: copy.new_id.clone(),
                    }];
                    return self.begin_dup(label, requests, DupWork::Book(copy));
                }
                DupPlan::Tree(plan) => {
                    let requests = duplicate::tree_requests(&plan);
                    if requests.is_empty() {
                        // A tree of links and dead rows copies nothing: no
                        // card, and the landing is the whole run.
                        let name = duplicate::land_tree(
                            &mut self.library.books,
                            &mut self.library.shelves,
                            plan,
                            &HashMap::new(),
                            now,
                        );
                        self.dup_landed.push(Duplicated { name, shelf: true });
                        continue;
                    }
                    let label = plan.label.clone();
                    return self.begin_dup(label, requests, DupWork::Tree(plan));
                }
            }
        }
        // The queue ran out: the report the whole batch ends with, and the
        // one persist that covers it — a link-only batch, which never met a
        // run, lands here too.
        if self.dup_landed.is_empty() {
            return Task::none();
        }
        let report = duplicate::report(&self.dup_landed);
        self.dup_landed.clear();
        self.toasts.show(Tone::Info, report, Instant::now());
        self.persist_library()
    }

    /// A duplicate's store run: the card wears the name of what is being
    /// duplicated, the way a folder run wears its folder.
    fn begin_dup(
        &mut self,
        label: String,
        requests: Vec<BookFileRequest>,
        work: DupWork,
    ) -> Task<Message> {
        self.begin_store_run(label, requests, Stage::Duplicating { work: Box::new(work) })
    }

    /// One store batch of the reader's own asking: one card for the whole
    /// gesture, and the stage that lands when the copies come home.
    fn begin_store_run(
        &mut self,
        label: String,
        requests: Vec<BookFileRequest>,
        stage: Stage,
    ) -> Task<Message> {
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun { task, label, sink, rx, latest: None, stage });
        let emit = Arc::clone(&self.runs[self.runs.len() - 1].sink);
        let task_name = task.to_string();
        Task::perform(
            async move { store::store_books(&task_name, &requests, &emit) },
            move |results| Message::CopiesDone(task, results),
        )
    }

    /// The book half of a move, screened by the departure's gate: a
    /// read-at-place book leaving the ground that made it becomes the
    /// library's own stored copy, and a copy is a cost the reader agrees to
    /// before anything moves. True when the move — or a return's bind —
    /// wrote; a gated move writes nothing and waits in the sheet.
    fn gated_seat(
        &mut self,
        books: &[String],
        from: Option<String>,
        to: String,
        index: Option<usize>,
    ) -> bool {
        // A re-order leaves nothing behind — the rows are arriving where they
        // already are — and every other hand-move is screened by the one
        // departure rule, which reads the book's own rung and not the shelf
        // it happens to stand on.
        let arriving_elsewhere = match from.as_deref() {
            Some(from) => from != to,
            None => to != ALL_SHELF,
        };
        if arriving_elsewhere {
            let hand = RowMove::Seat { from: from.clone(), to: to.clone(), index };
            if self.ask_move_copy(books, &to, hand) {
                return false;
            }
        }
        let mut wrote = library::arrange::move_many_to_shelf(
            &mut self.library.shelves,
            &mut self.library.books,
            books,
            from.as_deref(),
            &to,
            index,
        );
        // Every stored book the move lands can bind a folder's moved-out log
        // as a return: the address they share is the bind.
        for id in books {
            wrote |= departure::bind_returned(
                &self.library.books,
                &self.library.shelves,
                &mut self.library.folders,
                id,
                &to,
            );
        }
        wrote
    }

    /// The lift out of one shelf, screened the same way: to the library's own
    /// floor is a departure for every read-at-place book a folder placed. No
    /// bind on this door — a lift out arrives nowhere a log could name.
    fn gated_unfile(&mut self, books: &[String], shelf: &str) -> bool {
        let hand = RowMove::Unfile { shelf: shelf.to_string() };
        if self.ask_move_copy(books, ALL_SHELF, hand) {
            return false;
        }
        library::arrange::unfile_books(&mut self.library.shelves, books, shelf)
    }

    /// The gate every hand-move rides: a row that reads in place and is
    /// leaving the ground that made it becomes the library's own stored
    /// copy, and a copy is a question. True means the move waits on the
    /// sheet.
    fn ask_move_copy(&mut self, ids: &[String], to: &str, hand: RowMove) -> bool {
        let converting =
            departure::converting_rows(&self.library.books, &self.library.folders, ids, to);
        let Some(ask) =
            departure::ask_of_rows(&self.library.books, &self.library.folders, &converting, hand)
        else {
            return false;
        };
        self.sheet = Some(Sheet::Copy { ask });
        true
    }

    /// The sheet's "Copy and move": the copies ride a store batch of their
    /// own — one card for the whole gesture, the way fifty books leaving
    /// their ground are one thing the reader asked for — and the interrupted
    /// move resumes when they come home.
    fn copy_and_finish(&mut self, ask: CopyAsk) -> Task<Message> {
        let CopyWork::Rows { ids, hand } = ask.work;
        // The answer screens again, because the sheet was up while the
        // library went on living.
        let converting =
            departure::converting_rows(&self.library.books, &self.library.folders, &ids, hand.to());
        let requests: Vec<BookFileRequest> = converting
            .iter()
            .filter_map(|id| {
                let book = book::find_row(&self.library.books, id)?.book()?;
                Some(BookFileRequest { from: book.path().to_string(), id: id.clone() })
            })
            .collect();
        if requests.is_empty() {
            // Nothing left to copy: the rows the gate named went while the
            // sheet was up, and the gesture resumes without them — a copy
            // that failed costs that book its move and nothing else.
            let rest: Vec<String> =
                ids.into_iter().filter(|id| !converting.contains(id)).collect();
            let moved =
                if rest.is_empty() { false } else { self.resume_move(hand, rest, Vec::new()) };
            return if moved { self.persist_library() } else { Task::none() };
        }
        let label = match converting.len() {
            1 => book::find_row(&self.library.books, &converting[0])
                .map(|row| row.display_name())
                .unwrap_or_else(|| "1 book".to_string()),
            n => format!("{n} books"),
        };
        let work = DepartWork { ids, converting, hand };
        self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) })
    }

    /// The store's answer for a departure: the copies become the library's
    /// own — the moved-out log first, because the tombstone wears the book's
    /// ORIGINAL fingerprint — and the interrupted gesture resumes with what
    /// came home.
    fn departing_done(&mut self, work: DepartWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let now = now_ms();
        let mut departed: Vec<String> = Vec::new();
        for id in &work.converting {
            let Some((store, measured)) = copies.get(id) else { continue };
            let Some(book) = book::find_by_id(&self.library.books, id) else { continue };
            let path = book.path().to_string();
            departure::write_moved_stones(
                &mut self.library.folders,
                &self.library.shelves,
                book,
                None,
                now,
            );
            if let Some(book) = book::find_book_mut(&mut self.library.books, id) {
                book.become_stored(&path, store.clone(), *measured);
            }
            departed.push(id.clone());
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        // A copy that failed costs that book its move and nothing else: it
        // stays where it was, and every screen downstream sees the books as
        // what they are about to be.
        let failed: HashSet<String> = work
            .converting
            .iter()
            .filter(|id| !departed.contains(*id))
            .cloned()
            .collect();
        let rest: Vec<String> = work.ids.into_iter().filter(|id| !failed.contains(id)).collect();
        let any_copies = !departed.is_empty();
        let moved =
            if rest.is_empty() { false } else { self.resume_move(work.hand, rest, departed) };
        if moved || any_copies {
            return self.persist_library();
        }
        Task::none()
    }

    /// The gesture a copy question interrupted, finished: the same rows land,
    /// and the copies land marked — a departure is not a return, so a copied
    /// book binds no folder's moved-out log.
    fn resume_move(&mut self, hand: RowMove, ids: Vec<String>, departed: Vec<String>) -> bool {
        match hand {
            RowMove::Seat { from, to, index } => {
                let mut wrote = library::arrange::move_many_to_shelf(
                    &mut self.library.shelves,
                    &mut self.library.books,
                    &ids,
                    from.as_deref(),
                    &to,
                    index,
                );
                for id in &ids {
                    if !departed.contains(id) {
                        wrote |= departure::bind_returned(
                            &self.library.books,
                            &self.library.shelves,
                            &mut self.library.folders,
                            id,
                            &to,
                        );
                    }
                }
                wrote
            }
            RowMove::Row { to, index } => {
                // One row's own move: off every shelf it was on and onto the
                // one named. The single-row door arrives with the conflict
                // sheet; the resume answers it from the first day.
                let mut wrote = false;
                for id in &ids {
                    shelf::forget_everywhere(&mut self.library.shelves, id);
                    if to == ALL_SHELF {
                        if index.is_some() {
                            wrote |= library::arrange::reorder_root(
                                &mut self.library.books,
                                std::slice::from_ref(id),
                                index,
                            );
                        }
                    } else if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &to) {
                        shelf::place(&mut shelf.books, id, index);
                        wrote = true;
                    }
                    if !departed.contains(id) {
                        wrote |= departure::bind_returned(
                            &self.library.books,
                            &self.library.shelves,
                            &mut self.library.folders,
                            id,
                            &to,
                        );
                    }
                }
                wrote
            }
            RowMove::Unfile { shelf } => {
                library::arrange::unfile_books(&mut self.library.shelves, &ids, &shelf)
            }
        }
    }

    /// The diff's answer, written to the live lists: relinks first, then
    /// the mints — each wearing the shelf its rung names, minting the whole
    /// chain between the folder's root and the file's own subfolder — then
    /// the rehang the fresh map owes, the row written back whole, and the
    /// news the reader is told.
    #[allow(clippy::too_many_lines)]
    fn land_folder_walk(
        &mut self,
        plan: WalkPlan,
        copies: Option<CopyMap>,
    ) -> Task<Message> {
        let WalkPlan { mut folder, asked, root_name, planned_root, adds, relinks, .. } = plan;
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
            // A file at an address the library already reads is that book,
            // whatever the two fingerprints say: a migrated row's
            // placeholder identity is healed by the ground it stands on.
            if copies.is_none()
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
            let placed_id = book::add_book(&mut self.library.books, minted);
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

        let total = placed + relinked + healed;
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
        persist
    }

    // ── The loose-file run ──────────────────────────────────────────────

    /// The measurement arrived: heal and mark what it measured, answer each
    /// file by the ground it stands on, and queue the rest for the store —
    /// loose files land as the library's own copies, because no folder
    /// rescans them and a linked row no ledger answers for is a row no rule
    /// can keep honest.
    #[allow(clippy::too_many_lines)]
    fn files_checked(&mut self, task: u64, checks: Vec<PathCheck>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let Stage::Measuring { target } = &mut self.runs[ix].stage else {
            return Task::none();
        };
        let target = target.take();

        let mut changed = false;
        for check in &checks {
            if !book::apply_check(&mut self.library.books, check).is_empty() {
                changed = true;
            }
        }

        let found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
        if found.is_empty() {
            self.runs.remove(ix);
            self.toasts.show(
                Tone::Error,
                "None of those files could be opened.",
                Instant::now(),
            );
            return if changed { self.persist_library() } else { Task::none() };
        }

        let stamp = now_ms();
        let mut already = 0usize;
        let mut restored = 0usize;
        let mut pending: Vec<PendingCopy> = Vec::new();

        for file in found {
            // A file a read-at-place tree holds is that folder's business
            // first: a row already reading the address is counted and filed
            // here, and a removal the tree logged comes back as the tree's
            // own linked book.
            let covering = self
                .library
                .folders
                .iter()
                .filter(|folder| folder.mode().reads_in_place())
                .find(|folder| rel_under(&file.path, &folder.root).is_some())
                .map(|folder| folder.id.clone());
            if let Some(folder_id) = covering {
                let held_at_address = book::book_rows(&self.library.books)
                    .find(|each| each.path() == file.path)
                    .map(|each| each.id.clone());
                if let Some(row_id) = held_at_address {
                    already += 1;
                    if let Some(target) = &target
                        && let Some(home) = shelf::find_mut(&mut self.library.shelves, target)
                    {
                        shelf::shelf_add(home, &row_id);
                    }
                    changed = true;
                    continue;
                }
                let stone = folder_ops::find(&self.library.folders, &folder_id)
                    .and_then(|folder| ledger::find_tombstone(folder, &file.fp).cloned());
                let placed_by_folder = folder_ops::find(&self.library.folders, &folder_id)
                    .is_some_and(|folder| folder.placed.contains(&file.fp));
                if stone.is_some() || placed_by_folder {
                    self.restore_covered(&file, &folder_id, stone.as_ref(), stamp);
                    restored += 1;
                    changed = true;
                    continue;
                }
            }
            // Content the library already holds is content the reader
            // already has, wherever it is filed: counted, and filed here
            // when a shelf is standing. A row whose address died is no
            // answer — the copy lands beside it. (The conflict sheet's
            // questions land with the duplicates work; until then the
            // placement is the answer.)
            if let Some(held) = ledger::existing_for(&self.library.books, file.fp)
                && !held.missing
            {
                already += 1;
                if let Some(target) = &target
                    && let Some(home) = shelf::find_mut(&mut self.library.shelves, target)
                {
                    shelf::shelf_add(home, &held.row_id);
                }
                changed = true;
                continue;
            }
            // A removal any folder logged against this content is spent by
            // the explicit ask: the name it remembered rides onto the copy.
            let title = self.lift_stone_for(&file.fp);
            pending.push(PendingCopy {
                book_id: library_core::id::next_id(stamp),
                file,
                title,
            });
        }

        if pending.is_empty() {
            self.runs.remove(ix);
            if restored > 0 {
                self.toasts.show(
                    Tone::Info,
                    format!("{} came back", lib_text::plural(restored, "book", "books")),
                    Instant::now(),
                );
            } else if already > 0 {
                self.toasts.show(Tone::Info, "Already in the library", Instant::now());
            }
            return if changed { self.persist_library() } else { Task::none() };
        }

        let requests: Vec<BookFileRequest> = pending
            .iter()
            .map(|item| BookFileRequest { from: item.file.path.clone(), id: item.book_id.clone() })
            .collect();
        let sink = Arc::clone(&self.runs[ix].sink);
        let task_name = task.to_string();
        self.runs[ix].stage = Stage::Copying {
            plan: Box::new(FilesPlan { target, pending, already, restored }),
        };
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::FilesCopied(task, results),
        )
    }

    /// The folder's book comes back the way a folder walk brings it back —
    /// a linked book at the file's address, wearing the name the log
    /// remembered, on the log's shelf when it still stands — and the
    /// landing spends the log.
    fn restore_covered(
        &mut self,
        file: &FoundFile,
        folder_id: &str,
        stone: Option<&Tombstone>,
        stamp: u64,
    ) {
        let mut minted = Book::new(
            library_core::id::next_id(stamp),
            file.fp,
            file.admitted_format(),
            Origin::Linked { src: file.path.clone() },
            stamp,
        );
        minted.title = stone.and_then(|entry| entry.title.clone());
        let placed_id = book::add_book(&mut self.library.books, minted);
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &file.fp);
            folder.mark_placed(file.fp);
        }
        // The log's shelf when it stands, else the folder's root rung, else
        // the file stays unfiled — the order the walk's own restore keeps.
        let home = stone
            .and_then(|entry| entry.shelf_id.clone())
            .or_else(|| {
                folder_ops::find(&self.library.folders, folder_id)
                    .and_then(|folder| folder.shelf_map.get("").cloned())
            })
            .filter(|id| shelf::find(&self.library.shelves, id).is_some());
        if let Some(home) = home
            && let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &home)
        {
            shelf::shelf_add(shelf, &placed_id);
        }
    }

    /// Spend the removal that was holding this content out of any folder's
    /// walk, and answer with the name it remembered. The match is the
    /// fingerprint, not the address: a file removed from a folder, moved
    /// across the disk and dropped back into the library is the same file
    /// the log was written for.
    fn lift_stone_for(&mut self, fp: &Fingerprint) -> Option<String> {
        let owner = self
            .library
            .folders
            .iter()
            .find(|folder| ledger::find_tombstone(folder, fp).is_some())
            .map(|folder| folder.id.clone())?;
        let folder = folder_ops::find_mut(&mut self.library.folders, &owner)?;
        ledger::restore_deleted(folder, fp).and_then(|stone| stone.title)
    }

    /// The store's answer for the loose files: land the copies that came
    /// home, each wearing its own measurement, on the level the pick named.
    fn files_copied(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        let Stage::Copying { plan } = run.stage else {
            return Task::none();
        };
        let FilesPlan { target, pending, already, restored } = *plan;
        let (copies, failure) = partition_store_results(results);
        let stamp = now_ms();

        let mut landed = 0usize;
        let mut placements: Vec<String> = Vec::new();
        for item in pending {
            let Some((store_path, measured)) = copies.get(&item.book_id) else {
                continue;
            };
            // A row already reading the source address makes this copy its
            // own book: independent, with its own marks and place.
            let independent =
                book::book_rows(&self.library.books).any(|each| each.path() == item.file.path);
            let mut minted = Book::new(
                item.book_id,
                item.file.fp,
                item.file.admitted_format(),
                Origin::Stored { src: Some(item.file.path.clone()), store: store_path.clone() },
                stamp,
            );
            minted.title = item.title;
            minted.independent = independent;
            minted.adopt_measurement(*measured);
            let placed_id = minted.id.clone();
            self.library.books.push(library_core::book::Row::Book(minted));
            placements.push(placed_id);
            landed += 1;
        }
        if let Some(target) = &target
            && let Some(home) = shelf::find_mut(&mut self.library.shelves, target)
        {
            for placed_id in &placements {
                shelf::place(&mut home.books, placed_id, None);
            }
        }

        let added = landed + restored;
        if added > 0 || already > 0 {
            let persist = self.persist_library();
            if let Some(error) = failure {
                self.toasts.show(Tone::Error, error, Instant::now());
            } else if added > 0 {
                let line = if target.is_some() {
                    format!("Added {} to this shelf", lib_text::plural(added, "book", "books"))
                } else {
                    format!("Added {}", lib_text::plural(added, "book", "books"))
                };
                self.toasts.show(Tone::Info, line, Instant::now());
            } else {
                self.toasts.show(Tone::Info, "Already in the library", Instant::now());
            }
            return persist;
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        Task::none()
    }

    /// The folder the level on screen belongs to, when it is a watched
    /// folder's shelf — the add menu's in-folder doors answer for it.
    fn standing_folder_id(&self) -> Option<String> {
        if self.shelf == ALL_SHELF {
            return None;
        }
        shelf::find(&self.library.shelves, &self.shelf)
            .and_then(|shelf| shelf.kind.folder_id().map(str::to_string))
    }

    /// What this folder could give the reader back: the ledger's
    /// recoverables, pure and synchronous — the menu opens on a click and
    /// answers from the last walk's log, not from a directory tree.
    fn restore_candidates(&self) -> Vec<Recovered> {
        let Some(folder_id) = self.standing_folder_id() else {
            return Vec::new();
        };
        let Some(folder) = folder_ops::find(&self.library.folders, &folder_id) else {
            return Vec::new();
        };
        let index = ledger::index_by_fp(&self.library.books);
        ledger::recoverables(folder, &index, &self.library.shelves)
    }

    /// The gone check behind the add menu's Restore section: a removed
    /// book's file may have left the disk since the walk logged it, and a
    /// row that promises a file that is not there would land an error
    /// where the menu could have shown the truth. Dispatched when the
    /// panel opens; the rows answer disabled as the check arrives.
    fn check_restore_paths(&mut self) -> Task<Message> {
        self.restore_gone.clear();
        let addresses: Vec<String> = self
            .restore_candidates()
            .into_iter()
            .filter_map(|item| match item {
                Recovered::Deleted(entry) => Some(entry.last_path),
                Recovered::Moved { .. } => None,
            })
            .collect();
        if addresses.is_empty() {
            return Task::none();
        }
        Task::perform(async move { fs::check_paths(&addresses) }, Message::RestoreChecked)
    }

    /// The add menu's folder-side facts: the in-folder picker's door, the
    /// Restore section's rows, and the confirm face when a Moved row was
    /// asked. The labels are computed here — the menu lays out, the app
    /// knows.
    fn add_facts(&self) -> menus::AddFacts {
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

    /// The Restore section's removed row: measure the one file the log
    /// remembered, then land it back the way the folder holds its books.
    /// A restore re-measures before it promises — the file on disk, not
    /// the log, has the last word.
    fn restore_deleted(&mut self, folder_id: String, fp: Fingerprint) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        let Some((opts, stone)) = folder_ops::find(&self.library.folders, &folder_id).and_then(
            |folder| {
                ledger::find_tombstone(folder, &fp)
                    .cloned()
                    .map(|stone| (folder.opts.clone(), stone))
            },
        ) else {
            return Task::none();
        };
        let address = stone.last_path.clone();
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.runs.push(FsRun {
            task,
            label: stone.label(),
            sink,
            rx,
            latest: None,
            stage: Stage::Restoring { folder_id, stone: Box::new(stone), opts },
        });
        Task::perform(
            async move { fs::check_paths(std::slice::from_ref(&address)) },
            move |checks| Message::RestoreMeasured(task, checks),
        )
    }

    /// The restore's measurement answered: a file that is not there any
    /// more is an error the reader hears; a file that is there lands
    /// linked when the tree reads at place, and goes through the store
    /// when it does not.
    fn restore_measured(&mut self, task: u64, checks: Vec<PathCheck>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let mut run = self.runs.remove(ix);
        let Stage::Restoring { folder_id, stone, opts } = &run.stage else {
            return Task::none();
        };
        let (folder_id, stone, opts) = (folder_id.clone(), (**stone).clone(), opts.clone());
        let Some(found) = checks.first().and_then(found_from_check) else {
            self.toasts.show(
                Tone::Error,
                format!("{} is not there any more.", stone.label()),
                Instant::now(),
            );
            return Task::none();
        };
        let book_id = library_core::id::next_id(now_ms());
        if opts.mode().reads_in_place() {
            return self.land_restore(&folder_id, stone, &opts, found, book_id, None);
        }
        let requests = vec![BookFileRequest { from: found.path.clone(), id: book_id.clone() }];
        let sink = Arc::clone(&run.sink);
        let task_name = task.to_string();
        run.stage = Stage::RestoreCopying {
            folder_id,
            stone: Box::new(stone),
            opts,
            found: Box::new(found),
            book_id,
        };
        self.runs.push(run);
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::RestoreCopied(task, results),
        )
    }

    /// The restore's copy answered: land the stored book with its own
    /// measurement, or tell the reader the copy did not come home.
    fn restore_copied(&mut self, task: u64, results: Vec<StoreResult>) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let run = self.runs.remove(ix);
        let Stage::RestoreCopying { folder_id, stone, opts, found, book_id } = run.stage else {
            return Task::none();
        };
        let (copies, failure) = partition_store_results(results);
        let measured = copies.get(&book_id).cloned();
        if let Some(error) =
            failure.or_else(|| measured.is_none().then(|| "The copy did not land.".to_string()))
        {
            self.toasts.show(Tone::Error, error, Instant::now());
            return Task::none();
        }
        self.land_restore(&folder_id, *stone, &opts, *found, book_id, measured)
    }

    /// The restore's landing: the book comes back wearing the name the log
    /// remembered and the measurement its own bytes answered with, the log
    /// is spent, the file is marked placed, and the book is filed on the
    /// shelf the log remembered when that shelf still stands — else on the
    /// folder's root rung, else nowhere. A file that changed since the
    /// removal spends the changed address's log too, so the next walk does
    /// not give the book back a second time.
    fn land_restore(
        &mut self,
        folder_id: &str,
        stone: Tombstone,
        opts: &FolderOpts,
        found: FoundFile,
        book_id: String,
        measured: Option<Stored>,
    ) -> Task<Message> {
        let stamp = now_ms();
        let origin = match &measured {
            Some((store_path, _)) => {
                Origin::Stored { src: Some(found.path.clone()), store: store_path.clone() }
            }
            None => Origin::Linked { src: found.path.clone() },
        };
        let mut minted = Book::new(book_id, found.fp, found.admitted_format(), origin, stamp);
        minted.title = stone.title.clone();
        if opts.mode().copies_files() {
            minted.adopt_measurement(measured.as_ref().and_then(|(_, measure)| *measure));
        }
        let label = stone.label();
        let placed_id = book::add_book(&mut self.library.books, minted);
        if let Some(folder) = folder_ops::find_mut(&mut self.library.folders, folder_id) {
            ledger::restore_deleted(folder, &stone.fp);
            folder.mark_placed(found.fp);
            if found.fp != stone.fp {
                folder.ignored.retain(|entry| entry.fp != found.fp);
            }
        }
        let home = stone
            .shelf_id
            .clone()
            .or_else(|| {
                folder_ops::find(&self.library.folders, folder_id)
                    .and_then(|folder| folder.shelf_map.get("").cloned())
            })
            .filter(|id| shelf::find(&self.library.shelves, id).is_some());
        if let Some(home) = home
            && let Some(shelf) = shelf::find_mut(&mut self.library.shelves, &home)
        {
            shelf::shelf_add(shelf, &placed_id);
        }
        self.toasts.show(Tone::Info, format!("“{label}” came back"), Instant::now());
        self.persist_library()
    }

    /// One book, two memberships, nothing copied: the folder's ledger is
    /// untouched — the book stays placed where it was placed, which keeps
    /// the next rescan quiet about it.
    fn also_show(&mut self, book_id: &str, shelf_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        let Some(home) = shelf::find_mut(&mut self.library.shelves, shelf_id) else {
            return Task::none();
        };
        shelf::shelf_add(home, book_id);
        self.persist_library()
    }

    /// The confirm face's second answer: close the menu and take the
    /// reader to the shelf the book is on — the first membership in shelf
    /// order, or the library's own floor when it is on none.
    fn go_and_look(&mut self, book_id: &str) -> Task<Message> {
        self.menu = None;
        self.menu_confirm = None;
        self.context = None;
        self.shelf = shelf::containing(&self.library.shelves, book_id)
            .first()
            .map(|shelf| shelf.id.clone())
            .unwrap_or_else(|| ALL_SHELF.to_string());
        Task::none()
    }

    /// Tell the view what auto-fit measured, and persist it when it moved —
    /// the stepper's `+` starts from what the shelf shows.
    fn report_auto_fit(&mut self) {
        if library::report_fit(&mut self.library.view, self.viewport.width) {
            let _ = self.persist_library();
        }
    }

    fn apply_appearance(&mut self) {
        let base = self.settings.appearance.base;
        self.tokens = Tokens::for_base(base);
        self.theme = theme::build(self.tokens, base);
    }

    /// Settings save at the moment of change, atomically; a failure is the
    /// toast slot's business, and the in-memory copy stays authoritative.
    fn persist_settings(&mut self) -> Task<Message> {
        if let Err(error) = storage::save_settings(&self.settings) {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        Task::none()
    }

    /// The library's own save contract: the same moment-of-change rule.
    fn persist_library(&mut self) -> Task<Message> {
        if let Err(error) = storage::save_library(&self.library) {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let now = Instant::now();
        let factor = self.titlebar.factor(now);
        // The drag's standing answer, recomputed per frame: the effect the
        // cells dress for and the fold preview the ghost would wear. The
        // table itself answers None when no drag is in flight.
        let answer = self.drag_answer();
        // The breadcrumb's fold, decided against the bar's estimated
        // budget: the same arithmetic the web bar ran against its
        // measured one.
        let plan = (self.route == Route::Library)
            .then(|| fold::plan(&self.library, &self.shelf, self.crumb_avail()));

        let content: Element<'_, Message> = match self.route {
            Route::Library => library::view(
                self.tokens,
                &self.library,
                &self.shelf,
                &self.query,
                self.hovered_card.as_deref(),
                self.viewport.width,
                library::SelectionFacts { selecting: self.selecting, selected: &self.selected },
                library::DragFacts {
                    payload: self.drag.as_ref().map(|drag| &drag.payload),
                    effect: answer.as_ref().map(|(effect, _)| effect),
                },
            ),
            Route::Reader => reader_surface(self.tokens, self.open_document.as_deref()),
        };

        let mut layers: Vec<Element<'_, Message>> = vec![content];
        // An open panel: the scrim that closes it on any outside press, and
        // the panel itself placed at the pointer. The bar stays above the
        // scrim, so its trigger still toggles the panel shut.
        if self.menu.is_some() {
            layers.push(scrim());
            if let Some(panel) = self.menu_layer() {
                layers.push(panel);
            }
        }
        // A right-click menu: the same scrim-and-panel answer, placed where
        // the pointer asked.
        if self.context.is_some() {
            layers.push(scrim());
            if let Some(panel) = self.context_layer() {
                layers.push(panel);
            }
        }
        // The runs' live lines, while any are in flight.
        if self.route == Route::Library && !self.runs.is_empty() {
            layers.push(runs_dock(self.tokens, &self.runs));
        }
        // The fold's panel, while open: the elided chain packed into rows,
        // hanging under the ellipsis. The ghost rides above it — the web's
        // own lane order puts both fold lanes below the drag overlay.
        if self.ellipsis_open
            && let Some(plan) = plan.as_ref().filter(|plan| plan.split > 0)
            && let Some(panel) = self.ellipsis_layer(plan)
        {
            layers.push(panel);
        }
        // The selection's bar, while the mode is on. Bottom-right, where
        // the web ActionBar stood; the runs' dock holds the centre, so the
        // two never argue over one corner.
        if self.route == Route::Library && self.selecting {
            layers.push(self.select_bar());
        }
        // The drag's ghost rides above the level and its bars — what the
        // pointer carries, drawn at the pointer, until the release answers.
        // The titlebar stays above it: chrome is chrome, even mid-drag.
        if let Some(drag) = &self.drag {
            layers.push(ghost_layer(
                self.tokens,
                drag,
                &self.library,
                answer.as_ref().and_then(|(_, fold)| fold.as_ref()),
                self.cursor,
            ));
        }
        // A hidden bar is not in the tree at all: nothing to hit, nothing
        // to hover — the reveal band lives in the cursor subscription.
        if factor > 0.0 {
            layers.push(self.bar(factor, plan.as_ref()));
        }
        // A sheet covers the whole window, bar included: the shelf waits on
        // its answer.
        if self.sheet.is_some()
            && let Some(panel) = self.sheet_layer()
        {
            layers.push(panel);
        }
        if let Some(toast) = self.toasts.view(self.tokens) {
            layers.push(toast);
        }

        stack(layers).width(Length::Fill).height(Length::Fill).into()
    }

    /// The titlebar with the route's slots hung on it.
    fn bar(&self, factor: f32, plan: Option<&FoldPlan>) -> Element<'_, Message> {
        let (left, center, right) = match self.route {
            Route::Library => {
                let book_count = book::book_rows(&self.library.books).count();
                // The crumb under the drag, and only while one is live:
                // the hot fact the bar dresses and the sink counts.
                let hot_crumb =
                    if self.drag.is_some() { self.hovered_crumb.as_deref() } else { None };
                let view_trigger = button(icon(IconName::More, 15, fade(self.tokens.ink, factor)))
                    .padding(7.0)
                    .style(move |_, status| titlebar::ghost_button_style(self.tokens, factor, status))
                    .on_press(Message::ToggleMenu(MenuKind::View));
                let appearance =
                    button(icon(appearance_glyph(self.settings.appearance.base), 15, fade(self.tokens.ink, factor)))
                        .padding(7.0)
                        .style(move |_, status| {
                            titlebar::ghost_button_style(self.tokens, factor, status)
                        })
                        .on_press(Message::CycleAppearance);
                (
                    plan.map(|plan| {
                        bar::breadcrumb(
                            self.tokens,
                            plan,
                            factor,
                            self.renaming,
                            &self.rename_draft,
                            bar::CrumbFacts {
                                hot: hot_crumb,
                                ellipsis_open: self.ellipsis_open,
                            },
                        )
                    }),
                    Some(bar::search(self.tokens, book_count, &self.query, factor)),
                    vec![view_trigger.into(), appearance.into()],
                )
            }
            Route::Reader => (None, None, Vec::new()),
        };

        titlebar::view(
            &self.titlebar,
            titlebar::ViewContext {
                tokens: self.tokens,
                route: self.route,
                maximized: self.maximized,
                factor,
                title: self.route.title(),
                left,
                center,
                right,
                chrome: Message::Chrome,
            },
        )
    }

    /// The inset the cluster starts at: AppKit paints the traffic lights
    /// over the content on macOS, and the bar keeps clear of them.
    fn bar_left_inset(&self) -> f32 {
        match platform::os() {
            Os::Mac => platform::MACOS_LIGHTS_INSET + 8.0,
            _ => 8.0,
        }
    }

    /// The fold's budget: what the row leaves the left cluster once the
    /// right cluster's chrome and the centre pill's usable floor are
    /// reserved. The web bar measured this box live; the native bar
    /// estimates it — two route triggers and the pin, the OS's own
    /// captions, and the pill's floor — and the fold's arithmetic is the
    /// same.
    fn crumb_avail(&self) -> f32 {
        let right = match platform::os() {
            Os::Mac => 114.0,
            Os::Windows => 236.0,
            Os::Linux => 248.0,
        };
        (self.viewport.width - self.bar_left_inset() - right - CENTER_FLOOR).max(160.0)
    }

    /// Whether the pointer is inside the fold panel's box: the same
    /// estimate the placement rides — the packed size anchored under the
    /// ellipsis, clamped into the viewport.
    fn pointer_in_panel(&self) -> bool {
        let plan = fold::plan(&self.library, &self.shelf, self.crumb_avail());
        if plan.split == 0 {
            return false;
        }
        let budget = (self.viewport.width - 24.0).max(160.0);
        let size = bar::panel_size(&plan, budget);
        let at = popover::place(bar::ellipsis_anchor(self.bar_left_inset()), size, self.viewport);
        self.cursor.x >= at.x
            && self.cursor.x <= at.x + size.width
            && self.cursor.y >= at.y
            && self.cursor.y <= at.y + size.height
    }

    /// The fold's panel, placed: the popover's own clamp-and-place answer,
    /// anchored under the ellipsis.
    fn ellipsis_layer(&self, plan: &FoldPlan) -> Option<Element<'_, Message>> {
        // The panel's packing budget, straight off the web: the window's
        // width less its own air, floored so a sliver of a window still
        // gives every crumb a row.
        let budget = (self.viewport.width - 24.0).max(160.0);
        let hot = if self.drag.is_some() { self.hovered_crumb.as_deref() } else { None };
        let (panel, size) = bar::ellipsis_panel(self.tokens, plan, hot, budget);
        let at = popover::place(bar::ellipsis_anchor(self.bar_left_inset()), size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The open panel, clamped into the viewport at the pointer's last
    /// position.
    fn menu_layer(&self) -> Option<Element<'_, Message>> {
        let kind = self.menu?;
        let (panel, size) = match kind {
            MenuKind::Add => menus::add_menu(self.tokens, &self.add_facts()),
            MenuKind::View => menus::view_menu(self.tokens, &self.library.view),
            MenuKind::Shelf => menus::shelf_menu(self.tokens, &self.shelf),
        };
        let anchor = Point::new(self.cursor.x, platform::TITLE_BAR_H + 2.0);
        let at = popover::place(anchor, size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The right-click menu, clamped into the viewport where the pointer
    /// asked for it.
    fn context_layer(&self) -> Option<Element<'_, Message>> {
        let request = self.context.as_ref()?;
        let (panel, size): (Element<'static, Message>, Size) = match &request.target {
            ContextTarget::Row(id) => {
                let row = book::find_row(&self.library.books, id)?;
                menus::row_menu(self.tokens, row)
            }
            ContextTarget::Folder(id) => {
                let shelf = shelf::find(&self.library.shelves, id)?;
                menus::folder_menu(self.tokens, shelf, self.watch_facts(id))
            }
            ContextTarget::Selection => menus::selection_menu(self.tokens, self.selected.len()),
            ContextTarget::Level => {
                let anything = !library::level_rows(&self.library, &self.shelf, &self.query)
                    .is_empty()
                    || !library::level_folders(&self.library, &self.shelf, &self.query)
                        .is_empty();
                menus::level_menu(self.tokens, self.selecting, anything)
            }
        };
        let at = popover::place(request.at, size, self.viewport);
        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: at.y.max(0.0),
                    right: 0.0,
                    bottom: 0.0,
                    left: at.x.max(0.0),
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        )
    }

    /// The modal question in flight: the rename sheet or the remove sheet.
    fn sheet_layer(&self) -> Option<Element<'_, Message>> {
        let sheet_state = self.sheet.as_ref()?;
        let panel: Element<'_, Message> = match sheet_state {
            Sheet::Rename { draft, .. } => {
                let input = text_input("Name", draft)
                    .id(SHEET_INPUT)
                    .on_input(Message::SheetDraft)
                    .on_submit(Message::SheetSave)
                    .size(13)
                    .width(Length::Fill)
                    .padding(Padding { top: 7.0, right: 10.0, bottom: 7.0, left: 10.0 })
                    .style(move |_theme, _status| text_input::Style {
                        background: Background::Color(self.tokens.paper),
                        border: Border {
                            color: self.tokens.line,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        icon: self.tokens.muted,
                        placeholder: self.tokens.muted,
                        value: self.tokens.ink,
                        selection: self.tokens.accent_soft,
                    });
                sheet::panel(
                    self.tokens,
                    "Rename",
                    input.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Save", Message::SheetSave, false),
                    ],
                )
            }
            Sheet::Remove { name, .. } => {
                let body = text(format!(
                    "Remove “{name}” from the library? {}",
                    "The file on disk stays where it is."
                ))
                .size(13)
                .color(self.tokens.muted);
                sheet::panel(
                    self.tokens,
                    "Remove",
                    body.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
                    ],
                )
            }
            Sheet::RemoveMany { books, shelves } => {
                let mut parts: Vec<String> = Vec::new();
                if !books.is_empty() {
                    parts.push(lib_text::plural(books.len(), "book", "books"));
                }
                if !shelves.is_empty() {
                    parts.push(lib_text::plural(shelves.len(), "shelf", "shelves"));
                }
                let body = text(format!(
                    "Remove {} from the library? {}",
                    parts.join(" and "),
                    "The files on disk stay where they are."
                ))
                .size(13)
                .color(self.tokens.muted);
                sheet::panel(
                    self.tokens,
                    "Remove",
                    body.into(),
                    vec![
                        sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                        sheet::confirm_button(self.tokens, "Remove", Message::SheetSave, true),
                    ],
                )
            }
            Sheet::Import { root, ground } => sheet::panel_sized(
                self.tokens,
                sheet::IMPORT_W,
                "Import books",
                import_sheet(self.tokens, root, &self.import_opts, ground.as_ref()),
                vec![
                    sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                    sheet::confirm_button(self.tokens, "Import", Message::SheetSave, false),
                ],
            ),
            // The departure's question: the action as the heading, the
            // subject and the cost as the body's own lines, and one button
            // per answer the ask was raised with — Cancel always first, the
            // way the web sheet's footer stood them.
            Sheet::Copy { ask } => {
                let mut body = Column::new().spacing(6);
                body = body.push(text(ask.subject.clone()).size(13).color(self.tokens.muted));
                for line in &ask.lines {
                    body = body.push(text(line.clone()).size(12).color(self.tokens.muted));
                }
                let mut actions =
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)];
                for option in &ask.options {
                    actions.push(if option.primary {
                        sheet::confirm_button(
                            self.tokens,
                            &option.label,
                            Message::AnswerCopy(option.answer),
                            false,
                        )
                    } else {
                        sheet::cancel_button(
                            self.tokens,
                            &option.label,
                            Message::AnswerCopy(option.answer),
                        )
                    });
                }
                sheet::panel(self.tokens, &ask.action, body.into(), actions)
            }
        };
        Some(sheet::overlay(panel, Message::SheetCancel))
    }

    /// The selection's bar: the count, All, the shelf answer, Remove, and
    /// Done in a pill at the shelf's bottom-right — the ActionBar's shape,
    /// surface and hairline and fully round. The bar and its popover sit in
    /// mouse areas that capture and answer nothing, so a press on their
    /// chrome cannot fall through to the floor underneath and leave the
    /// mode the bar is for.
    fn select_bar(&self) -> Element<'_, Message> {
        let count = self.selected.len();
        let mut face = Column::new().spacing(8).align_x(Alignment::End);
        if self.select_pop {
            face = face
                .push(mouse_area(self.select_pop_panel()).on_press(Message::KeepSelection));
        }
        face = face.push(
            mouse_area(select_pill(self.tokens, count, self.select_pop))
                .on_press(Message::KeepSelection),
        );
        container(face)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 0.0 })
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .into()
    }

    /// The bar's "Add to shelf" panel: every shelf the WHOLE set may land
    /// on — one any chosen folder would end up inside itself on answers
    /// for none of them, and renders disabled rather than vanishing — and
    /// the door to a shelf minted for the occasion.
    fn select_pop_panel(&self) -> Element<'static, Message> {
        let (_, folder_ids) = self.split_selection();
        let mut rows: Vec<Element<'static, Message>> = Vec::new();
        rows.push(popover::section(self.tokens, "Add to shelf"));
        for shelf in &self.library.shelves {
            if shelf.id == ALL_SHELF {
                continue;
            }
            let nestable = folder_ids
                .iter()
                .all(|id| shelf::can_nest(&self.library.shelves, id, &shelf.id));
            rows.push(popover::owned_item(
                self.tokens,
                Some(IconName::Folder),
                shelf.name.clone(),
                None,
                false,
                nestable.then(|| Message::FileSelection(shelf.id.clone())),
            ));
        }
        rows.push(popover::separator(self.tokens));
        rows.push(popover::item(
            self.tokens,
            Some(IconName::Plus),
            "New shelf",
            None,
            false,
            Some(Message::FileSelectionOnNewShelf),
        ));
        popover::popover(self.tokens, rows, SELECT_POP_W)
    }

    fn title(&self) -> String {
        match (self.route, &self.open_document) {
            (Route::Reader, Some(path)) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| Route::Reader.title().to_owned()),
            _ => Route::Library.title().to_owned(),
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn style(&self, _theme: &Theme) -> iced::theme::Style {
        theme::application_style(self.tokens)
    }

    fn subscription(&self) -> Subscription<Message> {
        let now = Instant::now();
        let mut subscriptions = vec![event::listen_with(on_event)];
        // Frames flow only while something is in motion: the reveal
        // animating, a hide waiting out its grace, or a toast waiting out
        // its stamp. An idle window subscribes to nothing and costs no
        // redraws.
        if self.titlebar.needs_tick(now)
            || self.toasts.needs_tick(now)
            || self.press.is_some()
            || self.drag.is_some()
            || self.ellipsis_close_at.is_some()
        {
            subscriptions.push(window::frames().map(Message::Tick));
        }
        // A run's beats flow while the run lives: same id, same
        // subscription; gone from the tree, closed by the tracker.
        for run in &self.runs {
            subscriptions.push(
                progress::subscription(run.task, Arc::clone(&run.rx))
                    .map(Message::ImportProgress),
            );
        }
        Subscription::batch(subscriptions)
    }
}

/// Who holds a folder's root: the claim question a second run asks before
/// it starts.
enum Claim {
    Free,
    /// A focus walk holds it; an ask queues behind the walk's release.
    HeldByWalk,
    /// An ask holds it; a second ask is told, a walk waits for the next
    /// focus.
    HeldByAsk,
}

/// The store batch's answer, split: the copies that came home with their
/// measurements, and the first refusal's own sentence for the toast.
fn partition_store_results(results: Vec<StoreResult>) -> (CopyMap, Option<String>) {
    let mut copies: CopyMap = HashMap::new();
    let mut failure: Option<String> = None;
    for result in results {
        if result.is_ok() {
            copies.insert(result.id.clone(), (result.store.clone(), result.measured));
        } else if failure.is_none() {
            failure = result.error;
        }
    }
    (copies, failure)
}

/// The found file a path check describes, when the check found a document:
/// the loose-file run's translation from measurement to ledger row.
fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
    if !is_supported_path(&check.path) {
        return None;
    }
    let fp = check.fingerprint()?;
    Some(FoundFile {
        rel: paths::file_name(&check.path),
        path: check.path.clone(),
        ext: paths::extension(&check.path),
        size: check.size,
        fp,
    })
}

/// The removed row's second line: when the reader took the book out, and —
/// when the log remembers the file's own bytes — how big the promise is.
fn removed_sublabel(entry: &Tombstone, stamp: u64) -> String {
    let age = lib_text::human_age(entry.removed_ms, stamp);
    match entry.fp.mtime_ms {
        0 => format!("removed {age}"),
        _ => format!("removed {age} · {}", lib_text::human_size(entry.fp.size)),
    }
}

/// One spelling for the shelf chain a folder walk mints through — the books
/// it adds and the rungs between them — so a rung cannot be minted twice
/// under two spellings of its own name. Rungs already in the map are
/// reused, which is what makes a rescan continue the tree instead of
/// growing a twin beside it.
fn chain_for(
    folder: &mut WatchedFolder,
    key: &str,
    stamp: u64,
    planned_root: Option<&str>,
    root: &str,
    new_shelves: &mut Vec<shelf::Shelf>,
) -> String {
    let folder_id = folder.id.clone();
    let root = root.to_string();
    let planned = planned_root.map(str::to_string);
    folder.shelf_chain_for(
        key,
        |_| library_core::id::next_shelf_id(stamp),
        move |rung| {
            if rung.is_empty() {
                planned.clone().unwrap_or_else(|| paths::dir_label(&root))
            } else {
                rung.rsplit('/').next().unwrap_or(rung).to_string()
            }
        },
        |rung, id, name, parent| {
            let rel = (!rung.is_empty()).then(|| rung.to_string());
            new_shelves.push(shelf::Shelf::folder_shelf(id, name, &folder_id, rel, parent));
        },
    )
}

/// The first shelf a folder's tree holds a book on — the home a tombstone
/// remembers, so a restore puts the book back where the shelf showed it.
fn folder_shelf_of(shelves: &[shelf::Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|shelf| shelf.kind.folder_id() == Some(folder_id))
        .map(|shelf| shelf.id.clone())
}

/// Put the folder row back, by id. One place, because the ledger is the
/// part of the library that must never be written half-updated: a `placed`
/// set that lost an entry re-adds a book the reader already filed.
fn write_folder_row(folders: &mut Vec<WatchedFolder>, folder: WatchedFolder) {
    match folders.iter().position(|each| each.id == folder.id) {
        Some(at) => folders[at] = folder,
        None => folders.push(folder),
    }
}

/// The runtime's event firehose, narrowed to what the state tree consumes.
/// A plain `fn` — `listen_with` takes one by design, so the filter cannot
/// smuggle captured state into the subscription's identity.
fn on_event(event: iced::Event, status: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(event) => Some(Message::WindowEvent(id, event)),
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::Cursor(Some(position)))
        }
        iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::Cursor(None)),
        // The hold machine listens to EVERY left press and release, not
        // just the ones no widget claimed: a hold starts on a card, and a
        // card's button claims the press. The machine's own guards decide
        // which presses are holds.
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::PressStarted)
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::PressEnded)
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::EscapePressed),
        // Enter only while nothing captured it: a focused field owns the
        // key, and the shelf hears what the fields decline.
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            Some(if modifiers.shift() { Message::ShiftEnter } else { Message::EnterPressed })
        }
        _ => None,
    }
}

/// The scrim under an open menu: transparent, window-wide, and closing the
/// menu on any press that reaches it.
fn scrim() -> Element<'static, Message> {
    mouse_area(container(Space::new().width(Length::Fill).height(Length::Fill)))
        .on_press(Message::CloseMenu)
        .into()
}

/// The selection bar's popover width.
const SELECT_POP_W: f32 = 240.0;

/// The bar's own pill: the count readout and the four answers, in the
/// ActionBar's chrome — the surface, the hairline, the float's shadow.
fn select_pill(tokens: Tokens, count: usize, pop_open: bool) -> Element<'static, Message> {
    let face = row![
        container(text(format!("{count} selected")).size(12).color(tokens.muted))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }),
        pill_button(tokens, "All", false, Some(Message::SelectAll)),
        pill_button(
            tokens,
            "Add to shelf",
            pop_open,
            Some(Message::ToggleSelectPop),
        ),
        pill_button(
            tokens,
            &format!("Remove ({count})"),
            false,
            (count > 0).then_some(Message::AskRemoveSelection),
        ),
        pill_button(tokens, "Done", false, Some(Message::ClearSelection)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    container(face)
        .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 16.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.surface)),
            border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
            shadow: Shadow {
                color: wash(Color::BLACK, 0.18),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 12.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// One answer on the bar: quiet text in a round ghost, red for the
/// dangerous one, washed while open for the one holding the popover.
/// `None` for the message renders the answer disabled — listed, but not
/// quietly dropped.
fn pill_button(
    tokens: Tokens,
    label: &str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let danger = message.as_ref().is_some_and(|message| {
        matches!(message, Message::AskRemoveSelection)
    });
    let ink = if danger { crate::theme::DANGER } else { tokens.ink };
    let action = button(text(label.to_string()).size(12).color(ink))
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
        .style(move |_, status| {
            let wash_of = if danger { crate::theme::DANGER } else { tokens.accent };
            let background = match status {
                button::Status::Hovered => Some(Background::Color(wash(wash_of, 0.12))),
                button::Status::Pressed => Some(Background::Color(wash(wash_of, 0.20))),
                _ => active.then_some(Background::Color(wash(tokens.accent, 0.12))),
            };
            button::Style {
                background,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
                text_color: ink,
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The runs' live lines: a pill per run at the foot of the shelf, naming
/// the folder or the files and the count so far. Decorations, not controls
/// — a walk is not interruptible, so the pills take no presses and claim no
/// pointer.
fn runs_dock(tokens: Tokens, runs: &[FsRun]) -> Element<'static, Message> {
    let mut pills = Column::new().spacing(8).align_x(Alignment::Center);
    for run in runs {
        pills = pills.push(dock_pill(tokens, run_line(run)));
    }
    container(pills)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top: 0.0, right: 0.0, bottom: 20.0, left: 0.0 })
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .into()
}

/// One pill: the line in a rounded card, floating over the shelf.
fn dock_pill(tokens: Tokens, line: String) -> Element<'static, Message> {
    container(
        container(text(line).size(12).color(tokens.ink))
            .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.surface, 0.95))),
                border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
                shadow: Shadow {
                    color: wash(Color::BLACK, 0.18),
                    offset: Vector::new(0.0, 4.0),
                    blur_radius: 12.0,
                },
                ..container::Style::default()
            }),
    )
    .into()
}

/// The pill's line: the phase the last beat carries decides the verb, and
/// the stage decides the waiting wording before the first beat lands.
fn run_line(run: &FsRun) -> String {
    match &run.latest {
        Some(beat) if beat.phase == ImportPhase::Copy && beat.total > 0 => {
            format!("Copying “{}”: {} of {}", run.label, beat.done, beat.total)
        }
        Some(beat) if beat.total > 0 => {
            format!("Scanning “{}”: {} of {} found", run.label, beat.done, beat.total)
        }
        Some(beat) if beat.done > 0 => {
            format!("Scanning “{}”: {} found", run.label, beat.done)
        }
        _ => match run.stage {
            Stage::Measuring { .. } => format!("Measuring “{}”…", run.label),
            Stage::Restoring { .. } => format!("Restoring “{}”…", run.label),
            Stage::Copying { .. }
            | Stage::RestoreCopying { .. }
            | Stage::Duplicating { .. }
            | Stage::Departing { .. } => {
                format!("Copying “{}”…", run.label)
            }
            Stage::Walking { .. } | Stage::Storing { .. } => {
                format!("Scanning “{}”…", run.label)
            }
        },
    }
}

/// The import sheet's body: the folder, the formats, the size threshold,
/// how the books are held and the structure answer — the walk's orders,
/// written before it walks. `ground` is the tree the pick belongs to, when
/// one governs it: the notes speak the rung's own promise then, not the
/// whole import's.
#[allow(clippy::too_many_lines)]
fn import_sheet(
    tokens: Tokens,
    root: &Path,
    opts: &FolderOpts,
    ground: Option<&GroundWatch>,
) -> Element<'static, Message> {
    let section = |label: &'static str| -> Element<'static, Message> {
        container(text(label).size(11).color(tokens.muted))
            .padding(Padding { top: 2.0, right: 0.0, bottom: 6.0, left: 0.0 })
            .into()
    };

    // The folder's address, in a bordered pill.
    let path = root.to_string_lossy().into_owned();
    let folder_pill = container(
        row![
            icon(IconName::Open, 15, tokens.muted),
            container(text(path).size(13).color(tokens.ink)).width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_| container::Style {
        border: Border { color: tokens.line, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    });

    let include_row = row![
        chip(tokens, "Include selected", opts.include_selected, Some(Message::SheetImportInclude(true))),
        chip(tokens, "Exclude selected", !opts.include_selected, Some(Message::SheetImportInclude(false))),
    ]
    .spacing(6);

    // The format chips, two to a line.
    let mut format_rows: Vec<Element<'static, Message>> = Vec::new();
    for chunk in selectable_formats().chunks(2) {
        let mut line = Row::new().spacing(6);
        for format in chunk {
            line = line.push(chip(
                tokens,
                format.label(),
                opts.formats.contains(format),
                Some(Message::SheetImportFormat(*format)),
            ));
        }
        if chunk.len() == 1 {
            line = line.push(Space::new().width(Length::Fill));
        }
        format_rows.push(line.into());
    }

    // The size threshold, with its adjusters.
    let at_floor = opts.min_size == MIN_SIZE_FLOOR;
    let at_ceil = opts.min_size >= MIN_SIZE_CEIL;
    let size_row = row![
        container(text(opts.min_size_label()).size(12).color(tokens.ink))
            .padding(Padding { top: 3.0, right: 9.0, bottom: 3.0, left: 9.0 })
            .style(move |_| container::Style {
                background: Some(Background::Color(wash(tokens.line, 0.50))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
                ..container::Style::default()
            }),
        Space::new().width(Length::Fill),
        stepper(tokens, IconName::Minus, !at_floor, Message::SheetImportSize(-1)),
        stepper(tokens, IconName::Plus, !at_ceil, Message::SheetImportSize(1)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // How the books are held: one control, three answers, because there are
    // three modes — the fourth pair (a watching copy) is not one the sheet
    // can show, and every click writes both switches from the mode picked.
    let mode = opts.mode();
    let books_rows = Column::new()
        .push(chip(
            tokens,
            FolderMode::Copy.label(),
            mode == FolderMode::Copy,
            Some(Message::SheetImportMode(FolderMode::Copy)),
        ))
        .push(chip(
            tokens,
            FolderMode::LinkInPlace.label(),
            mode == FolderMode::LinkInPlace,
            Some(Message::SheetImportMode(FolderMode::LinkInPlace)),
        ))
        .push(chip(
            tokens,
            FolderMode::LinkInPlaceWatched.label(),
            mode == FolderMode::LinkInPlaceWatched,
            Some(Message::SheetImportMode(FolderMode::LinkInPlaceWatched)),
        ))
        .spacing(6);
    let books_note = text(mode_note(mode, ground.is_some())).size(12).color(tokens.muted);

    // The structure answer, and the promise it makes about the tree.
    let structure_rows = Column::new()
        .push(chip(
            tokens,
            "A shelf for each folder",
            opts.groups,
            Some(Message::SheetImportGroups(true)),
        ))
        .push(chip(
            tokens,
            "One shelf for everything",
            !opts.groups,
            Some(Message::SheetImportGroups(false)),
        ))
        .spacing(6);
    // A pick inside a governed tree answers for its rung alone: the note
    // says so, because "the whole import" would over-promise.
    let in_tree = ground.is_some_and(|watch| !watch.rung.is_empty());
    let structure_note = text(if in_tree {
        "This answer stands for this folder and the ones under it — the rest of the tree keeps its own."
    } else {
        "A shelf for each folder gives every subfolder its own shelf; one shelf keeps the whole import together."
    })
    .size(12)
    .color(tokens.muted);

    Column::new()
        .push(section("FOLDER"))
        .push(folder_pill)
        .push(section("FORMATS"))
        .push(include_row)
        .push(Column::with_children(format_rows).spacing(6))
        .push(section("FILE SIZE LARGER THAN"))
        .push(size_row)
        .push(section("HOW THE BOOKS ARE HELD"))
        .push(books_rows)
        .push(books_note)
        .push(section("FOLDER STRUCTURE"))
        .push(structure_rows)
        .push(structure_note)
        .spacing(10)
        .width(Length::Fill)
        .into()
}

/// The mode's own sentence under the sheet's control — one wording per
/// mode, so a mode described two ways never reads as two modes. A watched
/// pick inside a governed tree gets the rung's wording: the promise is the
/// subfolder's, and the rest of the tree keeps its own answer.
fn mode_note(mode: FolderMode, in_ground: bool) -> &'static str {
    match mode {
        FolderMode::Copy => {
            "Books are copied into the app's own files, so they keep working even if the folder moves or is deleted."
        }
        FolderMode::LinkInPlace => {
            "Books stay where they are — the library just remembers where they live. Books added to the folder later are not picked up."
        }
        FolderMode::LinkInPlaceWatched if in_ground => {
            "Books stay where they are, and this subfolder is checked for new ones. The rest of the tree keeps its own answer."
        }
        FolderMode::LinkInPlaceWatched => {
            "Books stay where they are, and the folder is checked for new ones when the app opens or you come back to it."
        }
    }
}

/// A toggle chip: the active one wears the accent's soft bed, the inactive
/// one waits quiet. `None` for the message renders it disabled.
fn chip(
    tokens: Tokens,
    label: &'static str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let action = button(text(label).size(12).color(if active { tokens.ink } else { tokens.muted }))
        .width(Length::Fill)
        .padding(Padding { top: 6.0, right: 10.0, bottom: 6.0, left: 10.0 })
        .style(move |_, status| {
            let background = if active {
                tokens.accent_soft
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => wash(tokens.line, 0.45),
                    _ => Color::TRANSPARENT,
                }
            };
            button::Style {
                background: Some(Background::Color(background)),
                border: Border {
                    color: if active { tokens.accent } else { tokens.line },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                text_color: if active { tokens.ink } else { tokens.muted },
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

/// The size threshold's round adjuster.
fn stepper(tokens: Tokens, glyph: IconName, enabled: bool, message: Message) -> Element<'static, Message> {
    let face = container(icon(glyph, 13, if enabled { tokens.ink } else { tokens.muted }))
        .padding(5.0);
    let action = button(face).style(move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed if enabled => wash(tokens.line, 0.60),
            _ => wash(tokens.line, 0.35),
        })),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
        text_color: tokens.ink,
        shadow: Shadow::default(),
        snap: false,
    });
    if enabled {
        action.on_press(message).into()
    } else {
        action.into()
    }
}

/// The appearance button's glyph for the base on screen.
fn appearance_glyph(base: BaseMode) -> IconName {
    match base {
        BaseMode::Light => IconName::Sun,
        BaseMode::Dark => IconName::Moon,
        BaseMode::Dim => IconName::Dim,
    }
}

/// Milliseconds since the epoch — the stamp the library's ids and rows are
/// minted with.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The window the app opens in: the original's 1200×800 with its 640×480
/// floor, frameless on Windows and Linux, and on macOS a native frame whose
/// titlebar is transparent and full-size-content — AppKit's own traffic
/// lights floating over the app's bar, exactly as the Tauri shell had it.
fn window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: iced::Size::new(1200.0, 800.0),
        min_size: Some(iced::Size::new(640.0, 480.0)),
        decorations: platform::os() != Os::Mac,
        ..Default::default()
    };

    #[cfg(target_os = "macos")]
    {
        settings.platform_specific = window::settings::PlatformSpecific {
            title_hidden: true,
            titlebar_transparent: true,
            fullsize_content_view: true,
        };
    }
    #[cfg(windows)]
    {
        // A frameless window keeps its drop shadow on Windows.
        settings.platform_specific.undecorated_shadow = true;
    }
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id =
            "com.codewiththiha.mareader".to_owned();
    }

    settings
}

/// The reader route until the engines land: the document's name, the fact
/// that it is remembered, and the way back.
fn reader_surface(tokens: Tokens, document: Option<&Path>) -> Element<'static, Message> {
    let name = document
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the document".to_owned());
    container(
        column![
            icon(IconName::Type, 40, tokens.accent),
            text(name).size(18).color(tokens.ink),
            text("Selected, remembered, and safe — the reading surfaces land with the engines.")
                .size(13)
                .color(tokens.muted),
            button(text("Back to the shelf").size(13).color(tokens.ink))
                .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
                .style(move |_, status| titlebar::ghost_button_style(tokens, 1.0, status))
                .on_press(Message::BackToShelf),
        ]
        .align_x(Alignment::Center)
        .spacing(12)
        .width(Length::Shrink),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding(Padding {
        top: platform::TITLE_BAR_H,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    })
    .into()
}

/// Where held folders land after a drop: onto the root they re-hang with
/// no parent, one reparent each because the root has no member list to
/// batch into; onto a shelf they nest as a batch (the web commit's own
/// `land_folders`).
fn land_folders(shelves: &mut [Shelf], folders: &[String], to: &str) -> bool {
    if folders.is_empty() {
        return false;
    }
    if to == ALL_SHELF {
        let mut moved = false;
        for folder in folders {
            moved |= library::arrange::nest_shelf(shelves, folder, None);
        }
        moved
    } else {
        library::arrange::nest_many(shelves, folders, to)
    }
}

/// The ghost's box: the web layer's own 9rem cover at A4 proportion
/// (drag.css's 144×204), plus the fan's headroom — the reference shifts
/// each tile out of the stack by (7n, −6n) pixels, so the stack needs
/// 21px of right and 18px of top before its first tile.
const GHOST_W: f32 = 144.0;
const GHOST_H: f32 = 203.7;
const FAN_RISE: f32 = 18.0;
const FAN_SHIFT: f32 = 21.0;

/// The drag layer: the payload drawn at the pointer — the fan of cover
/// tiles a drag lifts, or, once the fold is brewing, the plate of the
/// shelf the drop will make. Non-interactive by construction: a wash of
/// containers the pointer never meets, so every press underneath keeps
/// working while the ghost rides above them.
fn ghost_layer(
    tokens: Tokens,
    drag: &Drag,
    library: &LibraryBlob,
    fold: Option<&FoldPreview>,
    at: Point,
) -> Element<'static, Message> {
    // Sunk, which happens on one kind of target only: a titlebar crumb.
    // At full size the ghost covers the name of the very level the reader
    // is aiming at, so the anchor moves to the parked spot's centre, the
    // ghost shrinks to a third and the crumb stays readable — the web
    // layer's own translate(-50%,-50%) and scale(0.38). Its 0.85 opacity
    // rides the same shrink; an iced container carries no opacity, and
    // the third-size ghost uncovers the crumb whatever its alpha.
    let scale = if drag.sunk.is_some() { SUNK_SCALE } else { 1.0 };
    let ghost: Element<'static, Message> = match fold {
        Some(preview) => fold_plate(tokens, preview.filled),
        None => ghost_fan(tokens, &ghost_tiles(library, &drag.payload), drag.payload.len(), scale),
    };
    let (left, top) = match drag.sunk {
        Some(spot) => (
            (spot.x - 0.5 * GHOST_W * scale).max(0.0),
            (spot.y - 0.5 * GHOST_H * scale).max(0.0),
        ),
        // The web layer anchors the ghost so the pointer sits at 38% of
        // the cover's width and 32% of its height; the headroom offsets
        // the box by the fan's own rise on top of that.
        None => ((at.x - 0.38 * GHOST_W).max(0.0), (at.y - 0.32 * GHOST_H - FAN_RISE).max(0.0)),
    };
    container(ghost)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top, right: 0.0, bottom: 0.0, left })
        .align_x(Alignment::Start)
        .align_y(Alignment::Start)
        .into()
}

/// The fan's tiles: the payload's own labels, books first — the order the
/// web ghost stacks them in. A book a scan just took off the shelf draws
/// no tile rather than an empty one.
fn ghost_tiles(library: &LibraryBlob, payload: &DragPayload) -> Vec<(String, bool)> {
    let mut tiles: Vec<(String, bool)> = Vec::new();
    for id in &payload.books {
        let Some(row) = book::find_row(&library.books, id) else { continue };
        let label = match row {
            book::Row::Book(book) => book.title(),
            book::Row::Link { name, .. } => name.clone(),
        };
        tiles.push((label, false));
    }
    for id in &payload.folders {
        let name = shelf::find(&library.shelves, id)
            .map(|each| each.name.clone())
            .unwrap_or_default();
        tiles.push((name, true));
    }
    tiles
}

/// The first letter, uppercased — the ghost tile's stand-in cover, the
/// same initial the web ghost letters a tile with.
fn initial(label: &str) -> String {
    label.chars().next().map(|letter| letter.to_uppercase().collect()).unwrap_or_default()
}

/// The fan itself: at most `THUMB_CAP` tiles, each shifted up-and-right
/// out of the stack (drag.css's `translate(7n, -6n)`), with the payload's
/// count on the corner when it is more than one. The web fan's per-tile
/// rotation is the one thing left out: a 0.14 container carries no
/// rotation, and the stepped stack reads the same without it.
fn ghost_fan(
    tokens: Tokens,
    tiles: &[(String, bool)],
    total: usize,
    scale: f32,
) -> Element<'static, Message> {
    let (w, h) = (GHOST_W * scale, GHOST_H * scale);
    let (rise, shift) = (FAN_RISE * scale, FAN_SHIFT * scale);
    let mut layers: Vec<Element<'static, Message>> = Vec::new();
    for (fan, (label, folder)) in tiles.iter().take(THUMB_CAP).enumerate() {
        let face: Element<'static, Message> = if *folder {
            container(icon(IconName::Open, (16.0 * scale) as u16, tokens.muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        } else {
            container(text(initial(label)).size(36.0 * scale).color(tokens.muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        };
        let tile = container(face)
            .width(w)
            .height(h)
            .style(move |_| container::Style {
                background: Some(Background::Color(tokens.surface)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    // The web tile's own book-spine corners: 3px on the
                    // spine side, 6px on the fore-edge.
                    radius: Radius {
                        top_left: 3.0,
                        top_right: 6.0,
                        bottom_right: 6.0,
                        bottom_left: 3.0,
                    },
                },
                shadow: Shadow {
                    color: wash(Color::BLACK, 0.35),
                    offset: Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                },
                ..container::Style::default()
            });
        layers.push(
            container(tile)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: rise - 6.0 * scale * fan as f32,
                    right: 0.0,
                    bottom: 0.0,
                    left: 7.0 * scale * fan as f32,
                })
                .align_x(Alignment::Start)
                .align_y(Alignment::Start)
                .into(),
        );
    }
    if total > 1 {
        layers.push(
            container(count_badge(tokens, total, scale))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: rise - 8.0 * scale,
                    right: shift - 8.0 * scale,
                    bottom: 0.0,
                    left: 0.0,
                })
                .align_x(Alignment::End)
                .align_y(Alignment::Start)
                .into(),
        );
    }
    container(Stack::with_children(layers)).width(w + shift).height(h + rise).into()
}

/// The payload's count, on the fan's top corner: the accent's pill the web
/// ghost wears when the drag is carrying more than one.
fn count_badge(tokens: Tokens, total: usize, scale: f32) -> Element<'static, Message> {
    container(text(total.to_string()).size(11.0 * scale).color(tokens.paper))
        .padding(Padding {
            top: 3.0 * scale,
            right: 7.0 * scale,
            bottom: 3.0 * scale,
            left: 7.0 * scale,
        })
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
            shadow: Shadow {
                color: wash(Color::BLACK, 0.35),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 6.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// The fold's promise, in the ghost's own hands: the folder card's plate
/// drawn the way the drop would leave it — `filled` cells wearing the
/// accent's tint because the shelf does not exist yet, the next cell
/// holding the plus that says the plate is still taking items, and the
/// label naming what the drop will do.
fn fold_plate(tokens: Tokens, filled: usize) -> Element<'static, Message> {
    const W: f32 = 136.0;
    let plate_h = W * 3.0 / 4.0;
    let gap = 3.0;
    let cell_w = (W - gap) / 2.0;
    let cell_h = (plate_h - gap) / 2.0;
    let mut lines: Vec<Element<'static, Message>> = Vec::with_capacity(2);
    for line_ix in 0..2usize {
        let mut line: Row<'static, Message> = Row::new().spacing(gap);
        for slot_ix in 0..2usize {
            let at = line_ix * 2 + slot_ix;
            let cell: Element<'static, Message> = if at < filled {
                container(Space::new().width(cell_w).height(cell_h))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.surface,
                            tokens.accent,
                            0.34,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            } else if at == filled {
                container(icon(IconName::Plus, 14, tokens.accent))
                    .width(cell_w)
                    .height(cell_h)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.surface,
                            tokens.accent,
                            0.10,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            } else {
                container(Space::new().width(cell_w).height(cell_h))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(mix(
                            tokens.paper,
                            tokens.surface,
                            0.72,
                        ))),
                        border: Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 3.0.into(),
                        },
                        ..container::Style::default()
                    })
                    .into()
            };
            line = line.push(cell);
        }
        lines.push(line.into());
    }
    let plate = container(Column::with_children(lines).spacing(gap))
        .width(W)
        .height(plate_h)
        .style(move |_| container::Style {
            background: Some(Background::Color(plate_seam(tokens))),
            border: Border { color: plate_seam(tokens), width: 1.0, radius: 8.0.into() },
            shadow: Shadow {
                color: wash(Color::BLACK, 0.35),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..container::Style::default()
        });
    container(
        column![
            plate,
            container(text("New shelf").size(11).color(tokens.accent))
                .width(Length::Fill)
                .center_x(Length::Fill),
        ]
        .spacing(5)
        .width(W),
    )
    .into()
}
