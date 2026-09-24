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

pub use document::{DocStatus, Document};
pub use viewer::Viewer;

use std::path::PathBuf;
use std::time::Instant;

use iced::widget::image::Handle;
use iced::{Element, Size};

use reader_core::settings::Settings;
use reader_core::zoom_math::FitMode;

use crate::formats::pdf::{self, Engine, Event, FrameKey, Request};
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

/// The page raster on screen: the texture the page host paints, the key it
/// answers, and the box it actually came back as.
///
/// The box is kept because the host may draw it at `width / dpr` logical px
/// rather than at the box it asked for: Pdfium keeps the page's aspect ratio,
/// and the raster it returns is the truth about what was drawn.
#[derive(Debug, Clone)]
struct Frame {
    key: FrameKey,
    width: u32,
    height: u32,
    handle: Handle,
}

impl Frame {
    /// The box this frame occupies on screen, in logical px.
    fn css_box(&self, dpr: f64) -> (f32, f32) {
        let dpr = if dpr > 0.0 { dpr } else { 1.0 };
        (
            (f64::from(self.width) / dpr) as f32,
            (f64::from(self.height) / dpr) as f32,
        )
    }
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
        // The page belonged to the book that just closed; the reader's own
        // layout — mode, fit, scale — is theirs and survives it.
        self.viewer.reset_position();
        self.engine.send(Request::Close);
        effects
    }

    /// One message, and what the app owes the world because of it.
    pub fn update(&mut self, message: Message, _now: Instant) -> Vec<Effect> {
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
            Message::Close => self.close(),
        }
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
                    width,
                    height,
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
        // of jumping to it a moment later.
        self.viewer.scale = self.viewer.resolved_scale(page1);
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
        // decides whether the scale follows it. An active fit always follows;
        // a hand-picked zoom follows only while the setting is on.
        if self.viewer.auto_resize && self.viewer.fit != FitMode::None {
            let box_ = self.document.page_box(next);
            self.viewer.scale = self.viewer.resolved_scale(box_);
        }
        self.request_frame();
        self.effect_of(Effect::Progress)
    }

    /// The window changed size. A fit follows it; a hand-picked scale (3b's)
    /// does not — that is the whole difference between the two, and the reason
    /// this asks the viewer rather than deciding here.
    fn resize(&mut self, size: Size) {
        self.viewer.container = size;
        if self.document.status.is_ready() {
            let box_ = self.document.page_box(self.viewer.page);
            self.viewer.scale = self.viewer.resolved_scale(box_);
            self.request_frame();
        }
    }

    /// Ask the engine for the page under the reader's eyes, in the box it
    /// occupies on screen.
    fn request_frame(&mut self) {
        if !self.document.status.is_ready() || !self.viewer.measured() {
            return;
        }
        let (width, height) = self.page_box_px();
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

    /// The box the page occupies on screen, in CSS px: the frame's own box when
    /// one is up, else the box the next frame will occupy.
    ///
    /// Snapped to the device-pixel grid (`pdf_core::pixel_grid`), and the same
    /// number the raster is asked for in — one arithmetic, so the page's edge
    /// is a whole device pixel and a page turn cannot leave a hairline of
    /// background along the seam. A capped raster comes back smaller than this
    /// box and is drawn up to it; every other frame lands on it exactly.
    fn page_box_px(&self) -> (f32, f32) {
        let (width, height) = self.viewer.page_px(self.document.page_box(self.viewer.page));
        let snapped = (
            pdf_core::pixel_grid::snap_px(f64::from(width)) as f32,
            pdf_core::pixel_grid::snap_px(f64::from(height)) as f32,
        );
        if let Some(frame) = self.frame_here() {
            let (fw, fh) = frame.css_box(self.viewer.dpr);
            return (snapped.0.max(fw), snapped.1.max(fh));
        }
        snapped
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
        let seeded_scale = reader.viewer.scale;
        reader.resize(Size::new(800.0, 1000.0));
        assert!((reader.viewer.scale - seeded_scale).abs() < 1e-9);
        assert!((seeded_scale - 800.0 / 612.0).abs() < 1e-9);
    }

    #[test]
    fn a_window_with_no_size_yet_does_not_fit_the_page() {
        // Before the first `Resized`, the container is unmeasured: the page
        // keeps the scale it has rather than being slammed to the minimum.
        let mut reader = reader();
        seeded(&mut reader, 3, 1);
        assert!((reader.viewer.scale - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_resize_refits_a_page_and_moves_the_scale_with_the_window() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.resize(Size::new(400.0, 1000.0));
        assert!((reader.viewer.scale - 400.0 / 612.0).abs() < 1e-9);
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
            width: key.width,
            height: key.height,
            handle: Handle::from_rgba(1, 1, vec![0, 0, 0, 0]),
        });
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
        assert!((reader.viewer.scale - 800.0 / 612.0).abs() < 1e-9);
        reader.turn(1);
        assert!(
            (reader.viewer.scale - 800.0 / 1224.0).abs() < 1e-9,
            "the landscape plate is fitted on its own terms"
        );
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
        let scale = reader.viewer.scale;
        reader.turn(1);
        assert!(
            (reader.viewer.scale - scale).abs() < 1e-9,
            "the reader's own scale is not moved behind their back"
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
