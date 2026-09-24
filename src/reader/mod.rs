//! The reading surface: the document on screen and everything that moves it.
//!
//! The web app's reader was a route, a store of signals and a pile of effects
//! reacting to each other — the page watching the scroll, the zoom watching the
//! container, the search watching the page. Natively the same facts live in one
//! struct ([`Reader`]) that the Elm loop updates, and the only things it talks
//! to are the engine thread (requests out, events in) and the app it belongs to
//! (effects out). Everything else — the fit arithmetic, the page geometry, the
//! status ladder — is pure and tested beside itself.
//!
//! Deliberately not here: writing to the shelf. The reader reports *what*
//! happened ([`Effect`]) and the app owns every byte that reaches the library
//! blob, the disk or a toast — the same split the rest of the app keeps.

mod document;
mod page;
mod viewer;
mod zoom;

pub use document::{DocStatus, Document};
pub use viewer::Viewer;
pub use zoom::Command;

use std::path::PathBuf;
use std::time::Instant;

use iced::widget::image::Handle;
use iced::{Element, Size};

use reader_core::settings::Settings;
use reader_core::zoom_math::FitMode;

use crate::formats::pdf::{self, Engine, Event, FrameKey, Request};

use zoom::{Advanced, Zoom};
use crate::theme::Tokens;
use crate::ui::toast::Tone;

/// The book the app asks the reader to open: where it lives, and what the
/// library already knows about it.
///
/// The resume point travels *in* rather than being looked up here, because
/// which row answers for an address is the library's business and the app has
/// the rows in hand when it dispatches the open — the web app read its resume
/// point before the engine was asked for anything, so a concurrent progress
/// write from the closing document could not clobber it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub path: PathBuf,
    /// The library row the reader named, when they named one.
    pub book_id: Option<String>,
    /// The row's own name for the book — the library's display name for it,
    /// which is what answers for the title when the document itself carries
    /// none worth showing.
    pub row_title: Option<String>,
    /// The page the library remembers.
    pub resume: u32,
}

/// The reader's whole state.
pub struct Reader {
    pub document: Document,
    pub viewer: Viewer,
    /// The three scales and the transition in flight between them — the web
    /// app's zoom pipeline, which is the one owner of how big the page is.
    zoom: Zoom,
    /// The instant the last animation frame was handed out. A tween's progress
    /// is measured from it, so the reader's clock is the app's own frame rate
    /// rather than a timer of its own.
    last_tick: Instant,
    /// The page raster on screen, if any.
    frame: Option<Frame>,
    /// The engine worker, started by the first request it is given.
    engine: Engine,
    /// Whether the engine found a Pdfium library, once it has said. `None`
    /// before its first word — an open that arrives first still goes through,
    /// because the answer may be on its way.
    bound: Option<Result<(), String>>,
    /// The session stamp: which open every message belongs to. Bumped by every
    /// open and every close.
    session: u64,
    /// The raster the reader is waiting for, if any. A frame that answers any
    /// other key is not the one the page on screen is waiting on.
    awaiting: Option<FrameKey>,
}

/// The page raster on screen: the texture the page host paints, and the key it
/// answers.
///
/// The key is kept so a frame that belongs to another page — or another device
/// grid, or another session — can be told apart from the one the page host is
/// waiting for; the raster's own pixel box is not, because the page is drawn at
/// the box its scale resolves to and the texture is fitted to it either way.
#[derive(Debug, Clone)]
struct Frame {
    key: FrameKey,
    handle: Handle,
}

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

