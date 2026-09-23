//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine, the theme the tokens resolved to, the two
//! persisted blobs, the shelf's navigation facts (the level it stands on,
//! the query narrowing it, the menu it holds open), the toast slot and the
//! filesystem run in flight. Every event is a message, every message
//! returns at most a task, and the view is a pure read of the state — the
//! Elm order the web app's `AppState` kept.

use std::collections::{BTreeSet, HashMap, HashSet};
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
use library_core::book::{self, Book, Origin};
use library_core::folder::{FolderOpts, MIN_SIZE_CEIL, MIN_SIZE_FLOOR};
use library_core::ledger;
use library_core::scan::{selectable_formats, subfolder_of, FoundFile};
use library_core::shelf::{self, ALL_SHELF};
use library_core::sort::SortKey;
use library_core::text as lib_text;
use library_core::view::{CoverFit, LibraryLayout};
use library_core::wire::ImportProgress;
use reader_core::format::{format_from_ext, Format};
use reader_core::appearance::BaseMode;
use reader_core::settings::Settings;

use crate::chrome::icons::{icon, IconName};
use crate::chrome::platform::{self, Os};
use crate::chrome::titlebar::{self, Titlebar};
use crate::library::{self, bar, menus};
use crate::platform::{dialogs, fs, progress};
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
    /// walk admits.
    Import { root: PathBuf },
}

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
    /// The persisted shelf: rows, shelves, folders, and the view that
    /// paints them. Loaded at boot, saved at the moment of every change.
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
    /// The folder scan in flight, if any.
    scan: Option<ScanRun>,
    /// The id the next filesystem run wears. Progress beats carry it, so
    /// two runs' answers can never mix.
    next_task: u64,
}

/// One filesystem run and its channel: the id the beats carry, the folder's
/// display name, the parked receiver the subscription takes on first build,
/// and the latest beat — the status line's whole input.
struct ScanRun {
    task: u64,
    root: String,
    /// The structure answer the import sheet gave: one shelf per subfolder,
    /// or the whole tree on one shelf.
    groups: bool,
    rx: progress::SharedProgress,
    latest: Option<ImportProgress>,
}

/// Everything the application can be told.
#[derive(Debug, Clone)]
pub enum Message {
    /// The main window's identity, from the boot query.
    WindowDiscovered(Option<window::Id>),
    /// A window lifecycle event: opened, rescaled, resized, a file dropped.
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
    /// The import sheet: one shelf per subfolder, or one shelf for the
    /// whole tree.
    SheetImportGroups,
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
    /// One progress beat from the run in flight.
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
            scan: None,
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
                // grid snaps against it from the first frame.
                match id {
                    Some(id) => window::scale_factor(id).map(Message::ScaleFactor),
                    None => Task::none(),
                }
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
            Message::SheetImportGroups => {
                self.import_opts.groups = !self.import_opts.groups;
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
            Message::ScanDone(task, result) => {
                let finished = if self.scan.as_ref().is_some_and(|run| run.task == task) {
                    self.scan.take()
                } else {
                    None
                };
                match result {
                    // A stale answer — a run replaced mid-walk — stays
                    // silent; stale advice is still worth surfacing.
                    Ok(found) => match finished {
                        Some(run) => self.import_folder(run.root, found, run.groups),
                        None => Task::none(),
                    },
                    Err(error) => {
                        self.toasts.show(Tone::Error, error, now);
                        Task::none()
                    }
                }
            }
            Message::ImportProgress(beat) => {
                if let Some(run) = &mut self.scan
                    && run.task.to_string() == beat.task
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
            // A drop anywhere in the window is an open request — the same
            // door the picker uses, with the same gate in front of it.
            window::Event::FileDropped(path) => self.open_document_path(path),
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
    /// also name them), its children re-hang on its parent, and it goes.
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
            Sheet::Remove { id, name } => {
                let before = self.library.books.len();
                self.library.books.retain(|row| match row {
                    book::Row::Book(book) => book.id != id,
                    book::Row::Link { id: link_id, .. } => link_id != &id,
                });
                if self.library.books.len() != before {
                    shelf::forget_everywhere(&mut self.library.shelves, &id);
                    self.toasts.show(
                        Tone::Info,
                        format!("Removed “{name}” from the library"),
                        Instant::now(),
                    );
                }
                self.persist_library()
            }
            Sheet::Import { root } => {
                let opts = self.import_opts.clone();
                self.start_scan(root, opts)
            }
        }
    }

