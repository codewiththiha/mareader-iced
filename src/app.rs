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

use iced::time::{Duration, Instant};
use iced::widget::{
    button, column, container, mouse_area, operation, row, scrollable, stack, text, text_input,
    Column, Id, Row, Space, Stack,
};
use iced::border::Radius;
use iced::{
    event, keyboard, mouse, window, Alignment, Background, Border, Color, Element, Length,
    Padding, Point, Shadow, Size, Subscription, Task, Theme,
};

use library_core::blob::LibraryBlob;
use library_core::book::{self, Book, Fingerprint, Origin};
use library_core::conflict::{self, Arrival, Placement};
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
use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::library::departure::{
    self, CopyAnswer, CopyAsk, CopyWork, ReturnPath, RowMove, ShelfDeparture, ShelfSeam,
};
use crate::library::duplicate::{self, BookCopy, Duplicated, DupPlan, TreePlan};
use crate::library::{self, bar, menus};
use crate::library::reveal::{self, Reveal};
use crate::platform::{dialogs, fs, now_ms, progress, store};
use crate::reader;
use crate::route::Route;
use crate::storage;
use crate::theme::{self, mix, wash, Elevation, Tokens};
use crate::ui::menu as popover;
use crate::ui::sheet;
use crate::ui::toast::{ToastHost, Tone};

/// The sheet's rename field's identity — focus lands on it the moment the
/// sheet appears.
const SHEET_INPUT: Id = Id::new("sheet-input");

/// The shelf grid's scroll, for the reveal's own way to a cell.
pub const LIBRARY_SCROLL: &str = "library-shelf";

/// The app's own name: the window title and the bar's centre both fall back to
/// it while no document owns them.
const APP_TITLE: &str = "Mareader";

/// The light's own clock: a reveal's ring stands for this long on the
/// level the answer walked the reader to.
const FLASH_DWELL: Duration = Duration::from_millis(1600);

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

/// The boot proof: build the whole app state and both surfaces, report what
/// was built, and return without opening a window.
///
/// This is what CI's build lane runs on every OS (`mareader --smoke`). It is
/// deliberately the app's own shape rather than a bespoke list: the surfaces it
/// builds are the routes the app has, so a phase that adds one without adding
/// it here fails the lane that exists to notice. Nothing here needs a display,
/// a window server, or a PDF engine — which is the point, because the lane
/// proves the app *starts* on machines that have none of them.
pub fn smoke() -> iced::Result {
    let (mut state, _boot) = Mareader::boot();
    let mut built: Vec<&str> = Vec::new();

    // Both routes, built as widget trees. Setting the route and calling the
    // same `view` the runtime calls is the whole check: a panic in layout
    // arithmetic, a broken token, a missing icon — anything the shelf or the
    // reading surface does while being built — lands here.
    for (route, name) in [(Route::Library, "shelf"), (Route::Reader, "reader")] {
        state.route = route;
        let _surface = state.view();
        built.push(name);
    }

    // The reading surface with a document under it: the ladder's `Opening`
    // state is what a reader sees the moment they open a book, and it is the
    // one state whose placeholders and waiting lines only exist between the ask
    // and the answer.
    // The document step is a struct literal rather than a door of its own:
    // the open the reader runs on is exactly this — an address, no row, and
    // the first page — and a shortcut here would be a second place to keep
    // the shape of an open.
    let effects = state.reader.update(
        reader::Message::Open(reader::Open {
            path: PathBuf::from("smoke.pdf"),
            book_id: None,
            row_title: None,
            resume: 1,
        }),
        Instant::now(),
    );
    // Nothing is written: an open that has not answered records no read, and
    // `apply_reader_effects` is what owns every write there is.
    let _ = state.apply_reader_effects(effects);
    state.route = Route::Reader;
    let _opening = state.view();
    built.push("reader (opening)");

    println!("mareader --smoke: booted; surfaces built: {}", built.join(", "));
    Ok(())
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

/// Which question a folder run answers; a boolean at the signature could
/// not say. An ask is the reader's own import; a walk is the app keeping a
/// watched folder's promise to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Explicitly,
    OnFocus,
}

/// The covered walk's own seat: the rung shelf the ground named, and the
/// tree that walks it.
struct Covered {
    tree_root: String,
    shelf_id: String,
    shelf_name: String,
}

/// The already-imported answer, kept with its close: where the light
/// stands, which shelf the sheet names, and what the import found.
/// The already-imported answer kept with its close: it is the modal's own
/// modal, because the sheet's panel and the handler both read it whole.
#[derive(Clone, Debug)]
struct LitNote {
    shelf_id: String,
    name: String,
    kind: conflicts::NoteKind,
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
    Walking { root: String, opts: FolderOpts, asked: Asked, plan: RootPlan },
    /// The scan half of an unbound copy run: the ground walked once more,
    /// with no ledger behind it.
    CopiesScan { root: String, opts: FolderOpts, dest: CopiesDest },
    /// The store half of an unbound copy run: the copes the landing files
    /// beside the ground's standing tree.
    Copies { work: Box<CopiesWork> },
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

/// A departure run's landing: the rows the copies were asked for, and the
/// gesture that finishes when they come home.
struct DepartWork {
    converting: Vec<String>,
    landing: DepartLand,
}

/// The gesture a copy run finishes: a move resumes with what landed, a rung
/// comes apart once its books are safe, and a removal runs whatever the
/// copies did.
enum DepartLand {
    /// A hand-move: the gesture's whole id list and the hand that resumes.
    Move { ids: Vec<String>, hand: RowMove },
    /// The shelf move's landing: the departures read at the answer, the
    /// level they were going to, and the seam a sibling drop named.
    ShelfMove {
        deps: Vec<ShelfDeparture>,
        level: Option<String>,
        seam: Option<ShelfSeam>,
    },
    /// The rung the take-apart question named.
    Rung { id: String },
    /// The removal sheet's own gesture.
    Removal { purge: Vec<String>, shelves: Vec<String> },
}

/// The folder walk's answer, planned against the ledger and waiting for its
/// copies (or landing straight away when the books stay at place).
struct WalkPlan {
    /// The ledger row, resolved against the library as the walk ended —
    /// placed marks, spent tombstones and the shelf map are written onto it
    /// as the landing runs, and it goes back whole at the end.
    folder: WatchedFolder,
    asked: Asked,
    /// The covered walk's light — the shelf its own pick names, kept with
    /// the plan until the landing answers for it.
    continuation: Option<Continuation>,
    /// The folder's display name, for the toasts.
    root_name: String,
    /// The deduped name a fresh root rung wears; `None` when the map already
    /// holds the root shelf.
    planned_root: Option<String>,
    /// The additions, each wearing the book id it will land as.
    adds: Vec<(String, FoundFile)>,
    /// The moves the ledger healed: book id to its new address.
    relinks: Vec<(String, String)>,
    /// The per-file questions a merge owes, asked only once the landing is
    /// done — a file asked about must not stand among the landed rows.
    asks: Vec<ConflictAsk>,
    /// The rows a planned tree re-seats: the row, and the file whose rung
    /// decides where it lands.
    replacements: Vec<(String, FoundFile)>,
    /// The addresses whose file the library already reads in place, where
    /// this run lands its own copy beside the linked row that reads it.
    copy_paths: HashSet<String>,
    /// The rows a moved-out log already answers for: the reader has these
    /// books, so the walk lights them up rather than landing neighbours.
    represented: Vec<String>,
}

/// The walk's own root answer: *as new* names the tree something else and
/// continues from a fresh row, so the old row keeps answering its shelf;
/// *merge* files the new scans into the shelf that already holds the name.
#[derive(Clone, Debug, Default)]
struct RootPlan {
    rename: Option<String>,
    into: Option<String>,
    /// The covered walk's light: the shelf its pick stood on, and the name
    /// its ledger row wears — the re-import's own run answers to them both.
    continuation: Option<Continuation>,
}

/// What the covered walk's own light knows: where the pick's own shelf
/// stands, and which name the session opened with. The landing owes it an
/// answer even when nothing new was found.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Continuation {
    shelf_id: String,
    name: String,
}

/// The unbound copy run's seat: the run ground the tree's family reads, but
/// whose copies are the library's own rather than a second read of the
/// ground.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CopiesDest {
    /// Spliced right behind the shelf whose name the arrival collided with:
    /// a copy appended to the end of the level is a shelf the reader has to
    /// go and find.
    NewShelf { name: String, after: Option<String> },
    /// The *replace*'s target, whose books the sweep has just taken out.
    Into { shelf_id: String },
}

/// A copy run's plan, queued past the scan: the files still owed copies,
/// each wearing the id it lands as, and the run's own answers.
struct CopiesWork {
    /// The ground the run walked: its claim, its label.
    root: String,
    dest: CopiesDest,
    opts: FolderOpts,
    pending: Vec<PendingCopy>,
}

