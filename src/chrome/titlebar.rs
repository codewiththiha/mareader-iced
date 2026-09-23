//! The hover/grab titlebar: headless until the pointer asks for it.
//!
//! The web chrome's contract, restated natively:
//!
//!   * `visible = pinned || hovered` — one hover truth for the whole bar,
//!     where "hovered" means the pointer is inside the bar while it shows,
//!     or inside the thin reveal band along the top edge while it hides;
//!   * a hide never lands immediately: it waits out the 400ms grace, and a
//!     pointer (or a pin) that comes back inside the grace cancels it;
//!   * the pin is remembered PER ROUTE — the shelf's bar starts pinned
//!     (navigation a reader has to hover to find is navigation the shelf is
//!     hiding) and the reader's starts free;
//!   * the bar is grabbable: a press on the band the buttons do not own
//!     starts a window drag, and a double-press toggles maximize.
//!
//! The reveal is an `Animation<bool>`; the application subscribes to window
//! frames only while the animation runs or a grace is pending, so an idle
//! window costs no redraws.

use iced::animation::Animation;
use iced::time::{Duration, Instant};
use iced::widget::{button, container, mouse_area, row, stack, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Point, Shadow, Vector};

use super::captions;
use super::icons::{icon, IconName};
use super::platform::{self, Os};
use crate::route::Route;
use crate::theme::{fade, Tokens};

/// What the bar can be asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Hold the bar open (or release it) on the route it is showing for.
    TogglePin,
    /// A window command from the drag band or a caption.
    Window(WindowAction),
}

/// The window commands the chrome issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Drag,
    Minimize,
    ToggleMaximize,
    Close,
}

/// The bar's whole memory: the two pins, the hover truth, the pending hide
/// and the reveal animation.
pub struct Titlebar {
    pinned_reader: bool,
    pinned_library: bool,
    hovered: bool,
    hide_at: Option<Instant>,
    reveal: Animation<bool>,
}

impl Titlebar {
    pub fn new() -> Self {
        Self {
            // The reader's bar hides out of the document's way; the shelf's
            // bar is how you move, so it starts pinned — the same two
            // defaults the persisted settings carry.
            pinned_reader: false,
            pinned_library: true,
            hovered: false,
            hide_at: None,
            reveal: Animation::new(false).quick(),
        }
    }

    /// Whether the bar is held open on a route.
    pub fn pinned(&self, route: Route) -> bool {
        match route {
            Route::Library => self.pinned_library,
            Route::Reader => self.pinned_reader,
        }
    }

    /// Flip the pin of the route on screen, and settle: unpinning with the
    /// pointer elsewhere starts the grace immediately.
    pub fn toggle_pin(&mut self, route: Route, now: Instant) {
        let slot = match route {
            Route::Library => &mut self.pinned_library,
            Route::Reader => &mut self.pinned_reader,
        };
        *slot = !*slot;
        self.settle(route, now);
    }

    /// Whether the bar is shown or on its way to shown — the view renders
    /// nothing at all once this is false and the reveal has finished.
    pub fn shown(&self) -> bool {
        self.reveal.value()
    }

    /// The reveal's interpolated 0..1 progress; every colour the bar paints
    /// rides it.
    pub fn factor(&self, now: Instant) -> f32 {
        self.reveal.interpolate(0.0, 1.0, now)
    }

    /// Whether the application must keep a frame subscription alive: the
    /// reveal is moving, or a hide is waiting out its grace.
    pub fn needs_tick(&self, now: Instant) -> bool {
        self.reveal.is_animating(now) || self.hide_at.is_some()
    }

    /// One pointer report — every cursor move in the window and every
    /// cursor leave. The hover limit depends on whether the bar is out:
    /// a hidden bar is found in the reveal band, a shown one is kept by
    /// anywhere inside its own height.
    pub fn on_cursor(&mut self, position: Option<Point>, route: Route, now: Instant) {
        let limit = if self.shown() { platform::TITLE_BAR_H } else { platform::REVEAL_BAND };
        self.hovered = position.is_some_and(|p| (0.0..limit).contains(&p.y));
        self.settle(route, now);
    }

    /// One animation frame: re-settle, which lands a hide whose grace has
    /// run out.
    pub fn on_tick(&mut self, route: Route, now: Instant) {
        self.settle(route, now);
    }