    /// The pickers' answer: measure each file, mint its row, and — when the
    /// shelf stands on a shelf — file it there. The fingerprint dedupes: a
    /// file the library already holds is placed, not copied in again.
    fn import_files(&mut self, picked: Option<Vec<PathBuf>>) -> Task<Message> {
        self.menu = None;
        let Some(paths) = picked else { return Task::none() };
        if paths.is_empty() {
            return Task::none();
        }

        let now = now_ms();
        let on_shelf = self.shelf != ALL_SHELF;
        let mut added = 0usize;
        let mut already = 0usize;
        let mut failures: Vec<String> = Vec::new();

        for path in paths {
            let address = path.to_string_lossy().into_owned();
            if let Err(error) = fs::ensure_readable_document(&address) {
                failures.push(error);
                continue;
            }
            match fs::measure_document(&address) {
                Ok((fingerprint, format)) => {
                    let existed = self.library.books.iter().any(|row| {
                        row.book().is_some_and(|book| book.fp == fingerprint)
                    });
                    let row_id = {
                        let minted = Book::new(
                            library_core::id::next_id(now),
                            fingerprint,
                            format,
                            Origin::Linked { src: address },
                            now,
                        );
                        book::add_book(&mut self.library.books, minted)
                    };
                    if on_shelf
                        && let Some(current) =
                            self.library.shelves.iter_mut().find(|s| s.id == self.shelf)
                    {
                        shelf::shelf_add(current, &row_id);
                    }
                    if existed {
                        already += 1;
                    } else {
                        added += 1;
                    }
                }
                Err(error) => failures.push(error),
            }
        }

        let mut outcome = Task::none();
        if added > 0 {
            let line = if on_shelf {
                format!("Added {} to this shelf", lib_text::plural(added, "book", "books"))
            } else {
                format!("Added {}", lib_text::plural(added, "book", "books"))
            };
            self.toasts.show(Tone::Info, line, Instant::now());
            outcome = self.persist_library();
        } else if already > 0 {
            self.toasts.show(Tone::Info, "Already in the library", Instant::now());
            if on_shelf {
                outcome = self.persist_library();
            }
        }
        if !failures.is_empty() && added == 0 {
            self.toasts.show(Tone::Error, failures.swap_remove(0), Instant::now());
        }
        outcome
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

    /// Kick off a folder walk: one run id, one channel, one subscription
    /// that lives exactly as long as the run. The options come from the
    /// import sheet — what the walk admits is the sheet's answer.
    fn start_scan(&mut self, dir: PathBuf, opts: FolderOpts) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        let display = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.scan = Some(ScanRun { task, root: display, groups: opts.groups, rx, latest: None });

        let task_name = task.to_string();
        Task::perform(
            async move { fs::scan(&task_name, &root, &opts, &sink) },
            move |result| Message::ScanDone(task, result),
        )
    }