/// What waits behind a held root: everything the deferred run owes to start
/// again — the picked ground, the sheet answers it walked, and how the
/// reader meant the books held. A re-pick waits whole where a focus walk
/// waits politely.
#[derive(Clone)]
enum QueuedImport {
    Walk(PathBuf, FolderOpts, RootPlan),
    Copies { dir: PathBuf, opts: FolderOpts, dest: CopiesDest },
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
    /// Covered files restored as their folder's linked books, counted for
    /// the toast.
    restored: usize,
    /// The questions the screens raised, riding along so they are asked
    /// only once the copies have landed: a sheet answered while its own
    /// copy is still in flight would land beside a ghost.
    asks: Vec<ConflictAsk>,
    /// The books the drop found the reader already had, by a folder's own
    /// moved-out log: the light lands on the first of them.
    represented: Vec<String>,
    /// The folder ledger an answered file settles when its copy comes
    /// home — the answer's own placement record, absent for a pick's run.
    settle: Option<(String, Fingerprint)>,
    /// The slot the answered file takes, when an answer named one.
    index: Option<usize>,
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
    /// The reveal in flight: what the answer lit, and when its flash
    /// began — a second reveal of the same thing is a second reveal, by
    /// the nonce the answer bumps.
    reveal: Option<(Reveal, Instant)>,
    /// The grid's viewport height, read off the scroll's own report so the
    /// reveal's offset is the viewport's own geometry.
    shelf_viewport_h: f32,
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
    /// The name questions waiting behind the sheet's current one: two drops
    /// in flight owe two answers, and an answered sheet stays up while the
    /// queue lasts.
    conflict_waiting: Vec<ConflictAsk>,
    /// The apply-to-all switch's state, reset every time a new question
    /// takes the sheet: a batch is a promise made per question, not a mood
    /// the reader left switched on.
    apply_all: bool,
    /// The import sheet's options. They outlive the sheet: a second folder
    /// is usually imported the same way as the first.
    import_opts: FolderOpts,
    /// The app-global toast slot.
    toasts: ToastHost,
    /// The reading surface: the open document, the viewer's geometry and the
    /// engine behind them. Alive from boot, but it loads no Pdfium until the
    /// first document is opened, so a run that never reads a PDF never touches
    /// the engine at all.
    reader: reader::Reader,
    /// The filesystem runs in flight: folder walks, their store batches,
    /// and the loose-file runs. One dock pill per run.
    runs: Vec<FsRun>,
    /// An explicit import waiting for a focus walk of the same folder to
    /// release it: an ask outranks a rescan, but never races its ledger
    /// write.
    queued_ask: Option<QueuedImport>,
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

impl Mareader {
    fn boot() -> (Self, Task<Message>) {
        let settings = storage::load_settings();
        let tokens = Tokens::for_base(settings.appearance.base);
        // The reading surface, built before the state that owns it: it seeds
        // its viewer from the settings, and starts no engine yet.
        let reader = reader::Reader::new(&settings);
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
            reveal: None,
            shelf_viewport_h: 800.0,
            dup_queue: Vec::new(),
            dup_landed: Vec::new(),
            tap_swallow: None,
            select_pop: false,
            renaming: false,
            rename_draft: String::new(),
            context: None,
            sheet: None,
            conflict_waiting: Vec::new(),
            apply_all: false,
            import_opts: FolderOpts::default(),
            settings,
            toasts: ToastHost::default(),
            reader,
            runs: Vec::new(),
            queued_ask: None,
            verifying: false,
            next_task: 0,
        };
        // The grid reports its first fit against the window the app opens
        // in; the Resized event confirms it.
        state.report_auto_fit();
        // The reading surface hears the size the window opens with, so a book
        // opened before the first resize is rasterised for the window the
        // reader is actually looking at.
        let opening = state.viewport;
        let _ = state.reader_resize(opening);
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
                self.reader_resize(size)
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
        // The reading surface re-rasterises on the new grid: a page drawn for a
        // 1× screen must not be stretched by the compositor on a 1.5× panel.
        let effects = self
            .reader
            .update(reader::Message::Scale(f64::from(factor)), Instant::now());
        let _ = self.apply_reader_effects(effects);
    }

    /// The window's new size, to the reading surface. The chrome is an overlay,
    /// so the reading area is the window itself and nothing is subtracted.
    fn reader_resize(&mut self, size: Size) -> Task<Message> {
        let effects = self
            .reader
            .update(reader::Message::Resized(size), Instant::now());
        self.apply_reader_effects(effects)
    }

