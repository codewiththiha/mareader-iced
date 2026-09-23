//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine, the theme the tokens resolved to, the two
//! persisted blobs, the shelf's navigation facts (the level it stands on,
//! the query narrowing it, the menu it holds open), the toast slot and the
//! filesystem run in flight. Every event is a message, every message
//! returns at most a task, and the view is a pure read of the state — the
//! Elm order the web app's `AppState` kept.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::time::Instant;
use iced::widget::{button, column, container, mouse_area, stack, text, Space};
use iced::{
    event, keyboard, mouse, window, Alignment, Element, Length, Padding, Point, Size,
    Subscription, Task, Theme,
};

use library_core::blob::LibraryBlob;
use library_core::book::{self, Book, Origin};
use library_core::folder::FolderOpts;
use library_core::shelf::{self, ALL_SHELF};
use library_core::sort::SortKey;
use library_core::text as lib_text;
use library_core::view::{CoverFit, LibraryLayout};
use library_core::wire::ImportProgress;
use reader_core::appearance::BaseMode;
use reader_core::settings::Settings;

use crate::chrome::icons::{icon, IconName};
use crate::chrome::platform::{self, Os};
use crate::chrome::titlebar::{self, Titlebar};
use crate::library::{self, bar, menus};
use crate::platform::{dialogs, fs, progress};
use crate::route::Route;
use crate::storage;
use crate::theme::{self, fade, Tokens};
use crate::ui::menu as popover;
use crate::ui::toast::{ToastHost, Tone};

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
    /// A folder walk finished: the run's id, and the document count or the
    /// advice the walk answers with.
    ScanDone(u64, Result<usize, String>),
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
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                Task::none()
            }
            Message::EscapePressed => {
                self.menu = None;
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
                Some(dir) => self.start_scan(dir),
                None => Task::none(),
            },
            Message::ScanDone(task, result) => {
                let finished = if self.scan.as_ref().is_some_and(|run| run.task == task) {
                    self.scan.take()
                } else {
                    None
                };
                match result {
                    // A stale success — a run replaced mid-walk — stays
                    // silent; stale advice is still worth surfacing.
                    Ok(found) => {
                        if let Some(run) = finished {
                            self.toasts.show(
                                Tone::Info,
                                format!("Found {found} documents in “{}”", run.root),
                                now,
                            );
                        }
                    }
                    Err(error) => self.toasts.show(Tone::Error, error, now),
                }
                Task::none()
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
        let now = now_ms();
        let id = library_core::id::next_shelf_id(now);
        let in_use: HashSet<String> =
            self.library.shelves.iter().map(|shelf| shelf.name.clone()).collect();
        let name = book::duplicate_title("New shelf", &in_use);
        let parent = (self.shelf != ALL_SHELF).then(|| self.shelf.clone());
        self.library
            .shelves
            .push(shelf::Shelf::virtual_shelf(id.clone(), name, parent));
        self.menu = None;
        self.shelf = id;
        self.persist_library()
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
    /// that lives exactly as long as the run.
    fn start_scan(&mut self, dir: PathBuf) -> Task<Message> {
        let root = dir.to_string_lossy().into_owned();
        let display = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone());
        let (sink, rx) = progress::channel();
        self.next_task += 1;
        let task = self.next_task;
        self.scan = Some(ScanRun { task, root: display, rx, latest: None });

        let task_name = task.to_string();
        Task::perform(
            async move {
                let opts = FolderOpts::default();
                fs::scan(&task_name, &root, &opts, &sink).map(|found| found.len())
            },
            move |result| Message::ScanDone(task, result),
        )
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
        // A hidden bar is not in the tree at all: nothing to hit, nothing
        // to hover — the reveal band lives in the cursor subscription.
        if factor > 0.0 {
            layers.push(self.bar(factor));
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
                    Some(bar::breadcrumb(self.tokens, &self.library, &self.shelf, factor)),
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