    /// The walk's answer, filed: the folder becomes a shelf at the level
    /// the reader stands on, each subfolder a shelf beneath it, and every
    /// document the walk measured becomes a linked book on the shelf of its
    /// directory. The ledger decides what is new: content the library
    /// already holds is counted, not copied in again.
    fn import_folder(
        &mut self,
        root_name: String,
        found: Vec<FoundFile>,
        groups: bool,
    ) -> Task<Message> {
        if found.is_empty() {
            self.toasts.show(
                Tone::Info,
                format!("No documents found in “{root_name}”"),
                Instant::now(),
            );
            return Task::none();
        }

        let now = now_ms();
        let known = ledger::registry_of(&self.library.books);
        let mut names: HashSet<String> =
            self.library.shelves.iter().map(|s| s.name.clone()).collect();

        // The folder's own shelf, minted at the level on screen.
        let folder_shelf = library_core::id::next_shelf_id(now);
        let folder_name = book::duplicate_title(&root_name, &names);
        names.insert(folder_name.clone());
        let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        self.library.shelves.push(shelf::Shelf::virtual_shelf(
            folder_shelf.clone(),
            folder_name,
            parent,
        ));

        // One shelf per directory, nested under its parent directory's
        // shelf — when the sheet's structure answer asks for shelves per
        // subfolder. Every ancestor of a found file counts as a directory —
        // a file three levels down mints all three rungs — and the set's
        // order walks parents before their children.
        let mut dir_shelves: HashMap<String, String> = HashMap::new();
        if groups {
            let mut dirs: BTreeSet<String> = BTreeSet::new();
            for file in &found {
                let mut dir = subfolder_of(&file.rel);
                while !dir.is_empty() {
                    dirs.insert(dir.to_string());
                    dir = dir.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
                }
            }
            for dir in dirs {
                let (parent_id, segment) = match dir.rsplit_once('/') {
                    Some((parent_dir, leaf)) => (
                        dir_shelves.get(parent_dir).cloned().unwrap_or_else(|| folder_shelf.clone()),
                        leaf.to_string(),
                    ),
                    None => (folder_shelf.clone(), dir.clone()),
                };
                let name = book::duplicate_title(&segment, &names);
                names.insert(name.clone());
                let id = library_core::id::next_shelf_id(now);
                dir_shelves.insert(dir.clone(), id.clone());
                self.library
                    .shelves
                    .push(shelf::Shelf::virtual_shelf(id, name, Some(parent_id)));
            }
        }

        // Place the books: new content is minted and filed; content the
        // library already holds is left where the reader filed it.
        let mut imported = 0usize;
        let mut already = 0usize;
        for file in found {
            if known.contains_key(&file.fp) {
                already += 1;
                continue;
            }
            let Some(format) = format_from_ext(&file.ext) else {
                continue;
            };
            let row_id = {
                let minted = Book::new(
                    library_core::id::next_id(now),
                    file.fp,
                    format,
                    Origin::Linked { src: file.path },
                    now,
                );
                book::add_book(&mut self.library.books, minted)
            };
            let target = match subfolder_of(&file.rel) {
                "" => folder_shelf.clone(),
                dir => dir_shelves.get(dir).cloned().unwrap_or_else(|| folder_shelf.clone()),
            };
            if let Some(home) = self.library.shelves.iter_mut().find(|s| s.id == target) {
                shelf::shelf_add(home, &row_id);
            }
            imported += 1;
        }

        if imported > 0 {
            self.toasts.show(
                Tone::Info,
                format!(
                    "Imported {} from “{}”",
                    lib_text::plural(imported, "book", "books"),
                    root_name
                ),
                Instant::now(),
            );
        } else if already > 0 {
            self.toasts.show(
                Tone::Info,
                format!("Everything in “{root_name}” is already in the library"),
                Instant::now(),
            );
        }
        self.persist_library()
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
        // The walk's live line, while one is in flight.
        if self.route == Route::Library
            && let Some(run) = &self.scan
        {
            layers.push(scan_dock(self.tokens, run));
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
                    "Remove “{name}” from the library? The file on disk stays where it is."
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
        // The run's beats flow while the run lives: same id, same
        // subscription; gone from the tree, closed by the tracker.
        if let Some(run) = &self.scan {
            subscriptions.push(
                progress::subscription(run.task, Arc::clone(&run.rx))
                    .map(Message::ImportProgress),
            );
        }
        Subscription::batch(subscriptions)
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

/// The folder walk's live line: a pill at the foot of the shelf naming the
/// folder and the count so far. A decoration, not a control — the walk is
/// not interruptible, so the pill takes no presses and claims no pointer.
fn scan_dock(tokens: Tokens, run: &ScanRun) -> Element<'static, Message> {
    let line = match &run.latest {
        Some(beat) if beat.total > 0 => {
            format!("Scanning “{}”: {} of {} found", run.root, beat.done, beat.total)
        }
        Some(beat) if beat.done > 0 => {
            format!("Scanning “{}”: {} found", run.root, beat.done)
        }
        _ => format!("Scanning “{}”…", run.root),
    };
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
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(Padding { top: 0.0, right: 0.0, bottom: 20.0, left: 0.0 })
    .align_x(Alignment::Center)
    .align_y(Alignment::End)
    .into()
}

/// The import sheet's body: the folder, the formats, the size threshold and
/// the structure answer — the walk's orders, written before it walks.
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

    // The structure answer, one wide chip.
    let groups_row = chip(
        tokens,
        if opts.groups { "One shelf per subfolder" } else { "One shelf for the whole folder" },
        true,
        Some(Message::SheetImportGroups),
    );

    Column::new()
        .push(section("FOLDER"))
        .push(folder_pill)
        .push(section("FORMATS"))
        .push(include_row)
        .push(Column::with_children(format_rows).spacing(6))
        .push(section("FILE SIZE LARGER THAN"))
        .push(size_row)
        .push(section("STRUCTURE"))
        .push(groups_row)
        .spacing(10)
        .width(Length::Fill)
        .into()
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