    /// The reader's reports, written: a read goes into the library blob and the
    /// disk, a sentence into the toast slot. The reader never touches either —
    /// it says what happened, and the app owns every byte that lands.
    fn apply_reader_effects(&mut self, effects: Vec<reader::Effect>) -> Task<Message> {
        let mut wrote = false;
        let mut created: Option<String> = None;
        for effect in effects {
            match effect {
                reader::Effect::Record(read) => {
                    let address = read.path.to_string_lossy().into_owned();
                    // A PDF's resume point is a page and a page count: it never
                    // inherits the fraction a reflowable book left in the same
                    // slot.
                    let point = book::ReadPoint {
                        page: read.page,
                        num_pages: read.num_pages,
                        fraction: None,
                    };
                    let landed = match read.book_id.as_deref() {
                        // The reader named a row: an id is the only thing that
                        // tells two rows of one address apart, and it is what
                        // distinguishes an independent book from a shared one.
                        Some(id) => book::record_read_row(
                            &mut self.library.books,
                            id,
                            &address,
                            read.title,
                            read.author,
                            point,
                            now_ms(),
                        ),
                        None => book::record_read(
                            &mut self.library.books,
                            &address,
                            read.title,
                            read.author,
                            point,
                            now_ms(),
                        ),
                    };
                    // A book the library did not know joins it as a linked row
                    // at the front of "All", wearing a placeholder fingerprint
                    // and carrying the name the document gave. It is measured
                    // on the way past, exactly as a freshly imported file is:
                    // otherwise a watched folder would keep refusing to rescan
                    // it until the next launch.
                    if let Some(book) = landed {
                        created = Some(book.path().to_string());
                    }
                    wrote = true;
                }
                reader::Effect::Progress(read) => {
                    let address = read.path.to_string_lossy().into_owned();
                    // The same rows the progress effect wrote in the web app:
                    // the reader's own book when it named one, every shared row
                    // at the address otherwise. Nothing is created here — a
                    // page turn is not a moment to add books to a library —
                    // and nothing but the position is touched.
                    for index in book::rows_for_read(
                        &self.library.books,
                        read.book_id.as_deref(),
                        &address,
                    ) {
                        if let Some(book) = self
                            .library
                            .books
                            .get_mut(index)
                            .and_then(book::Row::as_book_mut)
                        {
                            book.page = read.page.clamp(1, read.num_pages.max(1));
                            book.fraction = None;
                        }
                    }
                    wrote = true;
                }
                reader::Effect::Toast(tone, sentence) => {
                    self.toasts.show(tone, sentence, Instant::now());
                }
            }
        }
        let mut tasks = vec![];
        if wrote {
            tasks.push(self.persist_library());
        }
        if let Some(address) = created {
            // The one measurement an open owes a row it just created: its
            // fingerprint, so the library stops holding a placeholder.
            tasks.push(Task::perform(
                async move { fs::check_paths(std::slice::from_ref(&address)) },
                Message::ChecksDone,
            ));
        }
        Task::batch(tasks)
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

    /// A shelf taken apart, receipt and navigation aside: the primitive is
    /// the tree's whole business — the children re-hang on its parent, the
    /// folder's own rungs re-hang the way its next scan would hang them, the
    /// folder lets the rung go in its map, and the books come up exactly one
    /// level — and a reader standing ON the level steps out to the level it
    /// hung from. True when the id named a shelf.
    fn dismantle_shelf(&mut self, id: &str) -> bool {
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
            Open { id: String, path: PathBuf },
            Shelf(String),
            Nothing,
        }
        let tap = match book::find_row(&self.library.books, id) {
            Some(book::Row::Book(book)) => Tap::Open {
                id: book.id.clone(),
                path: PathBuf::from(book.path()),
            },
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
            Tap::Open { id, path } => self.open_row(Some(id), path),
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
                moved |= self.gated_seat(&payload.books, from.clone(), to.clone(), index, &[]);
                moved |= self.land_folders(&payload.folders, &to);
            }
            DropEffect::ShelfSibling { anchor_id, after } => {
                moved |= self.reorder_shelves(&payload.folders, &anchor_id, after);
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
                moved |= self.land_folders(&payload.folders, ALL_SHELF);
            }
            DropEffect::FileToShelf { shelf_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), shelf_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &shelf_id);
            }
            DropEffect::NestInto { folder_id } => {
                moved |= self.gated_seat(&payload.books, from.clone(), folder_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &folder_id);
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
                moved |= self.gated_seat(&books, from.clone(), shelf_id.clone(), None, &[]);
                moved |= self.land_folders(&payload.folders, &shelf_id);
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
        // Books go through the filing's own gate — the level's name screen
        // and the return's bind — and folders, whose names no level
        // collides with, ride the shelf move's own screen: a rung dropped
        // off the seat its tree names asks before it goes.
        let mut moved = self.gated_file(&book_ids, &target);
        let clean = self.screened_shelf_moves(&folder_ids, Some(&target), None);
        moved |= library::arrange::nest_many(&mut self.library.shelves, &clean, &target);
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
                // shelf map seats the pick on the shelf it named, and the
                // continuation keeps the rung's own light for the landing.
                if opts.mode().reads_in_place()
                    && let Some(covered) = self.covered_shelf(&root_str)
                {
                    let walk = self.begin_folder_walk(
                        PathBuf::from(covered.tree_root),
                        opts,
                        Asked::Explicitly,
                        RootPlan {
                            continuation: Some(Continuation {
                                shelf_id: covered.shelf_id,
                                name: covered.shelf_name,
                            }),
                            ..RootPlan::default()
                        },
                    );
                    return Task::batch([persisted, walk]);
                }
                // A folder whose name the root level already holds is a
                // question before it is an import — two shelves of one name
                // are two doors a reader cannot tell apart. A shelf the
                // arriving folder's own row named is a continuation: the
                // same question, the sheet's own words.
                let incoming = paths::dir_label(&root_str);
                if let Some(existing_id) =
                    conflict::collide_shelf(&self.library.shelves, None, &incoming)
                {
                    let own = self
                        .library
                        .folders
                        .iter()
                        .find(|f| f.root == root_str)
                        .and_then(|f| f.shelf_map.get("").cloned())
                        .is_some_and(|root_rung| root_rung == existing_id);
                    let existing_name = shelf::find(&self.library.shelves, &existing_id)
                        .map(|each| each.name.clone())
                        .unwrap_or_else(|| incoming.clone());
                    self.sheet = Some(Sheet::ShelfConflict {
                        ask: ShelfConflictAsk {
                            incoming_name: incoming,
                            existing_id,
                            existing_name,
                            root: root_str,
                            opts,
                            own,
                        },
                    });
                    return persisted;
                }
                let walk = self.begin_folder_walk(root, opts, Asked::Explicitly, RootPlan::default());
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

    /// The removal sheet's own door: a shelf coming off the list that reads
    /// books in place buys their copies first, and a removal with nothing to
    /// copy runs at once.
    fn remove_entries(&mut self, purge: Vec<String>, shelves: Vec<String>) -> Task<Message> {
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
    fn remove(&mut self, purge: Vec<String>, shelves: Vec<String>) -> bool {
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
    fn ask_shelf_apart(&mut self, id: &str) -> Task<Message> {
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
    fn purge_row(&mut self, id: &str) -> bool {
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

    /// The covered walk's seat: the rung shelf the ground names, its own
    /// name, and the tree's root — everything the landing needs to answer
    /// the pick back to the shelf it meant.
    fn covered_shelf(&self, root: &str) -> Option<Covered> {
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
                self.begin_folder_walk(PathBuf::from(root), opts, Asked::OnFocus, RootPlan::default()),
            ])
        } else {
            persisted
        }
    }

    /// Admit a document through the gate, remember it, and read it — the
    /// shape of the web app's open flow: the address is gated first, the
    /// library's row for it is settled second, and only then is the engine
    /// asked for anything.
    fn open_document_path(&mut self, path: PathBuf) -> Task<Message> {
        self.open_row(None, path)
    }

    /// The same door, told *which* row the reader meant. An id is what keeps a
    /// book the reader imported privately from resuming at the page a shared
    /// copy of the same file left behind.
    ///
    /// The web app's open flow, in the order it ran it: the gate first, the
    /// row second, the resume point third — read *before* the engine is asked
    /// for anything, so a progress write from the book that is still open
    /// cannot overwrite the page this open begins at.
    fn open_row(&mut self, book_id: Option<String>, path: PathBuf) -> Task<Message> {
        let address = path.to_string_lossy().into_owned();

        // A row the library already knows is dead does not open onto a
        // document at all. The web app answers a click on a missing book with
        // the Find-again question, which keeps the row, its shelves and its
        // page exactly as they are; until that sheet lands, the honest answer
        // is a sentence rather than a reader standing on a file that is not
        // there.
        if let Some(book) = book_id
            .as_deref()
            .and_then(|id| book::find_by_id(&self.library.books, id))
            .filter(|book| book.path() == address)
            .filter(|book| book.missing)
        {
            let name = book.title();
            self.toasts.show(
                Tone::Error,
                format!(
                    "{name} is not where the library last saw it. Rescan the folder it \
                     lived in and the shelf will point the book at its new home.",
                ),
                Instant::now(),
            );
            return Task::none();
        }

        // Which row answers for this address: the one the reader named when
        // they named one, else a *shared* row at the address. A book of its
        // own is the reader's private instance of the file, and an open that
        // arrived as nothing but an address has not said it meant that one — a
        // drop must never hijack a private book's resume point.
        let row = book_id
            .as_deref()
            .and_then(|id| book::find_by_id(&self.library.books, id))
            .filter(|book| book.path() == address)
            .or_else(|| {
                book::book_rows(&self.library.books)
                    .find(|book| book.path() == address && !book.independent)
            });

        // The address is gated after the row is settled, so a book the library
        // knows is asked the sharper question first: is the file readable at
        // all? A file that is gone is the missing-book gate's news, not this
        // one's.
        if let Err(error) = fs::ensure_readable_document(&address) {
            self.toasts.show(Tone::Error, error, Instant::now());
            return Task::none();
        }

        // The row the open ends up belonging to, and the name it wears: the
        // reader's own pick when they picked one, else the shared row that
        // answers for the address.
        let settled = row.map(|book| book.id.clone());
        // The row's *display* name, not its raw title: the library answers with
        // the name the file was imported under when the row carries none of its
        // own, and that is what keeps a stored copy from introducing itself by
        // the `source.pdf` it lives at.
        let row_title = row.map(Book::title);

        // The resume point, through the ported rule — read with the SETTLED
        // row, not the one the caller named, because that is the order the web
        // app read it in: a drop of a file whose first row at the address is a
        // private book resumes where the shared copy's reader left off, not
        // where the private copy's did.
        let (resume, _fraction) =
            book::resume_point(&self.library.books, settled.as_deref(), &address);
        let open = reader::Open {
            path,
            book_id: settled,
            row_title,
            // A PDF's resume point is a page and nothing else: no stream
            // position rides along, whatever a reflowable book left in the
            // slot.
            resume,
        };
        self.settings.last_path = Some(address);
        self.open_book(open)
    }

    /// Hand a document to the reading surface and stand on the reader route.
    /// The reading position the library remembers travels in with it, so the
    /// engine can be asked for the right page the first time.
    fn open_book(&mut self, open: reader::Open) -> Task<Message> {
        let effects = self
            .reader
            .update(reader::Message::Open(open), Instant::now());
        self.route = Route::Reader;
        self.menu = None;
        let writes = self.apply_reader_effects(effects);
        Task::batch([self.persist_settings(), writes])
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
            walks.push(self.begin_folder_walk(
                PathBuf::from(root),
                opts,
                Asked::OnFocus,
                RootPlan::default(),
            ));
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
    fn begin_folder_walk(
        &mut self,
        dir: PathBuf,
        opts: FolderOpts,
        asked: Asked,
        plan: RootPlan,
    ) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        match self.root_claim(&root) {
            Claim::Free => {}
            Claim::HeldByWalk => {
                if asked == Asked::Explicitly {
                    self.queued_ask = Some(QueuedImport::Walk(dir, opts, plan));
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
            stage: Stage::Walking { root: root.clone(), opts: opts.clone(), asked, plan },
        });
        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::ScanDone(task, result),
        )
    }

    /// The scan start of an unbound copy run: one walk of the ground, one
    /// card, no row to answer to — the standing tree's claims are beside
    /// the copies, never under them.
    fn begin_copies_run(&mut self, dir: PathBuf, opts: FolderOpts, dest: CopiesDest) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        match self.root_claim(&root) {
            Claim::Free => {}
            Claim::HeldByWalk => {
                self.queued_ask = Some(QueuedImport::Copies { dir, opts, dest });
                return Task::none();
            }
            Claim::HeldByAsk => {
                self.toasts.show(
                    Tone::Info,
                    format!("{} is already being imported.", paths::dir_label(&root)),
                    Instant::now(),
                );
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
            stage: Stage::CopiesScan { root: root.clone(), opts: opts.clone(), dest },
        });
        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::CopiesScanned(task, result),
        )
    }

    /// The unbound walk's scan answer: heal the rows the ground reads
    /// beside the tree, then owe every quieter file a copy of its own — the
    /// ledger's table says which, and the batch rides the run's own card.
    fn copies_scanned(
        &mut self,
        task: u64,
        result: Result<Vec<FoundFile>, String>,
        now: Instant,
    ) -> Task<Message> {
        let Some(ix) = self.runs.iter().position(|run| run.task == task) else {
            return Task::none();
        };
        let Stage::CopiesScan { root, opts, dest } = &self.runs[ix].stage else {
            return Task::none();
        };
        let (root, opts, dest) = (root.clone(), opts.clone(), dest.clone());
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.runs.remove(ix);
                self.toasts.show(Tone::Error, error, now);
                return self.release_root(&root);
            }
        };
        let stamp = now_ms();
        // Everything but the copy the library already made, one book per
        // fingerprint, off the ledger's own pure table; the heal reads the
        // copy paths again against the tree beside it.
        let registry = ledger::registry_of(&self.library.books);
        let copy_paths = ledger::copy_over_paths(&found, &registry, &self.library.books);
        let mut adds = ledger::unbound_copies(&found, &registry, &self.library.books);
        let mut healed = 0usize;
        // A file at an address the library already reads is that book,
        // whatever the two fingerprints say.
        adds.retain(|file| {
            if copy_paths.contains(&file.path) {
                return true;
            }
            match book::book_rows_mut(&mut self.library.books).find(|b| b.path() == file.path) {
                Some(existing) => {
                    existing.heal(file.fp);
                    healed += 1;
                    false
                }
                None => true,
            }
        });
        if adds.is_empty() {
            self.runs.remove(ix);
            self.toasts.show(
                Tone::Info,
                format!("Everything in “{}” is already in the library", paths::dir_label(&root)),
                now,
            );
            if healed > 0 {
                return Task::batch([self.persist_library(), self.release_root(&root)]);
            }
            return self.release_root(&root);
        }
        let pending: Vec<PendingCopy> = adds
            .into_iter()
            .map(|file| PendingCopy {
                book_id: library_core::id::next_id(stamp),
                file,
                title: None,
            })
            .collect();
        let requests: Vec<BookFileRequest> = pending
            .iter()
            .map(|each| BookFileRequest {
                from: each.file.path.clone(),
                id: each.book_id.clone(),
            })
            .collect();
        let sink = Arc::clone(&self.runs[ix].sink);
        let task_name = task.to_string();
        self.runs[ix].stage = Stage::Copies {
            work: Box::new(CopiesWork { root, dest, opts, pending }),
        };
        Task::perform(
            async move { store::store_books(&task_name, &requests, &sink) },
            move |results| Message::CopiesDone(task, results),
        )
    }

    /// The unbound copies' landing: every file lands as the library's own —
    /// its own measurement, its source's fingerprint free — on the seat the
    /// run's answer minted, and the tree beside reads on untouched.
    fn copies_run_done(&mut self, work: CopiesWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let stamp = now_ms();
        let mut landed_ids: Vec<(String, String)> = Vec::new();
        let mut root_shelf: Option<String> = None;
        let mut rungs: HashMap<String, String> = HashMap::new();
        for item in &work.pending {
            let Some((store_path, measured)) = copies.get(&item.book_id) else {
                continue;
            };
            let independent =
                book::book_rows(&self.library.books).any(|each| each.path() == item.file.path);
            let mut minted = Book::new(
                item.book_id.clone(),
                item.file.fp,
                item.file.admitted_format(),
                Origin::Stored { src: Some(item.file.path.clone()), store: store_path.clone() },
                stamp,
            );
            minted.independent = independent;
            minted.adopt_measurement(*measured);
            let placed_id = minted.id.clone();
            self.library.books.push(book::Row::Book(minted));
            // The seat each copy lands on, resolved in batch order before
            // any write: the root shelf is minted once, and a grouped run
            // cuts each file's rung under it.
            let on_shelf =
                root_shelf.get_or_insert_with(|| self.copies_dest_shelf(&work.dest, stamp)).clone();
            let seat = if work.opts.groups {
                self.copies_rung_shelf(&work.root, &on_shelf, &mut rungs, item.file.subfolder(), stamp)
            } else {
                on_shelf
            };
            landed_ids.push((placed_id, seat));
        }
        let landed = landed_ids.len();
        for (book_id, seat) in &landed_ids {
            if let Some(home) = shelf::find_mut(&mut self.library.shelves, seat) {
                shelf::shelf_add(home, book_id);
            }
        }
        let persist = if landed > 0 { self.persist_library() } else { Task::none() };
        if landed > 0 {
            self.toasts.show(
                Tone::Info,
                format!(
                    "Imported {} from “{}”",
                    lib_text::plural(landed, "book", "books"),
                    paths::dir_label(&work.root)
                ),
                Instant::now(),
            );
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        // Navigate to the shelf the run minted or emptied — the web's light
        // waits on the grid's scroll-to, documented beside the reveal's
        // other half; the way there rides first.
        let reveal = match &work.dest {
            CopiesDest::Into { shelf_id } => Some(shelf_id.clone()),
            _ => root_shelf,
        };
        if let Some(seat) = reveal {
            self.shelf = shelf::find(&self.library.shelves, &seat)
                .and_then(|each| each.parent.clone())
                .unwrap_or_else(|| ALL_SHELF.to_string());
        }
        let release = self.release_root(&work.root);
        Task::batch([persist, release])
    }

    /// The unbound run's root shelf, minted on first use: spliced behind
    /// the shelf the collision named, or the *replace*'s emptied target.
    fn copies_dest_shelf(&mut self, dest: &CopiesDest, stamp: u64) -> String {
        match dest {
            CopiesDest::Into { shelf_id } => shelf_id.clone(),
            CopiesDest::NewShelf { name, after } => {
                let id = library_core::id::next_shelf_id(stamp);
                let at = after
                    .as_deref()
                    .and_then(|after| self.library.shelves.iter().position(|s| s.id == after))
                    .map_or(self.library.shelves.len(), |at| at + 1);
                self.library.shelves.insert(
                    at,
                    shelf::Shelf::virtual_shelf(id.clone(), name.clone(), None),
                );
                id
            }
        }
    }

    /// The bound walk's chain rule on a local map instead of a ledger's: an
    /// unbound run owes no row, so its rungs live only until the landing
    /// files them onto `shelves`. A folder that does not group has no
    /// rungs.
    fn copies_rung_shelf(
        &mut self,
        root: &str,
        dest_shelf: &str,
        rungs: &mut HashMap<String, String>,
        key: &str,
        stamp: u64,
    ) -> String {
        let dir = root.to_string();
        let mut parent = dest_shelf.to_string();
        for rung in folder_ops::key_chain(key) {
            if rung.is_empty() {
                continue;
            }
            if let Some(id) = rungs.get(rung) {
                parent = id.clone();
                continue;
            }
            let id = library_core::id::next_shelf_id(stamp);
            // The rung's name is its directory's own label, the way a bound
            // walk names the rungs its files mint.
            let name =
                rung.rsplit('/').next().filter(|label| !label.is_empty()).unwrap_or(&dir).to_string();
            self.library
                .shelves
                .push(shelf::Shelf::virtual_shelf(id.clone(), name, Some(parent.clone())));
            rungs.insert(rung.to_string(), id.clone());
            parent = id;
        }
        parent
    }

    /// Who holds a root right now, if anyone — the runs and the queued ask
    /// behind them.
    fn root_claim(&self, root: &str) -> Claim {
        if self.queued_ask.as_ref().is_some_and(|queued| {
            let held = match queued {
                QueuedImport::Walk(dir, _, _) | QueuedImport::Copies { dir, .. } => {
                    dir.to_string_lossy()
                }
            };
            held.as_ref() == root
        }) {
            return Claim::HeldByAsk;
        }
        for run in &self.runs {
            let holds = match &run.stage {
                Stage::Walking { root: held, .. }
                | Stage::CopiesScan { root: held, .. } => held == root,
                Stage::Storing { plan } => plan.folder.root == root,
                Stage::Copies { work } => work.root == root,
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
                | Stage::Departing { .. }
                | Stage::CopiesScan { .. }
                | Stage::Copies { .. } => false,
            };
            return if focus_walk { Claim::HeldByWalk } else { Claim::HeldByAsk };
        }
        Claim::Free
    }

    /// A run ended: the ask queued behind its root, if one waited, starts
    /// now — the release the queued ask was promised.
    fn release_root(&mut self, root: &str) -> Task<Message> {
        match self.queued_ask.take() {
            Some(QueuedImport::Walk(dir, opts, plan)) if dir.to_string_lossy() == root => {
                self.begin_folder_walk(dir, opts, Asked::Explicitly, plan)
            }
            Some(QueuedImport::Copies { dir, opts, dest }) if dir.to_string_lossy() == root => {
                self.begin_copies_run(dir, opts, dest)
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
        let (root, opts, asked, plan) = match &self.runs[ix].stage {
            Stage::Walking { root, opts, asked, plan } => {
                (root.clone(), opts.clone(), *asked, plan.clone())
            }
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
        self.plan_folder_walk(ix, task, &root, opts, asked, plan, found)
    }

    /// The diff stage: everything between the walk's raw findings and the
    /// landing. Decides against the ledger the crate owns — tombstones
    /// first, then the registry, then the rung's tracking answer — and
    /// writes nothing but the folder's own row.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn plan_folder_walk(
        &mut self,
        ix: usize,
        task: u64,
        root: &str,
        opts: FolderOpts,
        asked: Asked,
        mut plan: RootPlan,
        mut found: Vec<FoundFile>,
    ) -> Task<Message> {
        let stamp = now_ms();
        // Importing a folder the library already holds continues that row's
        // `placed` and `ignored` sets — the whole point of them: re-importing
        // is how a reader would otherwise get back every book they deleted
        // last week.
        let standing = self.library.folders.iter().find(|folder| folder.root == root).cloned();
        // The *as new* run owes a fresh row beside the old one: placements
        // and quiet logs key off the row id, so a second run of one ground
        // cannot spend the first run's answers.
        let fresh_row = plan.rename.is_some() || standing.is_none();
        let mut folder = if plan.rename.is_some() {
            WatchedFolder::new(library_core::id::next_folder_id(stamp), root, opts.clone())
        } else {
            standing.clone().unwrap_or_else(|| {
                WatchedFolder::new(library_core::id::next_folder_id(stamp), root, opts.clone())
            })
        };
        folder.opts = opts;
        if folder.mode().reads_in_place() {
            if fresh_row {
                // A fresh row's watch is the sheet's switch, at the root;
                // `set_tracking` keeps the tree and the legacy flag agreed.
                folder.set_tracking("", folder.opts.watch);
            } else {
                // A continuation keeps the tree's own answers.
                folder.opts.watch = folder.tracking.tracked();
            }
        }
        folder.prune_shelf_map(&self.library.shelves);
        // The level it joins is the rung it files under from now on: the
        // merge's root rung is that shelf, which makes the answer a promise
        // the next scan keeps.
        if let Some(into) = &plan.into {
            folder.shelf_map.insert(String::new(), into.clone());
        }
        // An *as new* answer owes a tree of its own: every rung mints fresh
        // under the counter-named root.
        if plan.rename.is_some() {
            folder.shelf_map.clear();
        }

        let registry = ledger::registry_of(&self.library.books);
        // The addresses whose file the library already reads in place, where
        // this run lands its own copy beside the linked row: a copies import
        // is the library's second instance, unrelated to the tree reading the
        // ground, and the tree keeps the rows it has.
        let copy_paths: HashSet<String> =
            if folder.mode().copies_files() && asked == Asked::Explicitly {
                ledger::copy_over_paths(&found, &registry, &self.library.books)
            } else {
                HashSet::new()
            };
        ledger::prune_tombstones(&mut folder, &registry);
        // Written on every scan, including one that changes nothing: the
        // restore menu's "moved out of this folder" answer is only as fresh
        // as the last walk.
        folder.record_seen(&found);
        // A file a moved-out log binds to a LIVING row is a book the reader
        // already has: the walk lights that row up rather than landing a
        // second book beside it. A quiet walk asks nothing — its light would
        // be a light the reader did not ask for.
        let represented = if asked == Asked::OnFocus {
            Vec::new()
        } else {
            self.take_represented(Some(folder.id.as_str()), &mut found)
        };

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
        // The ledger answered Skip for the copy run's own files — their
        // content is known — but the reader asked for a second instance of
        // each, so the run owes every one of them a book.
        for file in found.iter().filter(|file| copy_paths.contains(&file.path)) {
            if !adds.iter().any(|each| each.path == file.path) {
                adds.push(file.clone());
            }
        }

        // A merge's new arrivals land on a rung the level already held, and
        // the rows a rename or a merge re-seats follow their own rungs onto
        // the planned tree: one screen answers both, and a question rides the
        // arrival itself rather than a second pass.
        let screened = conflicts::screen_planned(
            &folder,
            &self.library.books,
            &self.library.shelves,
            &registry,
            &found,
            (plan.rename.is_some(), plan.into.as_deref()),
            &mut adds,
            &copy_paths,
        );
        let asks = screened.asks;

        let root_name = paths::dir_label(root);
        if adds.is_empty() && relinks.is_empty() && asks.is_empty() && represented.is_empty() {
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
                if let Some(continuation) = plan.continuation.take() {
                    // The covered walk's empty answer is the note, and the
                    // note's close answers it back to the shelf the pick
                    // meant.
                    self.sheet = Some(Sheet::AlreadyImported {
                        note: LitNote {
                            shelf_id: continuation.shelf_id,
                            name: continuation.name,
                            kind: conflicts::NoteKind::NothingNew,
                        },
                    });
                } else {
                    let line = if found.is_empty() {
                        format!("No documents found in “{root_name}”")
                    } else {
                        format!("Everything in “{root_name}” is already in the library")
                    };
                    self.toasts.show(Tone::Info, line, Instant::now());
                }
            }
            return Task::batch([persist, self.release_root(root)]);
        }

        // The root rung's name, deduped against the shelves standing — two
        // doors of one name are two doors a reader cannot tell apart. A
        // continuation keeps the shelf the map already names.
        let planned_root = match plan.rename.clone() {
            Some(name) => Some(name),
            None => (!folder.shelf_map.contains_key("")).then(|| {
                let names: HashSet<String> =
                    self.library.shelves.iter().map(|each| each.name.clone()).collect();
                if names.contains(&root_name) {
                    book::duplicate_title(&root_name, &names)
                } else {
                    root_name.clone()
                }
            }),
        };
        let adds: Vec<(String, FoundFile)> = adds
            .into_iter()
            .map(|file| (library_core::id::next_id(stamp), file))
            .collect();
        let plan = WalkPlan {
            folder,
            asked,
            continuation: plan.continuation.take(),
            root_name,
            planned_root,
            adds,
            relinks,
            asks,
            replacements: screened.replacements,
            copy_paths,
            represented,
        };

        // A run with nothing to copy skips the store altogether: its only
        // remaining work is the light and the rows a log already answered for.
        if plan.folder.mode().copies_files() && !plan.adds.is_empty() {
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
            Stage::Copies { work } => self.copies_run_done(*work, results),
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

    /// The book half of a move, behind both of its gates: the departure's
    /// copy question first — a read-at-place book leaving the ground that
    /// made it becomes the library's own stored copy, and a copy is a cost
    /// the reader agrees to before anything moves — and then the level's
    /// name screen, where what collides waits on the sheet and what does
    /// not lands now. `departed` names the rows a copy of this very gesture
    /// made: a departure is not a return, so they bind no moved-out log.
    /// True when anything wrote; a gated move writes nothing and waits.
    fn gated_seat(
        &mut self,
        books: &[String],
        from: Option<String>,
        to: String,
        index: Option<usize>,
        departed: &[String],
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
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, &to, index, from.as_deref()),
        );
        let ids = conflicts::clean_move_ids(clean);
        let mut wrote = false;
        if !ids.is_empty() {
            wrote |= library::arrange::move_many_to_shelf(
                &mut self.library.shelves,
                &mut self.library.books,
                &ids,
                from.as_deref(),
                &to,
                index,
            );
            // Every stored book the move lands can bind a folder's moved-out
            // log as a return: the address they share is the bind.
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
        }
        self.raise_conflict(asks);
        wrote
    }

    /// The lift out of one shelf, behind the same two gates: to the
    /// library's own floor is a departure for every read-at-place book a
    /// folder placed, and the floor has names of its own to collide with.
    /// No bind on this door — a lift out arrives nowhere a log could name.
    fn gated_unfile(&mut self, books: &[String], shelf: &str) -> bool {
        let hand = RowMove::Unfile { shelf: shelf.to_string() };
        if self.ask_move_copy(books, ALL_SHELF, hand) {
            return false;
        }
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, ALL_SHELF, None, Some(shelf)),
        );
        let ids = conflicts::clean_move_ids(clean);
        let wrote = if ids.is_empty() {
            false
        } else {
            library::arrange::unfile_books(&mut self.library.shelves, &ids, shelf)
        };
        self.raise_conflict(asks);
        wrote
    }

    /// A second membership — a filing or an "also show": no copy and no
    /// departure, but the level's name screen rides the arrival all the
    /// same, and a stored book landing on a folder's shelf can be the
    /// file's return.
    fn gated_file(&mut self, books: &[String], shelf_id: &str) -> bool {
        let (clean, asks) = conflicts::screen(
            &self.library.books,
            &self.library.shelves,
            conflicts::moved_arrivals(&self.library.books, books, shelf_id, None, None),
        );
        let ids = conflicts::clean_move_ids(clean);
        let mut wrote = false;
        if !ids.is_empty() {
            wrote |= library::arrange::file_many(&mut self.library.shelves, &ids, shelf_id);
            for id in &ids {
                wrote |= departure::bind_returned(
                    &self.library.books,
                    &self.library.shelves,
                    &mut self.library.folders,
                    id,
                    shelf_id,
                );
            }
        }
        self.raise_conflict(asks);
        wrote
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

    /// The sheet's own "Copy" answer, per door: the copies ride a store
    /// batch of their own — one card for the whole gesture, the way fifty
    /// books leaving their ground are one thing the reader asked for — and
    /// the gesture finishes when they come home.
    fn copy_and_finish(&mut self, ask: CopyAsk) -> Task<Message> {
        match ask.work {
            CopyWork::Rows { ids, hand } => self.copy_rows(ids, hand),
            CopyWork::Shelf { ids, target, seam, .. } => self.copy_shelves(ids, target, seam),
            CopyWork::Rung { id } => {
                // The answer walks the rung again, because the sheet was up
                // while the library went on living: a book that went comes
                // back through the folder's own rescan, not this run.
                let books = departure::books_the_rung_takes(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    &id,
                );
                let label = shelf::find(&self.library.shelves, &id)
                    .map(|rung| rung.name.clone())
                    .unwrap_or_else(|| "shelf".to_string());
                self.begin_depart_run(label, books, DepartLand::Rung { id })
            }
            CopyWork::Removal { purge, shelves } => {
                let books = departure::shelf_books(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    &shelves,
                    &purge,
                );
                let label = lib_text::plural(books.len(), "book", "books");
                self.begin_depart_run(label, books, DepartLand::Removal { purge, shelves })
            }
        }
    }

    /// The move door's copies: the answer screens again — the sheet was up
    /// while the library went on living — and the interrupted move resumes
    /// when they come home.
    fn copy_rows(&mut self, ids: Vec<String>, hand: RowMove) -> Task<Message> {
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
            self.advance_conflict();
            return if moved { self.persist_library() } else { Task::none() };
        }
        let label = match converting.len() {
            1 => book::find_row(&self.library.books, &converting[0])
                .map(|row| row.display_name())
                .unwrap_or_else(|| "1 book".to_string()),
            n => format!("{n} books"),
        };
        let work = DepartWork { converting, landing: DepartLand::Move { ids, hand } };
        self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) })
    }

    /// The copies a door bought, as one store batch: the books to convert,
    /// and the landing that finishes the gesture when they come home. A door
    /// whose books all went while the sheet was up lands at once — a rung
    /// with nothing left to copy is still a take-apart, and a removal still
    /// removes.
    fn begin_depart_run(
        &mut self,
        label: String,
        converting: Vec<String>,
        landing: DepartLand,
    ) -> Task<Message> {
        let requests: Vec<BookFileRequest> = converting
            .iter()
            .filter_map(|id| {
                let book = book::find_row(&self.library.books, id)?.book()?;
                Some(BookFileRequest { from: book.path().to_string(), id: id.clone() })
            })
            .collect();
        if requests.is_empty() {
            let failed = if converting.is_empty() { Vec::new() } else { converting };
            let wrote = self.depart_landing(landing, Vec::new(), failed);
            self.advance_conflict();
            return if wrote { self.persist_library() } else { Task::none() };
        }
        let work = DepartWork { converting, landing };
        self.begin_store_run(label, requests, Stage::Departing { work: Box::new(work) })
    }

    /// One spelling for the three shelf hand-moves, because a drag, a bulk
    /// filing and a sibling reorder are one rule and one question. The rule
    /// is the core's own `departing_moves` — pure and host-tested — and the
    /// shelves that owe a departure ride the copy sheet, coming back through
    /// its own answer; the clean ones move now.
    fn screened_shelf_moves(
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
    fn reorder_shelves(&mut self, ids: &[String], anchor: &str, after: bool) -> bool {
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
    fn land_folders(&mut self, folders: &[String], to: &str) -> bool {
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
    fn copy_shelves(
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

    /// The landing the reader bought: the rungs leave the tree with the copy
    /// they paid for, the other folders' shelves that rode along take the
    /// hand's mark, the folder lets the departed zone go, and the copies
    /// wear the level's next free names — then the gesture's own seam or
    /// nest seats what landed. A shelf whose books the store refused entire
    /// stays where it was, with its own sentence.
    fn land_shelf_moves(
        &mut self,
        deps: Vec<ShelfDeparture>,
        level: Option<String>,
        seam: Option<ShelfSeam>,
        departed: &[String],
    ) -> bool {
        let mut landed: Vec<String> = Vec::new();
        for dep in &deps {
            if dep.books.is_empty() || dep.books.iter().any(|id| departed.contains(id)) {
                landed.push(dep.id.clone());
            } else {
                self.toasts.show(
                    Tone::Info,
                    format!(
                        "“{}” stayed where it was — the library could not copy its books.",
                        dep.name
                    ),
                    Instant::now(),
                );
            }
        }
        if landed.is_empty() {
            return false;
        }
        let going: Vec<&ShelfDeparture> =
            deps.iter().filter(|dep| landed.contains(&dep.id)).collect();
        let mut promised: HashSet<String> =
            shelf::children_of(&self.library.shelves, level.as_deref())
                .into_iter()
                .map(|s| s.name.clone())
                .collect();
        for dep in &going {
            for rung in &dep.rungs {
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, rung) {
                    one.kind = library_core::shelf::ShelfKind::Departed;
                    one.manual_parent = false;
                }
            }
            for id in &dep.subtree {
                if dep.rungs.contains(id) {
                    continue;
                }
                if let Some(one) = shelf::find_mut(&mut self.library.shelves, id)
                    && one.is_folder()
                {
                    one.manual_parent = true;
                }
            }
            if let Some(one) = shelf::find_mut(&mut self.library.shelves, &dep.id) {
                one.name = departure::free_name(&dep.name, &mut promised);
            }
        }
        for dep in &going {
            let Some(folder) = folder_ops::find_mut(&mut self.library.folders, &dep.folder_id)
            else {
                continue;
            };
            folder.shelf_map.retain(|key, shelf_id| {
                !folder_ops::key_in_zone(key, &dep.rel) && !dep.rungs.contains(shelf_id)
            });
        }
        let ids: Vec<String> = going.iter().map(|dep| dep.id.clone()).collect();
        // The seats ride the very gestures the screen wraps — clean by
        // construction now, because a Departed rung owes no second copy.
        if let Some(seam) = &seam {
            library::arrange::reorder_shelves_to_anchor(
                &mut self.library.shelves,
                &ids,
                &seam.anchor_id,
                seam.after,
            );
        } else {
            for id in &ids {
                library::arrange::nest_shelf(&mut self.library.shelves, id, level.as_deref());
            }
        }
        true
    }

    /// No copies: every mover that has a way home takes it, and a mover that
    /// has none stays where the tree put it. The fold is the import's own
    /// `reclaim_rung`, and the reseat rides the very `nest_shelf` the
    /// gesture did. The reveal that lights the first seated shelf waits on
    /// the reveal's own light.
    fn take_them_home(&mut self, returns: &[(String, ReturnPath)]) -> bool {
        let mut moved = false;
        for (shelf_id, path) in returns {
            moved |= match path {
                ReturnPath::Reclaim { tree, gone, rel } => {
                    self.reclaim_rung(tree, gone, rel, shelf_id).is_some()
                }
                ReturnPath::Reseat { seat } => library::arrange::nest_shelf(
                    &mut self.library.shelves,
                    shelf_id,
                    seat.as_deref(),
                ),
            };
        }
        moved
    }

    /// Put a displaced member back on the rung its directory names and fold
    /// the folder that was reading it into the tree that contains it: one
    /// ground, one reader from here on. Three writes, in the order that
    /// keeps them honest; the answer is the shelf the member sat on.
    ///
    /// A tree a walk holds is the one fold to refuse, because that walk's
    /// clone of the ledger lands after this write and drops it. Persists
    /// nothing itself: the caller ends its own transaction.
    fn reclaim_rung(
        &mut self,
        tree_id: &str,
        gone_id: &str,
        rel: &str,
        shelf_id: &str,
    ) -> Option<String> {
        let now = now_ms();
        let mut minted: Vec<Shelf> = Vec::new();
        let mut tree = folder_ops::find(&self.library.folders, tree_id)?.clone();
        let gone = folder_ops::find(&self.library.folders, gone_id)?.clone();
        // Read before anything is written: the answer is about the shelves
        // standing, not the ones this move mints.
        let rungs = member_rungs(&self.library.shelves, gone_id, rel);
        // A shelf that went while the sheet was up is an answer with nothing
        // to move; so is a tree a walk holds, whose ledger clone lands after
        // this write.
        let foreign_walk = !matches!(self.root_claim(&tree.root), Claim::Free);
        if !rungs.iter().any(|(_, id)| id == shelf_id) || foreign_walk {
            return None;
        }
        let root = tree.root.clone();
        // Ground the shape keeps on one shelf has no rung for the member's
        // directory to become: its books come onto the rung the ground
        // answers for and its own shelves go, so an adoption cannot cut a
        // nested rung into an import that asked for none.
        let seat = if tree.cuts(rel) {
            let parent = chain_for(
                &mut tree,
                folder_ops::parent_key(rel).unwrap_or(""),
                now,
                None,
                &root,
                &mut minted,
            );
            for (key, id) in &rungs {
                tree.shelf_map.insert(key.clone(), id.clone());
            }
            page_into(&mut self.library.shelves, minted);
            let nestable = shelf::can_nest(&self.library.shelves, shelf_id, &parent);
            for (key, id) in &rungs {
                let Some(one) = shelf::find_mut(&mut self.library.shelves, id) else {
                    continue;
                };
                one.kind = library_core::shelf::ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(key),
                };
                if id != shelf_id {
                    continue;
                }
                if nestable {
                    one.parent = Some(parent.clone());
                    one.manual_parent = false;
                } else {
                    one.manual_parent = true;
                }
            }
            shelf_id.to_string()
        } else {
            // A pointer to a shelf that went is no seat, and this tree's map
            // is not the one the run pruned: the mint is asked for the key
            // the map has no standing rung under.
            let standing = tree
                .shelf_map
                .get("")
                .filter(|id| shelf::find(&self.library.shelves, id).is_some())
                .cloned();
            let seat = match standing {
                Some(seat) => seat,
                None => {
                    tree.shelf_map.remove("");
                    chain_for(&mut tree, "", now, None, &root, &mut minted)
                }
            };
            page_into(&mut self.library.shelves, minted);
            flatten_rungs(&mut self.library.shelves, gone_id, &seat);
            seat
        };
        // The answer the folded row carried for its own root becomes the
        // rung it becomes: the row that answer was written on is the one
        // this fold retires. A tree that cuts no rungs has one answer for
        // the whole of its ground — the reader's own about its root — and
        // the adoption does not second-guess it.
        if tree.cuts(rel) {
            tree.set_tracking(rel, gone.opts.watch);
        }
        tree.placed.extend(gone.placed.iter().copied());
        for stone in gone.ignored.iter() {
            if !tree.is_ignored(&stone.fp) {
                tree.ignored.push(stone.clone());
            }
        }
        tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);
        self.library.folders.retain(|f| f.id != gone_id);
        match self.library.folders.iter().position(|f| f.id == tree_id) {
            Some(at) => self.library.folders[at] = tree,
            None => self.library.folders.push(tree),
        }
        Some(seat)
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
        let failed: Vec<String> = work
            .converting
            .iter()
            .filter(|id| !departed.contains(*id))
            .cloned()
            .collect();
        let any_copies = !departed.is_empty();
        let moved = self.depart_landing(work.landing, departed, failed);
        self.advance_conflict();
        if moved || any_copies {
            return self.persist_library();
        }
        Task::none()
    }

    /// The gesture a copy run finishes. A move resumes with what landed — a
    /// copy that failed costs that book its move and nothing else: it stays
    /// where it was, and every screen downstream sees the books as what they
    /// are about to be — except a replace, which seats its row whatever the
    /// copy did: a failed copy re-asks at the seat's own gate. A rung comes
    /// apart only once every book it owed is safe: a book the store refused
    /// leaves the shelf standing, so the reader can ask again rather than
    /// lose the ground the rest of the rung answers to. Shelves land as the
    /// take-out wrote them: a shelf whose books the store refused entire
    /// stays where it was. A removal runs whatever the copies did: the sheet
    /// promised the removal, and the copies were the books' own way out of
    /// it.
    fn depart_landing(
        &mut self,
        landing: DepartLand,
        departed: Vec<String>,
        failed: Vec<String>,
    ) -> bool {
        match landing {
            DepartLand::Move { ids, hand } => {
                let resume_ids: Vec<String> = match &hand {
                    RowMove::Replaced { .. } => ids.clone(),
                    _ => ids.iter().filter(|id| !failed.contains(*id)).cloned().collect(),
                };
                if resume_ids.is_empty() {
                    return false;
                }
                self.resume_move(hand, resume_ids, departed)
            }
            DepartLand::ShelfMove { deps, level, seam } => {
                self.land_shelf_moves(deps, level, seam, &departed)
            }
            DepartLand::Rung { id } => {
                if !failed.is_empty() {
                    return false;
                }
                self.dismantle_shelf(&id)
            }
            DepartLand::Removal { purge, shelves } => self.remove(purge, shelves),
        }
    }

    /// The gesture a copy question interrupted, finished: the same rows land
    /// through the same doors — a seat re-runs its own gate and screen,
    /// which the copies that came home pass by construction — and the copies
    /// land marked: a departure is not a return, so a copied book binds no
    /// folder's moved-out log.
    fn resume_move(&mut self, hand: RowMove, ids: Vec<String>, departed: Vec<String>) -> bool {
        match hand {
            RowMove::Seat { from, to, index } => self.gated_seat(&ids, from, to, index, &departed),
            RowMove::Row { to, index } => {
                let gone = !departed.is_empty();
                let mut wrote = false;
                for id in &ids {
                    wrote |= self.move_row(id, &to, index, gone);
                }
                wrote
            }
            RowMove::Replaced { to, index, inherited } => {
                let gone = !departed.is_empty();
                let mut wrote = false;
                for id in &ids {
                    self.seat_replace(id, &to, index, &inherited, gone);
                    wrote = true;
                }
                wrote
            }
            RowMove::Unfile { shelf } => self.gated_unfile(&ids, &shelf),
        }
    }

    /// One row's own move: off every shelf it was on and onto the one named,
    /// at the slot the drop pointed at. The root has no member list, so a
    /// move there is the lift out of every shelf. The single row rides the
    /// same departure gate as every hand-move — by the time a conflict
    /// answer reaches here the gate's screen is empty by construction, but
    /// an "as new" renames a row a filing converted, and that seat asks its
    /// own question. `departed` is the copy's own answer: a departure binds
    /// no moved-out log.
    fn move_row(&mut self, row_id: &str, to: &str, index: Option<usize>, departed: bool) -> bool {
        let one = [row_id.to_string()];
        let hand = RowMove::Row { to: to.to_string(), index };
        if self.ask_move_copy(&one, to, hand) {
            return false;
        }
        shelf::forget_everywhere(&mut self.library.shelves, row_id);
        if to == ALL_SHELF {
            if index.is_some() {
                library::arrange::reorder_root(&mut self.library.books, &one, index);
            }
        } else {
            if let Some(shelf) = shelf::find_mut(&mut self.library.shelves, to) {
                shelf::place(&mut shelf.books, row_id, index);
            }
            if !departed {
                departure::bind_returned(
                    &self.library.books,
                    &self.library.shelves,
                    &mut self.library.folders,
                    row_id,
                    to,
                );
            }
        }
        true
    }

    /// A sheet already up takes new asks onto its queue rather than being
    /// replaced: two drops in flight owe two answers.
    fn raise_conflict(&mut self, asks: Vec<ConflictAsk>) {
        if asks.is_empty() {
            return;
        }
        if self.sheet.is_some() {
            self.conflict_waiting.extend(asks);
            return;
        }
        let mut asks = asks;
        let first = asks.remove(0);
        self.conflict_waiting.extend(asks);
        self.sheet = Some(Sheet::Conflict { ask: first });
        self.apply_all = false;
    }

    /// The next question of the queue, when the slot is free: an answered
    /// sheet stays up while questions wait, an answer that raised a sheet of
    /// its OWN — a replace's copy question — keeps it, and a queue nothing
    /// drained shows the moment a slot opens.
    fn advance_conflict(&mut self) {
        if self.sheet.is_some() || self.conflict_waiting.is_empty() {
            return;
        }
        let ask = self.conflict_waiting.remove(0);
        self.sheet = Some(Sheet::Conflict { ask });
        self.apply_all = false;
    }

    /// What dropping this question means: the conflict sheet's close — its
    /// Cancel, its scrim, its Escape — skips the question on screen and
    /// every one behind it; placements already answered keep their answers.
    fn dismiss_sheet(&mut self) {
        if matches!(self.sheet, Some(Sheet::Conflict { .. })) {
            self.conflict_waiting.clear();
        }
        self.sheet = None;
    }

    /// The click's own dispatch: the question's offers are the guard, so a
    /// row the sheet rendered is a row the answer takes. The shelf scope —
    /// a folder's own name collision — arrives with the import's screen;
    /// every ask a move raises is about a row.
    fn apply_placement(&mut self, ask: &ConflictAsk, choice: Placement) -> Task<Message> {
        let offers = conflicts::offers_for(&self.library.books, &self.library.folders, ask);
        if !offers.contains(&choice) {
            return Task::none();
        }
        match choice {
            Placement::Open => self.reveal_existing(&ask.existing_id),
            Placement::KeepBoth => self.as_new(ask),
            Placement::LinkOnly => self.link_to_row(ask),
            Placement::Merge => self.merge_into_row(ask),
            Placement::Replace => self.replace_row(ask),
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
    fn reveal_shelf(&mut self, shelf_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_shelf(&self.library.shelves, shelf_id);
        self.light(shelf_id);
        self.reveal_scroll(shelf_id)
    }

    /// The row half of the same light: the level the card lives on, then
    /// the scroll and the ring — in that order, because the reveal's whole
    /// story is that the way and the light never disagree.
    fn reveal_book(&mut self, book_id: &str) -> Task<Message> {
        self.shelf = reveal::level_of_row(&self.library.shelves, book_id);
        self.light(book_id);
        self.reveal_scroll(book_id)
    }

    /// "Already imported": the whole of the reveal — no row, and the
    /// reader lands where the answer is. From 3g's own answers onward
    /// every way out of that question ends on the same light.
    fn reveal_existing(&mut self, book_id: &str) -> Task<Message> {
        self.reveal_book(book_id)
    }

    /// "As new": a moved row is renamed and then moved — the rename is
    /// what frees the collision, and a move that did not rename would ask
    /// the same question again on the way in. An import's file answer rides
    /// the single-file copy: made and measured before the row is promised,
    /// minted under the name the sheet showed.
    fn as_new(&mut self, ask: &ConflictAsk) -> Task<Message> {
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
    fn link_to_existing(&mut self, ask: &ShelfConflictAsk) -> Task<Message> {
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
    fn copies_beside_tree_run(&mut self, ask: ShelfConflictAsk) -> Task<Message> {
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
    fn replace_with_tree(&mut self, ask: ShelfConflictAsk) -> Task<Message> {
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
    fn settle_ledger(&mut self, folder_id: Option<&str>, fp: Fingerprint) {
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
    fn apply_folder_merge(&mut self, ask: &ConflictAsk, answer: Placement) -> Task<Message> {
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
    fn link_to_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
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
    fn merge_into_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
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
    fn replace_row(&mut self, ask: &ConflictAsk) -> Task<Message> {
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
    fn seat_replace(
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
    fn unlist_row(&mut self, id: &str) {
        book::remove_row(&mut self.library.books, id);
        book::drop_dangling_links(&mut self.library.books);
        shelf::forget_everywhere(&mut self.library.shelves, id);
    }

    /// The row's own byte, once no row reads it — the twin rule's other
    /// half: two rows of one file share an address, and a sweep that forgot
    /// the twin would delete the file the survivor reads. A linked book's
    /// bytes are the reader's and are never touched.
    fn sweep_row_bytes(&mut self, book: &Book) {
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

        let mut found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
        if found.is_empty() {
            self.runs.remove(ix);
            self.toasts.show(
                Tone::Error,
                "None of those files could be opened.",
                Instant::now(),
            );
            return if changed { self.persist_library() } else { Task::none() };
        }
        // The files a folder's own log already answers for: the reader has
        // these books, so the drop lights them up rather than landing copies
        // beside them.
        let represented = self.take_represented(None, &mut found);

        let stamp = now_ms();
        let shelf_id = target.clone().unwrap_or_else(|| ALL_SHELF.to_string());
        let mut restored = 0usize;

        // A file a read-at-place tree holds is that folder's business
        // first: a row already reading the address raises the folder's
        // two-answer question, and a removal the tree logged comes back as
        // the tree's own linked book.
        let mut covered_asks: Vec<ConflictAsk> = Vec::new();
        found.retain(|file| {
            let covering = self
                .library
                .folders
                .iter()
                .filter(|folder| folder.mode().reads_in_place())
                .find(|folder| rel_under(&file.path, &folder.root).is_some())
                .map(|folder| folder.id.clone());
            let Some(folder_id) = covering else {
                return true;
            };
            let held_at_address = book::book_rows(&self.library.books)
                .find(|each| each.path() == file.path)
                .map(|each| each.id.clone());
            if let Some(row_id) = held_at_address {
                let existing_name = book::find_row(&self.library.books, &row_id)
                    .map(|row| row.display_name())
                    .unwrap_or_else(|| file.rel.clone());
                covered_asks.push(ConflictAsk::covered(
                    Arrival::import(file.clone(), shelf_id.clone(), None),
                    row_id,
                    existing_name,
                    folder_id,
                ));
                return false;
            }
            let stone = folder_ops::find(&self.library.folders, &folder_id)
                .and_then(|folder| ledger::find_tombstone(folder, &file.fp).cloned());
            let placed_by_folder = folder_ops::find(&self.library.folders, &folder_id)
                .is_some_and(|folder| folder.placed.contains(&file.fp));
            if stone.is_some() || placed_by_folder {
                self.restore_covered(file, &folder_id, stone.as_ref(), stamp);
                restored += 1;
                changed = true;
                return false;
            }
            true
        });

        // Content the library already holds is content the reader already
        // has, wherever it is filed: the two-answer question asks whether
        // the drop meant a second instance or the one they have. A row
        // whose address died is no answer — the copy lands beside it.
        let mut held_asks: Vec<ConflictAsk> = Vec::new();
        found.retain(|file| {
            let Some(held) = ledger::existing_for(&self.library.books, file.fp) else {
                return true;
            };
            if held.missing {
                return true;
            }
            let existing_name = book::find_row(&self.library.books, &held.row_id)
                .map(|row| row.display_name())
                .unwrap_or_else(|| file.rel.clone());
            held_asks.push(ConflictAsk::already_have(
                Arrival::import(file.clone(), shelf_id.clone(), None),
                held.row_id.clone(),
                existing_name,
            ));
            false
        });

        // Every file whose name the level already holds is a question
        // rather than a placement; a twin on another shelf is not one.
        let arrivals: Vec<Arrival> = found
            .iter()
            .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
            .collect();
        let (clean, name_asks) =
            conflicts::screen(&self.library.books, &self.library.shelves, arrivals);

        let mut pending: Vec<PendingCopy> = Vec::new();
        for arrival in &clean {
            let Some(file) = arrival.file.clone() else {
                continue;
            };
            // A removal any folder logged against this content is spent by
            // the explicit ask: the name it remembered rides onto the copy.
            let title = self.lift_stone_for(&file.fp);
            pending.push(PendingCopy {
                book_id: library_core::id::next_id(stamp),
                file,
                title,
            });
        }
        let mut asks = covered_asks;
        asks.extend(held_asks);
        asks.extend(name_asks);

        if pending.is_empty() {
            self.runs.remove(ix);
            if restored > 0 {
                self.toasts.show(
                    Tone::Info,
                    format!("{} came back", lib_text::plural(restored, "book", "books")),
                    Instant::now(),
                );
            }
            self.raise_conflict(asks);
            // A drop the library already had in every file it named is a light
            // on the first book a log answered for: the copy's own landing
            // never runs, so the light rides this door instead.
            let light = match represented.first() {
                Some(book_id) => self.reveal_book(book_id),
                None => Task::none(),
            };
            return if changed {
                Task::batch([self.persist_library(), light])
            } else {
                light
            };
        }

        let requests: Vec<BookFileRequest> = pending
            .iter()
            .map(|item| BookFileRequest { from: item.file.path.clone(), id: item.book_id.clone() })
            .collect();
        let sink = Arc::clone(&self.runs[ix].sink);
        let task_name = task.to_string();
        self.runs[ix].stage = Stage::Copying {
            plan: Box::new(FilesPlan {
                target,
                pending,
                restored,
                asks,
                represented,
                settle: None,
                index: None,
            }),
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

    /// The files a folder's moved-out log already answers for: a log bound to
    /// a LIVING row says the file's book is still here — the row a shelf move
    /// or a link-making answer left standing for it — so an import of that
    /// file succeeds by lighting the row up rather than landing a second book
    /// beside it. `scope` narrows the search to one folder's own log; the
    /// loose run asks every folder at once.
    fn take_represented(&self, scope: Option<&str>, found: &mut Vec<FoundFile>) -> Vec<String> {
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
        let FilesPlan { target, pending, restored, asks, represented, settle, index } = *plan;
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
                shelf::place(&mut home.books, placed_id, index);
            }
        }
        // The answer's own ledger write, spent only when the copy it
        // waited for actually landed.
        if landed > 0
            && let Some((folder_id, fp)) = settle
        {
            self.settle_ledger(Some(&folder_id), fp);
        }

        let added = landed + restored;
        if added > 0 {
            let persist = self.persist_library();
            if let Some(error) = failure {
                self.toasts.show(Tone::Error, error, Instant::now());
            } else {
                let line = if target.is_some() {
                    format!("Added {} to this shelf", lib_text::plural(added, "book", "books"))
                } else {
                    format!("Added {}", lib_text::plural(added, "book", "books"))
                };
                self.toasts.show(Tone::Info, line, Instant::now());
            }
            self.raise_conflict(asks);
            let light = match represented.first() {
                Some(book_id) => self.reveal_book(book_id),
                None => Task::none(),
            };
            return Task::batch([persist, light]);
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        // A drop that landed nothing new but answered for a book the reader
        // already had is a light on that book rather than a receipt of zero:
        // the log remembered it, so the reader is taken to it.
        let light = match represented.first() {
            Some(book_id) => self.reveal_book(book_id),
            None => Task::none(),
        };
        self.raise_conflict(asks);
        light
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
    /// the next rescan quiet about it. The level's name screen rides the
    /// filing's own gate, as it does for every second membership.
    fn also_show(&mut self, book_id: &str, shelf_id: &str) -> Task<Message> {
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
                library::SelectionFacts {
                    selecting: self.selecting,
                    selected: &self.selected,
                    lit: self.reveal.as_ref().map(|(revealed, _)| revealed.id.as_str()),
                },
                library::DragFacts {
                    payload: self.drag.as_ref().map(|drag| &drag.payload),
                    effect: answer.as_ref().map(|(effect, _)| effect),
                },
            ),
            Route::Reader => self.reader.view(self.tokens).map(Message::Reader),
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
                let view_trigger = button(icon(IconName::More, 15, wash(self.tokens.ink, factor)))
                    .padding(7.0)
                    .style(move |_, status| titlebar::ghost_button_style(self.tokens, factor, status))
                    .on_press(Message::ToggleMenu(MenuKind::View));
                let appearance =
                    button(icon(appearance_glyph(self.settings.appearance.base), 15, wash(self.tokens.ink, factor)))
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

        let title = self.shown_title();

        titlebar::view(
            &self.titlebar,
            titlebar::ViewContext {
                tokens: self.tokens,
                route: self.route,
                maximized: self.maximized,
                factor,
                title,
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
                sheet::panel(self.tokens, ask.action.as_str(), body.into(), actions)
            }
            // The name question: the arriving name as the heading, where the
            // collision is — and what waits behind it — under that, the
            // question in one sentence, and the answers as rows that each
            // promise what choosing them does. Cancel means the same thing
            // on every sheet: leave the shelf as it is, and skip the queue.
            Sheet::Conflict { ask } => {
                let spec = conflicts::describe(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    ask,
                    &self.conflict_waiting,
                );
                let apply_all = self.apply_all;
                let choices = spec
                    .choices
                    .iter()
                    .map(|choice| {
                        sheet::choice_row(
                            self.tokens,
                            choice.label,
                            choice.note.clone(),
                            Message::AnswerPlacement(choice.placement, apply_all),
                        )
                    })
                    .collect();
                let mut body = Column::new().spacing(10);
                body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
                body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
                body = body.push(sheet::choice_group(self.tokens, choices));
                // Rendered only when questions are actually waiting — a
                // switch offering to answer nothing is a control that lies
                // about its reach.
                if spec.apply_all && spec.waiting > 0 {
                    body = body.push(apply_all_row(self.tokens, spec.waiting, apply_all));
                }
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    spec.heading.clone(),
                    body.into(),
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
                )
            }
            Sheet::ShelfConflict { ask } => {
                // Asked before the walk rather than after it: the answers
                // name a run rather than a placement, so the sheet renders
                // the same chrome and routes its rows down a second lane.
                let spec = conflicts::describe_shelf(
                    &self.library.books,
                    &self.library.shelves,
                    &self.library.folders,
                    ask,
                );
                let choices = spec
                    .choices
                    .iter()
                    .map(|choice| {
                        sheet::choice_row(
                            self.tokens,
                            choice.label,
                            choice.note.clone(),
                            Message::AnswerShelf(choice.placement),
                        )
                    })
                    .collect();
                let mut body = Column::new().spacing(10);
                body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
                body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
                body = body.push(sheet::choice_group(self.tokens, choices));
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    spec.heading.clone(),
                    body.into(),
                    vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
                )
            }
            Sheet::AlreadyImported { note } => {
                // Not a question — an answer: the shelf is named why, and
                // closing is the label on the button.
                let sentence = conflicts::note_sentence(note.kind, &note.name);
                let mut body = Column::new().spacing(10);
                body = body.push(
                    text(note.kind.sublabel().to_string()).size(12).color(self.tokens.muted),
                );
                body = body.push(text(sentence).size(12).color(self.tokens.muted));
                sheet::panel_sized(
                    self.tokens,
                    sheet::CONFLICT_W,
                    note.name.clone(),
                    body.into(),
                    vec![sheet::confirm_button(
                        self.tokens,
                        "Show the shelf",
                        Message::CloseAlreadyImported,
                        false,
                    )],
                )
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
            rows.push(popover::item(
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

    /// The window's own title: what iced hands the OS. Owned because the
    /// platform wants a value, which is the only difference from the bar's
    /// centre.
    fn title(&self) -> String {
        self.shown_title().to_owned()
    }

    /// The name the window and the bar's centre both show.
    ///
    /// The reader route hands it to the document — the web app's floating
    /// label — so both say what is being read rather than what the app is. A
    /// stored copy's name comes from its row, which is why this asks the
    /// reader rather than the file name. Borrowed, because the bar's element
    /// outlives the frame that built it.
    fn shown_title(&self) -> &str {
        match self.route {
            Route::Reader if self.reader.document.is_open() => self.reader.name(),
            _ => APP_TITLE,
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
        // The engine's answers. One engine lives for the whole run, so the
        // recipe is a constant in the tree rather than something a route
        // builds and tears down.
        subscriptions.push(self.reader.subscription().map(Message::Reader));
        // Frames flow only while something is in motion: the reveal
        // animating, a hide waiting out its grace, or a toast waiting out
        // its stamp. An idle window subscribes to nothing and costs no
        // redraws.
        if self.titlebar.needs_tick(now)
            || self.toasts.needs_tick(now)
            || self.reader.needs_tick()
            || self.press.is_some()
            || self.drag.is_some()
            || self.ellipsis_close_at.is_some()
            || self.reveal.is_some()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// The rungs a folded member brings: every shelf the folded folder owns,
/// keyed the way the receiving tree keys its own — the rung the member's
/// directory names, and the ones below it.
fn member_rungs(shelves: &[Shelf], gone_id: &str, rel: &str) -> Vec<(String, String)> {
    shelves
        .iter()
        .filter(|s| s.kind.folder_id() == Some(gone_id))
        .filter_map(|s| {
            let library_core::shelf::ShelfKind::Folder { rel: own, .. } = &s.kind else {
                return None;
            };
            let own = own.as_deref().unwrap_or("");
            let key =
                if own.is_empty() { rel.to_string() } else { format!("{rel}/{own}") };
            Some((key, s.id.clone()))
        })
        .collect()
}

/// The map's key as the shelf's own rel: the root's empty key is no rel.
fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// The minted rungs join the list: one push per shelf no id already holds.
fn page_into(shelves: &mut Vec<Shelf>, minted: Vec<Shelf>) {
    for made in minted {
        if !shelves.iter().any(|s| s.id == made.id) {
            shelves.push(made);
        }
    }
}

/// The whole of one tree's books onto `seat`, and the shelves the one-shelf
/// answer has no place for taken out: the flattening a re-import asks for,
/// and the one an adopting tree takes a member in by. Every rung of `from`
/// goes except the one that is `seat`. Books move, readers do not: a shelf
/// the reader made inside one comes up to `seat` with its books.
fn flatten_rungs(shelves: &mut Vec<Shelf>, from: &str, seat: &str) {
    let own: Vec<String> =
        shelf::rungs_of(shelves, from).into_values().map(String::from).collect();
    let going: HashSet<String> = own.iter().filter(|id| id.as_str() != seat).cloned().collect();
    // A set rather than a growing list: the whole tree's books pass through
    // here, and membership was a scan per book.
    let mut held: HashSet<String> = HashSet::new();
    for rung in &own {
        let Some(one) = shelf::find(shelves, rung) else {
            continue;
        };
        held.extend(one.books.iter().cloned());
    }
    if let Some(one) = shelf::find_mut(shelves, seat) {
        for book_id in held {
            shelf::shelf_add(one, &book_id);
        }
    }
    for one in shelves.iter_mut() {
        if one.parent.as_deref().is_some_and(|parent| going.contains(parent)) {
            one.parent = Some(seat.to_string());
        }
    }
    shelves.retain(|one| !going.contains(&one.id));
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
        // The page turns and the zoom ladder, and they come last on purpose:
        // `Escape` and `Enter` are matched above, and a key this arm declines
        // falls through to the same `None` every unhandled key does. A focused
        // field keeps its own keys — that is what the status guard says — so the
        // reader hears only what the fields have no use for.
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            // The zoom ladder's keys are the web app's own, and they are PLAIN
            // presses there: with a modifier the key belongs to the window's
            // shortcuts (⌘F, ⌘O, ⌘←/→) and the reader never sees it, so the
            // same guard holds here.
            let plain = !(modifiers.control() || modifiers.alt() || modifiers.logo());
            if plain {
                let zoom = match key.as_ref() {
                    keyboard::Key::Character("+") | keyboard::Key::Character("=") => Some(1),
                    keyboard::Key::Character("-") | keyboard::Key::Character("_") => Some(-1),
                    _ => None,
                };
                if let Some(dir) = zoom {
                    return Some(Message::Reader(reader::Message::Zoom(reader::Command::Step(
                        dir,
                    ))));
                }
            }
            let step = match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
                | keyboard::Key::Named(keyboard::key::Named::PageUp) => -1,
                keyboard::Key::Named(keyboard::key::Named::ArrowRight)
                | keyboard::Key::Named(keyboard::key::Named::PageDown) => 1,
                _ => 0,
            };
            // Both directions are one message, so the surface's own turn logic
            // — the clamp at either end of the book, and the fit that follows a
            // differently sized sheet — stays the only place a turn is decided.
            (step != 0).then_some(Message::Reader(reader::Message::Turn(step)))
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
            shadow: Elevation::Pill.shadow(),
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
                shadow: Elevation::Pill.shadow(),
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
            Stage::Walking { .. } | Stage::Storing { .. } | Stage::CopiesScan { .. } => {
                format!("Scanning “{}”…", run.label)
            }
            Stage::Copies { .. } => format!("Copying “{}”…", run.label),
        },
    }
}

/// The import sheet's body: the folder, the formats, the size threshold,
/// how the books are held and the structure answer — the walk's orders,
/// written before it walks. `ground` is the tree the pick belongs to, when
/// one governs it: the notes speak the rung's own promise then, not the
/// whole import's.
#[allow(clippy::too_many_lines)]
/// The "apply to all" row: one switch that gives every waiting question of
/// the same kind the answer being clicked.
fn apply_all_row(tokens: Tokens, waiting: usize, on: bool) -> Element<'static, Message> {
    let knob = button(
        text(if on { "On" } else { "Off" })
            .size(12)
            .color(if on { tokens.ink } else { tokens.muted }),
    )
    .padding(Padding { top: 5.0, right: 14.0, bottom: 5.0, left: 14.0 })
    .style(move |_, status| {
        let background = if on {
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
                color: if on { tokens.accent } else { tokens.line },
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: if on { tokens.ink } else { tokens.muted },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleApplyAll);
    container(
        row![
            container(
                text(format!("Apply to all {}", waiting + 1)).size(12).color(tokens.muted)
            )
            .width(Length::Fill),
            knob,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_| container::Style {
        border: Border { color: tokens.line, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    })
    .into()
}

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
                shadow: Elevation::Plate.shadow(),
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
            shadow: Elevation::Badge.shadow(),
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
            shadow: Elevation::Float.shadow(),
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
