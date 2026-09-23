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

use iced::time::Instant;
use iced::widget::{
    button, column, container, mouse_area, operation, row, stack, text, text_input, Column, Id,
    Row, Space,
};
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
use library_core::ledger::{self, ScanAction};
use library_core::paths;
use library_core::scan::{selectable_formats, FoundFile};
use library_core::shelf::{self, ALL_SHELF};
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
use crate::library::{self, bar, menus};
use crate::platform::{dialogs, fs, progress, store};
use crate::route::Route;
use crate::storage;
use crate::theme::{self, fade, wash, Tokens};
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
}

/// One right-click, answered: what was asked about, and where the pointer
/// stood when it asked.
#[derive(Debug, Clone)]
struct ContextRequest {
    target: ContextTarget,
    at: Point,
}

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
    /// A folder about to be imported: the options sheet decides what the
    /// walk admits, and how the books are held.
    Import { root: PathBuf },
}

/// Which question a folder run answers; a boolean at the signature could
/// not say. An ask is the reader's own import; a walk is the app keeping a
/// watched folder's promise to itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Explicitly,
    OnFocus,
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

/// The store batch's answer, keyed: the book id each copy was requested
/// under to the address it landed at and the measurement of its own bytes.
type CopyMap = HashMap<String, (String, Option<Fingerprint>)>;

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
    /// The last known pointer position in window coordinates — the anchor
    /// a menu is placed at.
    cursor: Point,
    /// The window's size — the viewport a menu is clamped into.
    viewport: Size,
    /// The card the pointer is over: the grid's hover truth.
    hovered_card: Option<String>,
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
            cursor: Point::new(600.0, 400.0),
            viewport: Size::new(1200.0, 800.0),
            hovered_card: None,
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
                self.menu = None;
                self.context = None;
                // A rename in flight belongs to the shelf it started on;
                // stepping away abandons it rather than carrying the draft.
                self.renaming = false;
                self.hovered_card = None;
                self.shelf = shelf;
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
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                self.renaming = false;
                Task::none()
            }
            Message::EscapePressed => {
                // Escape closes the topmost thing: a sheet, then a
                // right-click menu, then a bar panel, then a rename.
                if self.sheet.is_some() {
                    self.sheet = None;
                    return Task::none();
                }
                if self.context.is_some() {
                    self.context = None;
                    return Task::none();
                }
                if self.renaming {
                    self.renaming = false;
                    return Task::none();
                }
                self.menu = None;
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
            Message::ContextMenu(target) => {
                self.menu = None;
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
                let Some(path) = book::find_by_id(&self.library.books, &id)
                    .map(|book| PathBuf::from(book.path()))
                else {
                    return Task::none();
                };
                self.open_document_path(path)
            }
            Message::CardHover(hovered) => {
                self.hovered_card = hovered;
                Task::none()
            }
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
                    self.sheet = Some(Sheet::Import { root: dir });
                    Task::none()
                }
                None => Task::none(),
            },
            Message::ScanDone(task, result) => self.scan_done(task, result, now),
            Message::CopiesDone(task, results) => self.copies_done(task, results),
            Message::FilesChecked(task, checks) => self.files_checked(task, checks),
            Message::FilesCopied(task, results) => self.files_copied(task, results),
            Message::ChecksDone(checks) => self.checks_done(checks),
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
                        self.sheet = Some(Sheet::Import { root: path });
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
    fn create_shelf(&mut self) -> Task<Message> {
        let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        self.create_shelf_in(parent)
    }

    /// Mint a shelf under an explicit parent — `None` hangs it at the top
    /// level — and step into it.
    fn create_shelf_in(&mut self, parent: Option<String>) -> Task<Message> {
        let now = now_ms();
        let id = library_core::id::next_shelf_id(now);
        let in_use: HashSet<String> =
            self.library.shelves.iter().map(|shelf| shelf.name.clone()).collect();
        // The first one is simply "New shelf"; the counter starts only once
        // that name is taken.
        let name = if in_use.contains("New shelf") {
            book::duplicate_title("New shelf", &in_use)
        } else {
            "New shelf".to_string()
        };
        self.library
            .shelves
            .push(shelf::Shelf::virtual_shelf(id.clone(), name, parent));
        self.menu = None;
        self.context = None;
        self.shelf = id;
        self.persist_library()
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
        let Some(gone) = shelf::find(&self.library.shelves, id) else {
            return Task::none();
        };
        let step_out = gone.parent.clone().unwrap_or_else(|| ALL_SHELF.to_string());
        let gone_id = gone.id.clone();
        // Standing on the shelf that goes — or somewhere inside it — means
        // stepping out to the level it hung from.
        let standing_within =
            shelf::subtree_ids(&self.library.shelves, std::slice::from_ref(&gone_id))
                .contains(&self.shelf);
        shelf::lift_children(&mut self.library.shelves, &gone_id);
        self.library.shelves.retain(|shelf| shelf.id != gone_id);
        if standing_within {
            self.shelf = step_out;
        }
        self.persist_library()
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
            Sheet::Import { root } => {
                let opts = self.import_opts.clone();
                self.begin_folder_walk(root, opts, Asked::Explicitly)
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
        let line = if doomed.is_some() {
            format!("Removed “{name}” from the library")
        } else {
            format!("Removed the link “{name}”")
        };
        self.toasts.show(Tone::Info, line, Instant::now());
        self.persist_library()
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
                Stage::Measuring { .. } | Stage::Copying { .. } => false,
            };
            if !holds {
                continue;
            }
            let focus_walk = match &run.stage {
                Stage::Walking { asked, .. } => *asked == Asked::OnFocus,
                Stage::Storing { plan } => plan.asked == Asked::OnFocus,
                Stage::Measuring { .. } | Stage::Copying { .. } => false,
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
        let Stage::Storing { plan } = run.stage else {
            return Task::none();
        };
        let root = plan.folder.root.clone();
        let (copies, failure) = partition_store_results(results);
        let outcome = self.land_folder_walk(*plan, Some(copies));
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        let release = self.release_root(&root);
        Task::batch([outcome, release])
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

        let content: Element<'_, Message> = match self.route {
            Route::Library => library::view(
                self.tokens,
                &self.library,
                &self.shelf,
                &self.query,
                self.hovered_card.as_deref(),
                self.viewport.width,
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
        // A hidden bar is not in the tree at all: nothing to hit, nothing
        // to hover — the reveal band lives in the cursor subscription.
        if factor > 0.0 {
            layers.push(self.bar(factor));
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
    fn bar(&self, factor: f32) -> Element<'_, Message> {
        let (left, center, right) = match self.route {
            Route::Library => {
                let book_count = book::book_rows(&self.library.books).count();
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
                    Some(bar::breadcrumb(
                        self.tokens,
                        &self.library,
                        &self.shelf,
                        factor,
                        self.renaming,
                        &self.rename_draft,
                    )),
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

    /// The open panel, clamped into the viewport at the pointer's last
    /// position.
    fn menu_layer(&self) -> Option<Element<'_, Message>> {
        let kind = self.menu?;
        let (panel, size) = match kind {
            MenuKind::Add => menus::add_menu(self.tokens),
            MenuKind::View => menus::view_menu(self.tokens, &self.library.view),
            MenuKind::Shelf => menus::shelf_menu(self.tokens),
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
                menus::folder_menu(self.tokens, shelf)
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
            Sheet::Import { root } => sheet::panel_sized(
                self.tokens,
                sheet::IMPORT_W,
                "Import books",
                import_sheet(self.tokens, root, &self.import_opts),
                vec![
                    sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel),
                    sheet::confirm_button(self.tokens, "Import", Message::SheetSave, false),
                ],
            ),
        };
        Some(sheet::overlay(panel, Message::SheetCancel))
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
        if self.titlebar.needs_tick(now) || self.toasts.needs_tick(now) {
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
fn on_event(event: iced::Event, _status: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(event) => Some(Message::WindowEvent(id, event)),
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::Cursor(Some(position)))
        }
        iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::Cursor(None)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::EscapePressed),
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
            Stage::Copying { .. } => format!("Copying “{}”…", run.label),
            Stage::Walking { .. } | Stage::Storing { .. } => {
                format!("Scanning “{}”…", run.label)
            }
        },
    }
}

/// The import sheet's body: the folder, the formats, the size threshold,
/// how the books are held and the structure answer — the walk's orders,
/// written before it walks.
#[allow(clippy::too_many_lines)]
fn import_sheet(tokens: Tokens, root: &Path, opts: &FolderOpts) -> Element<'static, Message> {
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
    let books_note = text(mode_note(mode)).size(12).color(tokens.muted);

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
    let structure_note = text(
        "A shelf for each folder gives every subfolder its own shelf; one shelf keeps the whole import together.",
    )
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
/// mode, so a mode described two ways never reads as two modes.
fn mode_note(mode: FolderMode) -> &'static str {
    match mode {
        FolderMode::Copy => {
            "Books are copied into the app's own files, so they keep working even if the folder moves or is deleted."
        }
        FolderMode::LinkInPlace => {
            "Books stay where they are — the library just remembers where they live. Books added to the folder later are not picked up."
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
