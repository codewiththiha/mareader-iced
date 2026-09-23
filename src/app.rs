//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine and the theme the tokens resolved to. Later
//! phases hang the settings, the library and the reader off the same tree —
//! the shape the web app's `AppState` had, in Elm order: every event is a
//! message, every message returns at most a task, and the view is a pure
//! read of the state.

use iced::time::Instant;
use iced::widget::{column, container, stack, text};
use iced::{event, mouse, system, window, Element, Length, Point, Subscription, Task, Theme};

use crate::chrome::icons::{icon, IconName};
use crate::chrome::platform::{self, Os};
use crate::chrome::titlebar::{self, Titlebar};
use crate::route::Route;
use crate::theme::{self, Tokens};

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
    /// the reader drives this exactly as the web app's URL did: a document
    /// Ready is the reader, nothing open is the shelf.
    route: Route,
    /// The titlebar's hover machine.
    titlebar: Titlebar,
    /// The tokens of the current base, and the iced theme built from them.
    tokens: Tokens,
    theme: Theme,
}

/// Everything the application can be told.
#[derive(Debug, Clone)]
pub enum Message {
    /// The main window's identity, from the boot query.
    WindowDiscovered(Option<window::Id>),
    /// A window lifecycle event: opened, resized, rescaled, focus, a file
    /// dropped (the open pipeline that lands with the library).
    WindowEvent(window::Id, window::Event),
    /// The window answered a maximize query.
    Maximized(bool),
    /// The window answered a scale-factor query.
    ScaleFactor(f32),
    /// The pointer moved (window coordinates), or left the window.
    Cursor(Option<Point>),
    /// One animation frame, subscribed to only while the chrome animates.
    Tick(Instant),
    /// The titlebar's own business.
    Chrome(titlebar::Message),
    /// The system's light/dark answer, at boot and on change.
    SystemTheme(iced::theme::Mode),
}

impl Mareader {
    fn boot() -> (Self, Task<Message>) {
        let tokens = Tokens::for_mode(false);
        let state = Self {
            window: None,
            maximized: false,
            route: Route::Library,
            titlebar: Titlebar::new(),
            tokens,
            theme: theme::build(tokens, false),
        };
        (
            state,
            Task::batch([
                window::oldest().map(Message::WindowDiscovered),
                system::theme().map(Message::SystemTheme),
            ]),
        )
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
            Message::SystemTheme(mode) => {
                self.apply_mode(mode);
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
            }
            window::Event::Rescaled(factor) => self.set_scale_factor(factor),
            // CloseRequested will become the persistence flush's cue once
            // there is anything to flush; the window may close afterwards.
            _ => {}
        }
        Task::none()
    }

    /// Feed the window's live scale factor to the device-pixel grid the
    /// page geometry snaps to — at open and on every rescale.
    fn set_scale_factor(&mut self, factor: f32) {
        pdf_core::pixel_grid::set_device_pixel_ratio(f64::from(factor));
    }

    fn apply_mode(&mut self, mode: iced::theme::Mode) {
        let dark = matches!(mode, iced::theme::Mode::Dark);
        self.tokens = Tokens::for_mode(dark);
        self.theme = theme::build(self.tokens, dark);
    }

    fn view(&self) -> Element<'_, Message> {
        let now = Instant::now();
        let factor = self.titlebar.factor(now);

        let content = shelf_placeholder(self.tokens);

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

        stack(layers).width(Length::Fill).height(Length::Fill).into()
    }

    fn title(&self) -> String {
        self.route.title().to_owned()
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn style(&self, _theme: &Theme) -> iced::theme::Style {
        theme::application_style(self.tokens)
    }

    fn subscription(&self) -> Subscription<Message> {
        let now = Instant::now();
        let mut subscriptions = vec![
            event::listen_with(on_event),
            system::theme_changes().map(Message::SystemTheme),
        ];
        // Frames flow only while the chrome has something in motion: the
        // reveal animating or a hide waiting out its grace. An idle window
        // subscribes to nothing and costs no redraws.
        if self.titlebar.needs_tick(now) {
            subscriptions.push(window::frames().map(Message::Tick));
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
        decorations: platform::os() != Os::MacOs,
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

/// What the shelf shows until the library lands: the app's mark, its name,
/// and the honest state of the port.
fn shelf_placeholder(tokens: Tokens) -> Element<'static, Message> {
    container(
        column![
            icon(IconName::Library, 44, tokens.accent),
            text("Mareader").size(26).color(tokens.ink),
            text("The native foundation — the shelf arrives next.")
                .size(13)
                .color(tokens.muted),
        ]
        .align_x(iced::Alignment::Center)
        .spacing(10)
        .width(Length::Shrink),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}
