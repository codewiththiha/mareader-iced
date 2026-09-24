//! The app-global toast: one slot, centered near the bottom of the window,
//! auto-dismissed — the web chrome's toast host, natively.
//!
//! One current toast rather than a stack, exactly as the primitive it
//! replaces: a newer toast replaces an older one, and the expiry stamp is
//! re-struck on every `show`, so a stale timer can never wipe a newer
//! toast — the property the web host needed a monotonic id for falls out
//! of the single slot.
//!
//! Tones land with their producers, the same policy the web chrome kept:
//! `Error` is the app-global failure toast (a refused file, a failed
//! save), `Info` is the neutral confirmation (a scan's count). The gloss
//! `Undo` tone arrives with the gloss.

use iced::time::{Duration, Instant};
use iced::widget::{container, row, text};
use iced::alignment::Vertical;
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

use crate::chrome::icons::{icon, IconName};
use crate::theme::{Elevation, Tokens};

/// How long a toast stays up — the web primitive's 3500ms.
const TOAST_MS: u64 = 3500;

/// The anchor's lift off the bottom edge — the web host's `bottom: 4rem`.
const BOTTOM_INSET: f32 = 64.0;

/// The anchor's side padding — long messages never touch an edge.
const SIDE_INSET: f32 = 16.0;

/// The panel's ceiling — the web panel's `max-w-[min(90vw,32rem)]`, the
/// 32rem half of it; the 90vw half is the anchor's side padding.
const MAX_WIDTH: f32 = 512.0;

/// Visual tone of a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// The app-global failure toast: red-950 at 95%, a red-400 hairline at
    /// half, red-100 text — the web tone classes, resolved to colours.
    Error,
    /// A neutral confirmation in the theme's own surface.
    Info,
}

#[derive(Debug)]
struct Toast {
    tone: Tone,
    message: String,
    expires: Instant,
}

/// The single slot and nothing else: the current toast, if any.
#[derive(Debug, Default)]
pub struct ToastHost {
    current: Option<Toast>,
}

impl ToastHost {
    /// Replace the slot: the new toast wins and its expiry is struck now.
    pub fn show(&mut self, tone: Tone, message: impl Into<String>, now: Instant) {
        self.current = Some(Toast {
            tone,
            message: message.into(),
            expires: now + Duration::from_millis(TOAST_MS),
        });
    }

    /// One animation frame: drop the toast whose stamp has passed. A newer
    /// toast carries a later stamp, so a tick meant for an older one can
    /// never clear it.
    pub fn on_tick(&mut self, now: Instant) {
        if self.current.as_ref().is_some_and(|t| now >= t.expires) {
            self.current = None;
        }
    }

    /// Whether the application must keep a frame subscription alive for
    /// the expiry to land.
    pub fn needs_tick(&self, now: Instant) -> bool {
        self.current.as_ref().is_some_and(|t| now < t.expires)
    }

    /// The anchored panel, or nothing when the slot is empty. The wrapper
    /// is full-window but carries no interaction, so the mouse reaches
    /// whatever it covers.
    pub fn view<'a, Message: 'a>(&'a self, tokens: Tokens) -> Option<Element<'a, Message>> {
        let toast = self.current.as_ref()?;
        let (background, line, ink, glyph) = match toast.tone {
            Tone::Error => (
                Color::from_rgba(0.271, 0.039, 0.039, 0.95), // red-950 at 95%
                Color::from_rgba(0.973, 0.443, 0.443, 0.5),  // red-400 at 50%
                Color::from_rgb(0.996, 0.886, 0.886),        // red-100
                IconName::Close,
            ),
            Tone::Info => (
                Color { a: 0.97, ..tokens.surface },
                tokens.line,
                tokens.ink,
                IconName::Check,
            ),
        };

        let panel = container(
            row![icon(glyph, 16, ink), text(toast.message.as_str()).size(14).color(ink)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding(Padding { top: 10.0, right: 16.0, bottom: 10.0, left: 16.0 })
        .max_width(MAX_WIDTH)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            border: Border { color: line, width: 1.0, radius: 12.0.into() },
            shadow: Elevation::Toast.shadow(),
            text_color: Some(ink),
            ..container::Style::default()
        });

        Some(
            container(panel)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(Padding {
                    top: 0.0,
                    right: SIDE_INSET,
                    bottom: BOTTOM_INSET,
                    left: SIDE_INSET,
                })
                .center_x(Length::Fill)
                .align_y(Vertical::Bottom)
                .into(),
        )
    }
}
