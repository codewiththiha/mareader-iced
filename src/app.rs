//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine, the theme the tokens resolved to, the two
//! persisted blobs, the toast slot and the filesystem run in flight. Later
//! phases hang the import dock, the shelf and the reading surfaces off the
//! same tree — the shape the web app's `AppState` had, in Elm order: every
//! event is a message, every message returns at most a task, and the view
//! is a pure read of the state.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::time::Instant;
use iced::widget::{button, column, container, row, stack, text};
use iced::{event, mouse, window, Alignment, Element, Length, Padding, Point, Subscription, Task, Theme};

use library_core::blob::LibraryBlob;
use library_core::folder::FolderOpts;
use library_core::wire::ImportProgress;
use reader_core::appearance::BaseMode;
use reader_core::settings::Settings;

use crate::chrome::icons::{icon, IconName};
use crate::chrome::platform::{self, Os};
use crate::chrome::titlebar::{self, Titlebar};
use crate::platform::{dialogs, fs, progress};
use crate::route::Route;
use crate::storage;
use crate::theme::{self, Tokens};
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
    /// The persisted shelf. Read-only until the library's flows land; the
    /// count it carries is the proof persistence works end to end.
    library: LibraryBlob,
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
    /// A window lifecycle event: opened, rescaled, a file dropped.
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
    /// The shelf asked for the document picker.
    PickDocument,
    /// The picker answered: a path, or nothing when dismissed.
    FilePicked(Option<PathBuf>),
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
        let state = Self {
            window: None,
            maximized: false,
            route: Route::Library,
            titlebar: Titlebar::new(),
            theme: theme::build(tokens, settings.appearance.base),
            tokens,
            library: storage::load_library(),
            settings,
            toasts: ToastHost::default(),
            open_document: None,
            scan: None,
            next_task: 0,
        };
        (state, window::oldest().map(Message::WindowDiscovered))
    }

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
            Message::PickDocument => dialogs::pick_document(Message::FilePicked),
            Message::FilePicked(picked) => match picked {
                Some(path) => self.open_document_path(path),
                None => Task::none(),
            },
            Message::PickFolder => dialogs::pick_folder(Message::FolderPicked),
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
            // A drop anywhere in the window is an open request — the same
            // door the picker uses, with the same gate in front of it.
            window::Event::FileDropped(path) => self.open_document_path(path),
            // CloseRequested will become the persistence flush's cue once
            // there is anything left to flush that change-time saves miss.
            _ => Task::none(),
        }
    }

    /// Feed the window's live scale factor to the device-pixel grid the
    /// page geometry snaps to — at open and on every rescale.
    fn set_scale_factor(&mut self, factor: f32) {
        pdf_core::pixel_grid::set_device_pixel_ratio(f64::from(factor));
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

    fn view(&self) -> Element<'_, Message> {
        let now = Instant::now();
        let factor = self.titlebar.factor(now);

        let content = match self.route {
            Route::Library => {
                shelf(self.tokens, &self.library, self.settings.appearance.base, self.scan.as_ref())
            }
            Route::Reader => reader_surface(self.tokens, self.open_document.as_deref()),
        };

        let mut layers: Vec<Element<'_, Message>> = vec![content];
        // A hidden bar is not in the tree at all: nothing to hit, nothing
        // to hover — the reveal band lives in the cursor subscription.
        if factor > 0.0 {
            let bar = titlebar::view(
                &self.titlebar,
                titlebar::ViewContext {
                    tokens: self.tokens,
                    route: self.route,
                    maximized: self.maximized,
                    factor,
                    title: self.route.title(),
                },
            )
            .map(Message::Chrome);
            layers.push(bar);
        }
        if let Some(toast) = self.toasts.view(self.tokens) {
            layers.push(toast);
        }

        stack(layers).width(Length::Fill).height(Length::Fill).into()
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
        _ => None,
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

/// The shelf until the library lands: the app's mark, the honest count of
/// what the persisted blob holds, the open buttons that exercise the
/// platform layer end to end, and the walk's live line.
fn shelf<'a>(
    tokens: Tokens,
    library: &LibraryBlob,
    base: BaseMode,
    scan: Option<&'a ScanRun>,
) -> Element<'a, Message> {
    let status = match library.books.len() {
        0 => "The shelf is empty — the library's import flows land next.".to_owned(),
        1 => "1 book on the shelf.".to_owned(),
        n => format!("{n} books on the shelf."),
    };

    let (glyph, label) = match base {
        BaseMode::Light => (IconName::Sun, "Light"),
        BaseMode::Dark => (IconName::Moon, "Dark"),
        BaseMode::Dim => (IconName::Dim, "Dim"),
    };

    let mut content = column![
        icon(IconName::Library, 44, tokens.accent),
        text("Mareader").size(26).color(tokens.ink),
        text(status).size(13).color(tokens.muted),
        row![
            pill(tokens, text("Open Document…").size(13).color(tokens.ink).into(), Message::PickDocument, true),
            pill(tokens, text("Open Folder…").size(13).color(tokens.ink).into(), Message::PickFolder, scan.is_none()),
            pill(
                tokens,
                row![icon(glyph, 14, tokens.ink), text(label).size(13).color(tokens.ink)]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into(),
                Message::CycleAppearance,
                true,
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .align_x(Alignment::Center)
    .spacing(14)
    .width(Length::Shrink);

    if let Some(run) = scan {
        let line = match &run.latest {
            Some(beat) if beat.total > 0 => {
                format!("Scanning “{}”: {} of {} found", run.root, beat.done, beat.total)
            }
            _ => format!("Scanning “{}”…", run.root),
        };
        content = content.push(text(line).size(12).color(tokens.muted));
    }

    centered(content)
}

/// The reader route until the engines land: the document's name, the fact
/// that it is remembered, and the way back.
fn reader_surface(tokens: Tokens, document: Option<&Path>) -> Element<'static, Message> {
    let name = document
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the document".to_owned());
    centered(
        column![
            icon(IconName::Type, 40, tokens.accent),
            text(name).size(18).color(tokens.ink),
            text("Selected, remembered, and safe — the reading surfaces land with the engines.")
                .size(13)
                .color(tokens.muted),
            pill(
                tokens,
                text("Back to the shelf").size(13).color(tokens.ink).into(),
                Message::BackToShelf,
                true,
            ),
        ]
        .align_x(Alignment::Center)
        .spacing(12)
        .width(Length::Shrink),
    )
}

/// A quiet text button: the chrome's ghost style at full presence — the
/// same hover wash the titlebar's pin wears.
fn pill<'a>(
    tokens: Tokens,
    label: Element<'a, Message>,
    message: Message,
    enabled: bool,
) -> Element<'a, Message> {
    let action = button(label)
        .padding(Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 14.0 })
        .style(move |_, status| titlebar::ghost_button_style(tokens, 1.0, status));
    (if enabled { action.on_press(message) } else { action }).into()
}

/// Both placeholder surfaces' frame: dead center in the window.
fn centered<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