    /// The one place the reveal is decided.
    fn settle(&mut self, route: Route, now: Instant) {
        let out = self.reveal.value() || self.reveal.is_animating(now);
        if self.pinned(route) || self.hovered {
            // Shown, or on its way to shown; any pending hide is cancelled.
            self.hide_at = None;
            if !self.reveal.value() {
                self.reveal.go_mut(true, now);
            }
        } else if out {
            match self.hide_at {
                // The pointer just left: start the grace.
                None => {
                    self.hide_at =
                        Some(now + Duration::from_millis(platform::HIDE_GRACE_MS));
                }
                // The grace ran out while ticking: hide.
                Some(at) if now >= at => {
                    self.hide_at = None;
                    self.reveal.go_mut(false, now);
                }
                // Still waiting.
                Some(_) => {}
            }
        }
    }
}

/// Everything the view needs that the bar itself does not own.
pub struct ViewContext<'a> {
    pub tokens: Tokens,
    pub route: Route,
    /// The window's maximize state, for the caption glyph swap.
    pub maximized: bool,
    /// The reveal's 0..1 progress.
    pub factor: f32,
    /// The centered title (the reader's floating document title replaces
    /// this when a book is open).
    pub title: &'a str,
}

/// The bar: a drag band with the content laid over it. The band is the
/// stack's base, so the buttons — later children, therefore on top — take
/// their own clicks and everything else lands on the drag.
pub fn view<'a>(state: &Titlebar, ctx: ViewContext<'a>) -> Element<'a, Message> {
    let ViewContext { tokens, route, maximized, factor, title } = ctx;
    let os = platform::os();

    let band = mouse_area(
        container(Space::new().width(Length::Fill).height(platform::TITLE_BAR_H))
            .width(Length::Fill)
            .style(move |_| bar_style(tokens, factor)),
    )
    .on_press(Message::Window(WindowAction::Drag))
    .on_double_click(Message::Window(WindowAction::ToggleMaximize));

    // The center slot: the title centers in the free stretch between the
    // clusters and elides. The web bar measured its clusters live and
    // preferred the row's exact center when the title fit there; the
    // measured upgrade lands with the reader's floating title, and the
    // free-stretch tier is the honest default until then.
    let pinned = state.pinned(route);
    let center = container(text(title).size(13).color(fade(tokens.ink, factor)))
        .width(Length::Fill)
        .center_x(Length::Fill);

    let left: Element<'a, Message> = match os {
        // The traffic lights are painted by AppKit over the content; the
        // bar keeps clear of them and owns no captions of its own.
        Os::MacOs => Space::new().width(platform::MACOS_LIGHTS_INSET).into(),
        _ => Space::new().width(1.0).into(),
    };

    let pin = button(icon(
        IconName::Pin,
        14,
        fade(if pinned { tokens.accent } else { tokens.muted }, factor),
    ))
    .padding(7.0)
    .style(move |_, status| ghost_button_style(tokens, factor, status))
    .on_press(Message::TogglePin);

    let right_pad = match os {
        Os::MacOs => 16.0,
        Os::Windows => 0.0,
        Os::Linux => 12.0,
    };
    let right = row![pin, captions::view(tokens, os, maximized, factor)]
        .align_y(Alignment::Center)
        .spacing(4);

    let content = row![left, center, right]
        .align_y(Alignment::Center)
        .padding(Padding { top: 0.0, right: right_pad, bottom: 0.0, left: 8.0 })
        .height(platform::TITLE_BAR_H);

    stack![band, content].width(Length::Fill).height(platform::TITLE_BAR_H).into()
}

/// The bar's plate: the paper's own colour, mostly opaque, with the
/// hairline under it and a soft shadow — the glass toolbar's native
/// approximation until the appearance system layers it.
fn bar_style(tokens: Tokens, factor: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color { a: 0.92 * factor, ..tokens.paper })),
        border: Border { color: fade(tokens.line, factor), width: 1.0, radius: 0.0.into() },
        shadow: Shadow {
            color: Color { a: 0.10 * factor, ..Color::BLACK },
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0 * factor,
        },
        ..container::Style::default()
    }
}

/// A ghost button: nothing at rest, a surface wash under the pointer.
pub(crate) fn ghost_button_style(
    tokens: Tokens,
    factor: f32,
    status: button::Status,
) -> button::Style {
    let wash = match status {
        button::Status::Hovered => Some(fade(tokens.surface, factor)),
        button::Status::Pressed => Some(fade(tokens.line, factor)),
        _ => None,
    };
    button::Style {
        background: wash.map(Background::Color),
        border: iced::Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
        text_color: fade(tokens.ink, factor),
        shadow: Shadow::default(),
        snap: false,
    }
}
