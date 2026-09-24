//! The session around the library: the window's events, the persisted blobs,
//! the appearance and the shell's title.
use std::collections::HashSet;
use std::sync::Arc;

use iced::time::Instant;
use iced::{event, window, Point, Size, Subscription, Task, Theme};
use library_core::folder::FolderOpts;
use library_core::shelf::ALL_SHELF;

use crate::chrome::titlebar::Titlebar;
use crate::library::{self};
use crate::platform::{self, progress, Os};
use crate::route::Route;
use crate::theme::{self, Tokens};
use crate::ui::toast::{ToastHost, Tone};
use crate::{reader, storage};
use super::events::on_event;
use super::message::Message;
use super::{APP_TITLE, Mareader};

/// The window the app opens in: the original's 1200×800 with its 640×480
/// floor, frameless on Windows and Linux, and on macOS a native frame whose
/// titlebar is transparent and full-size-content — AppKit's own traffic
/// lights floating over the app's bar, exactly as the Tauri shell had it.
pub(super) fn window_settings() -> window::Settings {
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

impl Mareader {
    pub(super) fn boot() -> (Self, Task<Message>) {
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

    pub(super) fn window_event(&mut self, id: window::Id, event: window::Event) -> Task<Message> {
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
    pub(super) fn set_scale_factor(&mut self, factor: f32) {
        pdf_core::pixel_grid::set_device_pixel_ratio(f64::from(factor));
        // The reading surface re-rasterises on the new grid: a page drawn for a
        // 1× screen must not be stretched by the compositor on a 1.5× panel.
        let effects = self
            .reader
            .update(reader::Message::Scale(f64::from(factor)), Instant::now());
        let _ = self.apply_reader_effects(effects);
    }

    /// A view fact changed: close the menu that changed it and persist the
    /// library.
    pub(super) fn view_changed(&mut self) -> Task<Message> {
        self.menu = None;
        self.persist_library()
    }

    /// Mint a shelf at the level on screen and step into it — the web app's
    /// create-and-enter, one message.
    /// Stand on another level: the panels close, the rename and the choice
    /// in flight belong to the level they started on, and the hover truth
    /// resets — the pointer is over new ground now.
    pub(super) fn navigate_to(&mut self, shelf: String) {
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

    /// Tell the view what auto-fit measured, and persist it when it moved —
    /// the stepper's `+` starts from what the shelf shows.
    fn report_auto_fit(&mut self) {
        if library::report_fit(&mut self.library.view, self.viewport.width) {
            let _ = self.persist_library();
        }
    }

    pub(super) fn apply_appearance(&mut self) {
        let base = self.settings.appearance.base;
        self.tokens = Tokens::for_base(base);
        self.theme = theme::build(self.tokens, base);
    }

    /// Settings save at the moment of change, atomically; a failure is the
    /// toast slot's business, and the in-memory copy stays authoritative.
    pub(super) fn persist_settings(&mut self) -> Task<Message> {
        if let Err(error) = storage::save_settings(&self.settings) {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        Task::none()
    }

    /// The library's own save contract: the same moment-of-change rule.
    pub(super) fn persist_library(&mut self) -> Task<Message> {
        if let Err(error) = storage::save_library(&self.library) {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        Task::none()
    }

    /// The window's own title: what iced hands the OS. Owned because the
    /// platform wants a value, which is the only difference from the bar's
    /// centre.
    pub(super) fn title(&self) -> String {
        self.shown_title().to_owned()
    }

    /// The name the window and the bar's centre both show.
    ///
    /// The reader route hands it to the document — the web app's floating
    /// label — so both say what is being read rather than what the app is. A
    /// stored copy's name comes from its row, which is why this asks the
    /// reader rather than the file name. Borrowed, because the bar's element
    /// outlives the frame that built it.
    pub(super) fn shown_title(&self) -> &str {
        match self.route {
            Route::Reader if self.reader.document.is_open() => self.reader.name(),
            _ => APP_TITLE,
        }
    }

    pub(super) fn theme(&self) -> Theme {
        self.theme.clone()
    }

    pub(super) fn style(&self, _theme: &Theme) -> iced::theme::Style {
        theme::application_style(self.tokens)
    }

    pub(super) fn subscription(&self) -> Subscription<Message> {
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
