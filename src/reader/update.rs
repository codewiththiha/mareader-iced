//! The reader's half of the Elm loop: the messages it answers, the effects it
//! hands back, and the element it draws.

use std::time::Instant;

use iced::{Element, Size};
use reader_core::zoom_math::FitMode;

use crate::formats::pdf::Event;
use crate::theme::Tokens;
use crate::ui::toast::Tone;
use super::page;
use super::zoom::Command;
use super::{Open, Read, Reader};

/// Everything the reader can be told.
#[derive(Debug, Clone)]
pub enum Message {
    /// An answer from the engine thread.
    Engine(Event),
    /// Read a document.
    Open(Open),
    /// Step the page: `-1` back, `1` forward. The keyboard's arrows and the
    /// bar's own buttons both speak it.
    Turn(i32),
    /// The window moved. The reading area is the window: the chrome is an
    /// overlay, so nothing is subtracted.
    Resized(Size),
    /// The display's scale factor moved.
    Scale(f64),
    /// One zoom intent, posted by whichever door the reader used — the bar's
    /// own control, the `+`/`-` keys. Resolved against the window, the mode and
    /// the sheet under the reader's eyes, and run through the one transition
    /// pipeline.
    Zoom(Command),
    /// A fit mode was chosen. Written straight to the viewer, because it is a
    /// decision rather than a scale: the scale follows it in the same frame,
    /// untweened — a click answers where it lands instead of easing.
    Fit(FitMode),
    /// One animation frame: the app's frames subscription, alive exactly as
    /// long as a transition is ([`Reader::needs_tick`]).
    Tick,
    /// The surface asked to leave for the shelf. Standing on the shelf is the
    /// app's business — the route is the app's — so the app answers this one by
    /// changing route after the reader has let the document go.
    Close,
}

/// What an update asks the app to do. The reader never touches the library
/// blob, the disk or the toast slot itself: it reports, and the app writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A read is worth writing down as a row-recorded read: the open, and the
    /// leave. The only one of the two that may *create* the row a drop of an
    /// unknown file deserves.
    Record(Read),
    /// The reader moved within the book that is already open. Written to the
    /// rows that already answer for the address — a page turn is not a moment
    /// to add books to a library.
    Progress(Read),
    /// Tell the reader something went wrong.
    Toast(Tone, String),
}

impl Reader {
    /// The engine's answers, already wearing the surface's own message type —
    /// the app's subscription is one `.map` away from its route.
    pub fn subscription(&self) -> iced::Subscription<Message> {
        self.engine.subscription().map(Message::Engine)
    }

    /// One message, and what the app owes the world because of it.
    pub fn update(&mut self, message: Message, now: Instant) -> Vec<Effect> {
        match message {
            Message::Engine(event) => self.on_event(event),
            Message::Open(open) => self.begin_open(open),
            Message::Turn(step) => self.turn(step),
            Message::Resized(size) => {
                self.resize(size);
                Vec::new()
            }
            Message::Scale(factor) => {
                self.viewer.dpr = factor.max(0.1);
                // Every page's device box moves with the display, so the frame
                // on screen is re-rasterised on the new grid rather than scaled
                // by the compositor: a 1.5× laptop panel would otherwise read
                // text drawn for a 1× screen.
                self.request_frame();
                Vec::new()
            }
            // The doors all ask for the tween: one press is one gesture, and a
            // gesture is what the 120 ms is for. The watchers — a resize, a
            // chosen fit — come in untweened instead, because they must land in
            // the frame they were asked in.
            Message::Zoom(command) => {
                self.zoom_command(command, true);
                Vec::new()
            }
            Message::Fit(fit) => {
                self.viewer.fit = fit;
                // The window's own follow: it resolves to the fit when one is
                // active and to the reader's own scale otherwise, which is the
                // same rule a resize goes through.
                self.zoom_command(Command::Follow, false);
                Vec::new()
            }
            Message::Tick => {
                let delta = now.saturating_duration_since(self.last_tick);
                self.last_tick = now;
                if self.zoom.advance(delta.as_secs_f64() * 1000.0).committed {
                    // The transaction ended: the scale the rasters are drawn
                    // for has moved, and the page is re-asked for at it.
                    self.request_frame();
                }
                Vec::new()
            }
            Message::Close => self.close(),
        }
    }

    /// Whether the reader owes an animation frame. The app subscribes to the
    /// frames it needs and nothing else: an idle reader costs no redraws.
    pub fn needs_tick(&self) -> bool {
        self.zoom.ticking()
    }

    /// The reading surface.
    pub fn view(&self, tokens: Tokens) -> Element<'_, Message> {
        page::view(self, tokens)
    }
}