/// Where the reader is, as the library wants to hear it: which row the open
/// belongs to, what the document calls itself, and how far in the reader got.
///
/// The web app's reading-progress effect watched the page signal and wrote the
/// library as the reader moved; natively the same fact is handed out on
/// request, and the app — which owns the blob, the disk and the stamp — decides
/// what to make of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub path: PathBuf,
    /// The library row this open belongs to, when the reader opened one.
    pub book_id: Option<String>,
    /// The document's own title, when it is one worth persisting: the metadata
    /// half of `reader_core::filename::display_name`, never the address's
    /// stem. A shelf row that filled its name from whatever address the open
    /// ran on would write the store's own `source.pdf` over a stored book.
    pub title: Option<String>,
    pub author: Option<String>,
    pub page: u32,
    pub num_pages: u32,
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
    /// The reader, with its engine ready but not yet started: no Pdfium is
    /// loaded until the first document is asked for, so a run that never opens
    /// a PDF never touches the engine at all.
    pub fn new(settings: &Settings) -> Self {
        let layout = &settings.layout;
        let mut viewer = Viewer {
            fit: layout.default_fit,
            margin: layout.page_margin,
            auto_resize: layout.auto_resize,
            ..Viewer::default()
        };
        // `None` is not a startup fit: `sanitize` has already replaced a
        // persisted `None` with the default, and a fit of `None` would leave
        // the scale at 1 with nothing owning it.
        if viewer.fit == FitMode::None {
            viewer.fit = FitMode::Width;
        }
        Self {
            document: Document::default(),
            viewer,
            zoom: Zoom::default(),
            last_tick: Instant::now(),
            frame: None,
            engine: Engine::new(),
            bound: None,
            session: 0,
            awaiting: None,
        }
    }

    /// The engine's answers, already wearing the surface's own message type —
    /// the app's subscription is one `.map` away from its route.
    pub fn subscription(&self) -> iced::Subscription<Message> {
        self.engine.subscription().map(Message::Engine)
    }

    /// The name the title bar shows for what is open. A borrow rather than a
    /// fresh string: the bar asks for it while it builds its element, and the
    /// document keeps it resolved (see `Document::refresh_name`).
    pub fn name(&self) -> &str {
        self.document.name()
    }

    /// Start reading a document.
    ///
    /// An engine that already said it has no library answers here and now: a
    /// request posted to a worker that has exited would leave the reader
    /// waiting on an answer nobody is left to send.
    fn begin_open(&mut self, open: Open) -> Vec<Effect> {
        self.session += 1;
        self.awaiting = None;
        self.frame = None;
        self.document.begin(&open);
        match self.bound.clone() {
            Some(Err(message)) => {
                self.document.status = DocStatus::Error;
                self.document.error = Some(message.clone());
                vec![Effect::Toast(Tone::Error, message)]
            }
            _ => {
                self.engine.send(Request::Open {
                    stamp: self.session,
                    path: open.path,
                });
                Vec::new()
            }
        }
    }

    /// Let the document go and tell the app where the reader stopped, so the
    /// shelf can resume them there.
    pub fn close(&mut self) -> Vec<Effect> {
        // Taken before the reset, which is what forgets the name of the book.
        let effects = self.effect_of(Effect::Record);
        self.session += 1;
        self.awaiting = None;
        self.frame = None;
        self.document.reset();
        // Whatever was still moving belonged to the book that just closed;
        // a transition left open would keep asking for frames of a page
        // nobody is reading.
        self.zoom.cancel();
        // The page belonged to the book that just closed; the reader's own
        // layout — mode, fit, scale — is theirs and survives it.
        self.viewer.reset_position();
        self.engine.send(Request::Close);
        effects
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

    /// Post one zoom intent: resolve it against the state it lands in, record
    /// what the resolution decided, and open the transition it asks for.
    ///
    /// The one door every zoom goes through — the bar's buttons, the keyboard's
    /// rungs, the window's own follow and a page turn's re-fit — so a fit
    /// cannot race a gesture along a second path.
    fn zoom_command(&mut self, command: Command, animate: bool) -> Advanced {
        let sheet = self.document.page_box(self.viewer.page);
        let Some(target) = zoom::resolve(&self.viewer, &self.zoom, sheet, command) else {
            return Advanced::NONE;
        };
        if self.zoom.note(target) {
            // A hand-picked scale owns the zoom now: the fit mode steps aside,
            // or it would fight the gesture on the next window change.
            self.viewer.fit = FitMode::None;
        }
        let following = command == Command::Follow;
        let did = self.zoom.begin(target.scale, animate, following);
        if did.moved {
            // The instant the tween is measured from. The app's ticks carry
            // their own instants, and this is the one the first of them compares
            // against — a stale one would land the tween in a single frame.
            self.last_tick = Instant::now();
        }
        if did.committed {
            self.request_frame();
        }
        did
    }

    /// An answer from the engine.
    fn on_event(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Bound { result } => {
                let failed = result.clone().err();
                self.bound = Some(result);
                match (failed, self.document.status) {
                    // The engine has no library and a document is waiting on
                    // it: that wait is over, and the sentence is the reason.
                    (Some(message), DocStatus::Opening) => {
                        self.document.status = DocStatus::Error;
                        self.document.error = Some(message.clone());
                        vec![Effect::Toast(Tone::Error, message)]
                    }
                    _ => Vec::new(),
                }
            }
            Event::Opened { stamp, opened } => {
                if stamp != self.session {
                    // A document the reader has already left. Its geometry
                    // would be written over the book they are reading now.
                    return Vec::new();
                }
                self.seed(opened);
                // The chapter tree is asked for only now: page 1 can be painted
                // without it, and resolving a textbook's destinations is not
                // free.
                self.engine.send(Request::Outline {
                    stamp: self.session,
                });
                self.request_frame();
                // The open writes a read point at once, so a reader who quits
                // with the book still open does not lose their place — and so
                // a file the library has never seen joins it as a linked row
                // wearing the name the document gave.
                self.effect_of(Effect::Record)
            }
            Event::OpenFailed { stamp, message } => {
                if stamp != self.session {
                    return Vec::new();
                }
                self.document.status = DocStatus::Error;
                self.document.error = Some(message.clone());
                vec![Effect::Toast(Tone::Error, message)]
            }
            Event::Frame {
                stamp,
                key,
                width,
                height,
                pixels,
            } => {
                if stamp != self.session {
                    return Vec::new();
                }
                if self.awaiting == Some(key) {
                    self.awaiting = None;
                }
                if key.page != self.viewer.page {
                    // A page the reader has left. This is the whole reason the
                    // frame key exists: the answer is not painted over the page
                    // on screen, and it is not kept either — one raster lives
                    // in the reader, and it is the page under the reader's eyes.
                    return Vec::new();
                }
                self.frame = Some(Frame {
                    key,
                    handle: Handle::from_rgba(width, height, pixels),
                });
                Vec::new()
            }
            Event::FrameFailed {
                stamp, key, message, ..
            } => {
                if stamp != self.session {
                    return Vec::new();
                }
                if self.awaiting == Some(key) {
                    self.awaiting = None;
                }
                vec![Effect::Toast(Tone::Error, message)]
            }
            Event::PageText { .. } | Event::TextFailed { .. } => {
                // Text answers belong to the search index, which is built on
                // the first search (3e). Nothing asks for one yet, so nothing
                // can arrive here — the arm exists because the protocol is one
                // enum the engine owns, not a per-increment view of it.
                Vec::new()
            }
            Event::Outline { stamp, entries } => {
                if stamp != self.session {
                    return Vec::new();
                }
                self.document.outline =
                    pdf_core::outline::to_nodes(entries, self.document.num_pages);
                Vec::new()
            }
        }
    }

    /// The document is open: seed everything the fresh mount reads, in the
    /// order that makes each step safe — the web app's `seed`, with the search
    /// and gloss state it also reset belonging to their own increments.
    fn seed(&mut self, opened: pdf::Opened) {
        let page1 = opened.first_page();
        self.document.path = Some(opened.path);
        self.document.num_pages = opened.num_pages;
        self.document.sizes = opened.page_sizes;
        self.document.title = opened.title;
        self.document.author = opened.author;
        self.document.outline.clear();
        self.document.error = None;
        // The document's own title has arrived, and with it the name the bar
        // shows: the metadata now outranks the row's name for the rest of the
        // read.
        self.document.refresh_name();
        // The resume point is clamped to the book that actually opened: a
        // re-edited document may have fewer pages than the shelf remembers, and
        // a saved 0 must never resume before the book.
        self.viewer.page = self.document.resume.clamp(1, self.document.num_pages.max(1));
        // The seed scale is the same fit the first live refit will resolve, so
        // the first frame already sits where the fit is going to land instead
        // of jumping to it a moment later — and every scale is seeded at once,
        // so nothing is left holding the previous book's number.
        let seeded = self.viewer.resolved_scale(page1, self.zoom.committed);
        self.zoom.initialize(seeded);
        self.frame = None;
        self.awaiting = None;
        // `Ready` LAST: it is what makes the surface ask for a frame, and
        // everything a frame is drawn against is already in place.
        self.document.status = DocStatus::Ready;
    }

    /// Where the reader is, or `None` while nothing is open — or while the
    /// open never finished, because the library already holds the resume point
    /// it handed in and writing now would move the book to page 1.
    pub fn read(&self) -> Option<Read> {
        let path = self.document.path.clone()?;
        if !self.document.status.is_ready() && self.document.num_pages == 0 {
            return None;
        }
        Some(Read {
            path,
            book_id: self.document.book_id.clone(),
            // The metadata half only: a title the document actually supplied
            // is worth persisting, and the stem of whatever address the open
            // ran on is the fallback's business at the moment it is shown.
            title: reader_core::filename::document_title(self.document.title.as_deref()),
            author: self.document.author.clone(),
            // Clamped by the same rule an open's resume point goes through: a
            // page of 0 or one past the end is a transient that escaped the
            // syncs, and writing it would be the next open's starting point.
            page: self
                .viewer
                .page
                .clamp(1, self.document.num_pages.max(1)),
            num_pages: self.document.num_pages,
        })
    }

    /// The read, as the effect the caller asked for.
    fn effect_of(&self, kind: fn(Read) -> Effect) -> Vec<Effect> {
        self.read().map(kind).into_iter().collect()
    }

    /// One page forward or back.
    ///
    /// The step clamps to the book rather than wrapping: a reader leaning on
    /// the next button at the end of a book expects to arrive at the end, not
    /// at the beginning. Spread stepping and the horizontal strip's own
    /// ordering arrive with those modes in 3c.
    fn turn(&mut self, step: i32) -> Vec<Effect> {
        if !self.document.status.is_ready() || step == 0 {
            return Vec::new();
        }
        let last = self.document.num_pages.max(1);
        let next = self
            .viewer
            .page
            .saturating_add_signed(step)
            .clamp(1, last);
        if next == self.viewer.page {
            // A press with nowhere to go writes nothing: the shelf already
            // holds this page, and a read stamp that moves on its own would
            // reorder a shelf the reader never moved on.
            return Vec::new();
        }
        self.viewer.page = next;
        // A page turn may land on a differently sized sheet — a landscape plate
        // inside a portrait book — and the web app's `auto_resize` is what
        // decides whether the scale follows it. On, the new sheet is resolved
        // the way the window's own changes are: the fit when one owns the
        // scale, the reader's own zoom when it does not. Off, the turn touches
        // nothing, and a plate too wide for the window overflows and scrolls.
        //
        // Untweened, deliberately: the answer is not a gesture but the page
        // arriving — the same reasoning that keeps a window resize finding the
        // new size in the frame it was reported. (The web app debounced this by
        // its settle window because a SCROLL moves pages continuously; a strip
        // that does arrives in 3c, and the debounce belongs with it.)
        let moved = if self.viewer.auto_resize {
            let command = if self.viewer.fit == FitMode::None {
                Command::Constrain
            } else {
                Command::Refit
            };
            self.zoom_command(command, false).committed
        } else {
            false
        };
        if !moved {
            // The page under the reader's eyes changed, so a raster is owed for
            // it whether or not the zoom moved the scale the old one was drawn
            // at.
            self.request_frame();
        }
        self.effect_of(Effect::Progress)
    }

    /// The window changed size.
    ///
    /// The space around the page is a *follow*, not a refit: the layout moves
    /// with the window every time it is reported — a scale that waits for the
    /// drag to end leaves the page wider than its box, and the flex arithmetic
    /// that would have to squish it is exactly what the web app refused to do —
    /// while the crisp raster waits for the burst to go quiet, so a drag costs
    /// one render rather than one per frame. A fit tracks the window; a
    /// hand-picked zoom keeps its own scale and overflows instead, which is the
    /// difference between the two, and the reason this resolves through the
    /// pipeline rather than deciding here.
    fn resize(&mut self, size: Size) {
        self.viewer.container = size;
        if self.document.status.is_ready() {
            self.zoom_command(Command::Follow, false);
        }
    }

    /// Ask the engine for the page under the reader's eyes, in the box it
    /// occupies at the settled scale.
    fn request_frame(&mut self) {
        if !self.document.status.is_ready() || !self.viewer.measured() {
            return;
        }
        let (width, height) = self.frame_box_px();
        let key = FrameKey::of_css(
            self.viewer.page,
            f64::from(width),
            f64::from(height),
            self.viewer.dpr,
        );
        self.awaiting = Some(key);
        self.engine.send(Request::Frame {
            stamp: self.session,
            key,
        });
    }

    /// The page raster for the page on screen, if the one in hand is that
    /// page's. A frame for the page the reader just left stays out of the way
    /// until its replacement lands.
    fn frame_here(&self) -> Option<&Frame> {
        self.frame
            .as_ref()
            .filter(|frame| frame.key.page == self.viewer.page)
    }

    /// The box the page occupies on screen this frame, in CSS px: the settled
    /// box while nothing moves, and the box a transition is passing through
    /// while one does. The raster in hand is drawn up to it — stretched, the way
    /// the web app's page hosts stretched theirs — which is what makes a zoom
    /// read as the paper itself changing size rather than a jump at the end.
    fn page_box_px(&self) -> (f32, f32) {
        self.box_at(self.zoom.display)
    }

    /// The box the raster is asked for, in CSS px at the committed scale — the
    /// only scale a frame is ever drawn at, so a zoom in flight never asks the
    /// engine for a size the reader is already past.
    fn frame_box_px(&self) -> (f32, f32) {
        self.box_at(self.zoom.committed)
    }

    /// The page's box at `scale`, snapped to the device-pixel grid
    /// (`pdf_core::pixel_grid`): the paint box and the raster box are the same
    /// arithmetic, so the page's edge is a whole device pixel and a page turn
    /// cannot leave a hairline of background along the seam. A capped raster
    /// comes back smaller than this box and is drawn up to it.
    fn box_at(&self, scale: f64) -> (f32, f32) {
        let (width, height) = self
            .viewer
            .page_px(self.document.page_box(self.viewer.page), scale);
        (
            pdf_core::pixel_grid::snap_px(f64::from(width)) as f32,
            pdf_core::pixel_grid::snap_px(f64::from(height)) as f32,
        )
    }

    /// The reading surface.
    pub fn view(&self, tokens: Tokens) -> Element<'_, Message> {
        page::view(self, tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_core::settings::Settings;

    use std::time::Duration;

    /// One animation frame, `delta_ms` after the last: exactly what the app's
    /// tick carries while the pipeline owes one.
    fn tick(reader: &mut Reader, delta_ms: f64) {
        let now = reader.last_tick + Duration::from_secs_f64(delta_ms / 1000.0);
        reader.update(Message::Tick, now);
    }

    /// A reader whose engine can never start a worker (see
    /// [`Engine::inert`]): these tests are about the surface's own arithmetic,
    /// and none of them touches Pdfium.
    fn reader_of(settings: &Settings) -> Reader {
        let mut reader = Reader::new(settings);
        reader.engine = Engine::inert();
        reader
    }

    fn reader() -> Reader {
        let mut settings = Settings::default();
        settings.layout.default_fit = FitMode::Width;
        settings.layout.page_margin = 0.0;
        reader_of(&settings)
    }

    fn open() -> Open {
        Open {
            path: PathBuf::from("/books/one.pdf"),
            book_id: None,
            row_title: None,
            resume: 1,
        }
    }

    /// A document seeded by hand: the engine is not asked for anything, so
    /// these tests are about the reader's own arithmetic and never touch
    /// Pdfium.
    fn seeded(reader: &mut Reader, pages: u32, resume: u32) {
        reader.session += 1;
        let mut opened = open();
        opened.resume = resume;
        reader.document.begin(&opened);
        reader.seed(pdf::Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: pages,
            page_sizes: vec![
                pdf::PageBox {
                    width: 612.0,
                    height: 792.0,
                };
                pages as usize
            ],
            title: Some("One".to_string()),
            author: None,
        });
    }

    #[test]
    fn an_open_that_never_finished_writes_nothing_to_the_shelf() {
        // A failed open has no position worth writing: the library already
        // holds the resume point it handed in, and a write here would move the
        // book to page 1.
        let mut reader = reader();
        let effects = reader.begin_open(open());
        assert!(effects.is_empty(), "a document that is still opening is not an error");
        assert!(reader.close().is_empty());
    }

    #[test]
    fn the_resume_page_is_clamped_to_the_book_that_opened() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 40);
        assert_eq!(reader.viewer.page, 3, "a book with three pages resumes at 3");
        let effects = reader.close();
        match effects.as_slice() {
            [Effect::Record(read)] => {
                assert_eq!((read.page, read.num_pages), (3, 3));
                assert_eq!(read.path, PathBuf::from("/books/one.pdf"));
                assert_eq!(read.title.as_deref(), Some("One"));
            }
            other => panic!("expected one record, got {other:?}"),
        }
    }

    #[test]
    fn a_turn_reports_the_new_position_and_a_dead_press_reports_nothing() {
        // The web app's progress effect wrote the library as the reader moved;
        // natively the move is the reader's report and the write is the app's.
        // A press with nowhere to go reports nothing, so a shelf the reader
        // never moved on does not reorder itself.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        match reader.turn(1).as_slice() {
            [Effect::Progress(read)] => assert_eq!(read.page, 2),
            other => panic!("expected one progress report, got {other:?}"),
        }
        // At the end of the book: no move, no write.
        reader.turn(1);
        assert!(reader.turn(1).is_empty());
        assert_eq!(reader.viewer.page, 3);
    }

    #[test]
    fn a_read_names_the_row_and_only_a_title_worth_persisting() {
        // The row is what keeps a book of the reader's own from handing its
        // position to its twin at the address; the title is the metadata half
        // only, because the address's stem is a display fallback and writing
        // it down would rename a stored book after the store's own layout.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.session += 1;
        reader.document.begin(&Open {
            path: PathBuf::from("/store/items/b1/source.pdf"),
            book_id: Some("b1".to_string()),
            row_title: Some("Dune".to_string()),
            resume: 1,
        });
        reader.seed(pdf::Opened {
            path: PathBuf::from("/store/items/b1/source.pdf"),
            num_pages: 2,
            page_sizes: vec![
                pdf::PageBox {
                    width: 612.0,
                    height: 792.0,
                };
                2
            ],
            title: Some("Untitled".to_string()),
            author: Some("Frank Herbert".to_string()),
        });
        let read = reader.read().expect("a book is open");
        assert_eq!(read.book_id.as_deref(), Some("b1"));
        assert_eq!(read.title, None, "a placeholder title is not worth writing down");
        assert_eq!(read.author.as_deref(), Some("Frank Herbert"));
    }

    #[test]
    fn the_seed_scale_is_the_fit_the_resize_will_resolve() {
        // The first frame must already sit where the first refit lands: the
        // same geometry answers both, so a page never jumps a moment after it
        // appears.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let seeded_scale = reader.zoom.committed;
        assert!(
            (reader.zoom.display - seeded_scale).abs() < 1e-9,
            "the three scales agree the moment a book opens"
        );
        reader.resize(Size::new(800.0, 1000.0));
        assert!((reader.zoom.committed - seeded_scale).abs() < 1e-9);
        assert!((seeded_scale - 800.0 / 612.0).abs() < 1e-9);
    }

    #[test]
    fn a_window_with_no_size_yet_does_not_fit_the_page() {
        // Before the first `Resized`, the container is unmeasured: the page
        // keeps the scale it has rather than being slammed to the minimum.
        let mut reader = reader();
        seeded(&mut reader, 3, 1);
        assert!((reader.zoom.committed - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_resize_follows_the_window_and_sharpens_once_it_stops() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.resize(Size::new(400.0, 1000.0));
        assert!(
            (reader.zoom.display - 400.0 / 612.0).abs() < 1e-9,
            "the page is in the new window on the frame it was reported"
        );
        assert!(
            (reader.zoom.committed - 800.0 / 612.0).abs() < 1e-9,
            "while the raster waits for the burst to end"
        );
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 400.0 / 612.0).abs() < 1e-9);
    }

    #[test]
    fn a_turn_clamps_at_both_ends_of_the_book() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.turn(-1);
        assert_eq!(reader.viewer.page, 1, "there is no page 0");
        reader.turn(1);
        reader.turn(1);
        reader.turn(1);
        assert_eq!(reader.viewer.page, 3, "and no page 4");
        reader.turn(0);
        assert_eq!(reader.viewer.page, 3);
    }

    #[test]
    fn a_turn_before_the_book_is_ready_does_nothing() {
        let mut reader = reader();
        reader.begin_open(open());
        reader.turn(1);
        assert_eq!(reader.viewer.page, 1);
    }

    #[test]
    fn the_page_host_draws_the_frame_it_has_and_the_box_it_expects() {
        // The box on screen never depends on whether a frame has arrived: the
        // page must not change size the moment its raster lands.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let expected = reader.page_box_px();
        assert!(expected.0 > 0.0 && expected.1 > 0.0);
        let key = FrameKey::of_css(1, f64::from(expected.0), f64::from(expected.1), 1.0);
        reader.frame = Some(Frame {
            key,
            handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
        });
        assert!(reader.frame_here().is_some(), "the page's own raster is up");
        assert_eq!(reader.page_box_px(), expected, "the page does not move when its raster lands");
    }

    #[test]
    fn a_frame_for_another_page_is_not_painted() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let (w, h) = reader.page_box_px();
        let stale = FrameKey::of_css(2, f64::from(w), f64::from(h), 1.0);
        let effects = reader.on_event(Event::Frame {
            stamp: reader.session,
            key: stale,
            width: stale.width,
            height: stale.height,
            pixels: vec![0, 0, 0, 0],
        });
        assert!(effects.is_empty());
        assert!(reader.frame_here().is_none(), "the page on screen is still blank paper");
        assert!(reader.frame.is_none(), "and the raster is not kept either");
    }

    #[test]
    fn an_answer_from_a_session_the_reader_left_is_ignored() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let stale = reader.session + 7;
        let key = FrameKey::of_css(1, 800.0, 1035.0, 1.0);
        let effects = reader.on_event(Event::Frame {
            stamp: stale,
            key,
            width: key.width,
            height: key.height,
            pixels: vec![0, 0, 0, 0],
        });
        assert!(effects.is_empty());
        assert!(reader.frame.is_none());
    }

    #[test]
    fn a_turn_onto_a_different_sheet_refits_when_the_setting_says_so() {
        // A landscape plate inside a portrait book: with `auto_resize` on, the
        // fit follows the sheet under the reader's eyes.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.session += 1;
        reader.document.begin(&open());
        reader.seed(pdf::Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: 2,
            page_sizes: vec![
                pdf::PageBox {
                    width: 612.0,
                    height: 792.0,
                },
                pdf::PageBox {
                    width: 1224.0,
                    height: 792.0,
                },
            ],
            title: None,
            author: None,
        });
        assert!((reader.zoom.committed - 800.0 / 612.0).abs() < 1e-9);
        reader.turn(1);
        assert!(
            (reader.zoom.committed - 800.0 / 1224.0).abs() < 1e-9,
            "the landscape plate is fitted on its own terms"
        );
        assert!(!reader.needs_tick(), "and it lands in the frame the turn did");
    }

    #[test]
    fn a_turn_keeps_the_scale_when_the_reader_asked_it_to() {
        // The other half of `auto_resize`: off, a wide plate overflows and
        // scrolls (3b's affordance) rather than shrinking the text.
        let mut settings = Settings::default();
        settings.layout.default_fit = FitMode::Width;
        settings.layout.auto_resize = false;
        let mut reader = reader_of(&settings);
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.session += 1;
        reader.document.begin(&open());
        reader.seed(pdf::Opened {
            path: PathBuf::from("/books/one.pdf"),
            num_pages: 2,
            page_sizes: vec![
                pdf::PageBox {
                    width: 612.0,
                    height: 792.0,
                },
                pdf::PageBox {
                    width: 1224.0,
                    height: 792.0,
                },
            ],
            title: None,
            author: None,
        });
        let scale = reader.zoom.committed;
        reader.turn(1);
        assert!(
            (reader.zoom.committed - scale).abs() < 1e-9,
            "the reader's own scale is not moved behind their back"
        );
    }

    #[test]
    fn a_step_moves_the_page_at_once_and_sharpens_it_when_it_lands() {
        // The gesture's contract, and the whole reason three scales are kept:
        // the paper grows under the reader's eyes while the raster on it is the
        // one that was already crisp, and the engine is asked exactly once — at
        // the end, for the size the page actually came to rest at.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let before = reader.zoom.committed;
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        assert!(reader.needs_tick(), "a gesture owes frames");
        assert!(
            (reader.zoom.display - before).abs() < 1e-9,
            "the tween opens where the eye already is"
        );
        // The tween's first frame: the paper grows, and the raster on it is
        // still the one drawn for the size the engine was last asked at.
        tick(&mut reader, 60.0);
        assert!(reader.zoom.display > before, "the page grew");
        assert!(
            (reader.zoom.committed - before).abs() < 1e-9,
            "and the raster is drawn for the size it was asked at"
        );
        assert!(
            reader.page_box_px().0 > reader.frame_box_px().0,
            "the paper on screen is the bigger of the two"
        );
        // The rung above a 800/612 fit width.
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 1.5).abs() < 1e-9);
        assert!((reader.zoom.display - 1.5).abs() < 1e-9);
        assert!(!reader.needs_tick());
    }

    #[test]
    fn a_window_drag_moves_the_page_every_frame_and_rasterises_once_at_the_end() {
        // The follow's contract: the layout is in the new window on the frame
        // it was reported — a scale that waited for the drag to end would leave
        // the page wider than its box — while the RENDER waits, so a drag costs
        // one raster pass rather than one per frame.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let before = reader.zoom.committed;
        reader.resize(Size::new(600.0, 1000.0));
        assert!((reader.zoom.display - 600.0 / 612.0).abs() < 1e-9);
        reader.resize(Size::new(400.0, 1000.0));
        assert!((reader.zoom.display - 400.0 / 612.0).abs() < 1e-9);
        assert!((reader.zoom.committed - before).abs() < 1e-9, "still no raster");
        // Each frame of the burst pushes the deadline back…
        tick(&mut reader, 100.0);
        assert!((reader.zoom.committed - before).abs() < 1e-9);
        assert!(reader.needs_tick(), "the burst is not over");
        // …and the quiet at the end commits once, at the size it stopped at.
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 400.0 / 612.0).abs() < 1e-9);
        assert!(!reader.needs_tick());
    }

    #[test]
    fn a_hand_picked_zoom_is_not_shrunk_back_to_the_fit_ceiling() {
        // The ceiling a manual zoom resolves to is what the reader chose, not
        // fit width: a page zoomed in on keeps its scale and overflows with a
        // scroll affordance (3c's), rather than snapping back to fit.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        assert!((reader.zoom.desired - 1.5).abs() < 1e-9);
        assert_eq!(reader.viewer.fit, FitMode::None, "the gesture dropped the fit");
        // A window the fit would now answer differently: the reader's own zoom
        // is what the page keeps.
        reader.resize(Size::new(1400.0, 1000.0));
        assert!((reader.zoom.display - 1.5).abs() < 1e-9);
        assert!((reader.zoom.committed - 1.5).abs() < 1e-9, "nothing moved, so nothing renders");
    }

    #[test]
    fn choosing_a_fit_answers_in_the_frame_it_lands() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        assert_eq!(reader.viewer.fit, FitMode::None);
        // A click, not a gesture: the page is in its new size on the frame the
        // answer landed (the raster sharpens on the held commit behind it).
        reader.update(Message::Fit(FitMode::Page), Instant::now());
        assert_eq!(reader.viewer.fit, FitMode::Page);
        assert!(
            (reader.zoom.display - 1000.0 / 792.0).abs() < 1e-9,
            "the whole sheet shows: the height is the binding side"
        );
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 1000.0 / 792.0).abs() < 1e-9);
        assert!((reader.zoom.desired - 1000.0 / 792.0).abs() < 1e-9, "a fit owns the ceiling too");
    }

    #[test]
    fn closing_keeps_the_reader_s_own_zoom_and_drops_what_was_moving() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        reader.turn(1);
        let scale = reader.zoom.committed;
        // A gesture still in flight when the reader leaves: dropped, or it
        // would keep asking the engine for frames of a page nobody is reading.
        reader.update(Message::Zoom(Command::Step(-1)), Instant::now());
        assert!(reader.needs_tick());
        reader.close();
        assert!(!reader.needs_tick());
        assert_eq!(reader.viewer.page, 1);
        assert!(
            (reader.zoom.committed - scale).abs() < 1e-9,
            "the reader's own zoom survives the book"
        );
    }

    #[test]
    fn the_blank_state_before_an_open_has_one_open_page() {
        // Nothing open: the viewer still answers with a page 1 box, because the
        // surface is built before the first answer arrives.
        let reader = reader();
        assert_eq!(reader.viewer.page, 1);
        assert!(reader.frame_here().is_none());
        let (w, h) = reader.page_box_px();
        assert!(w > 0.0 && h > 0.0);
        // Nothing is open, so nothing names the reader's route: the bar falls
        // back to the app's own title.
        assert!(!reader.document.is_open());
        assert_eq!(reader.name(), "");
    }
}
