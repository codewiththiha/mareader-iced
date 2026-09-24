//! The PDF engine worker: one thread that owns Pdfium and the open document.
//!
//! The web app's pdf.js ran in a worker the browser owned, and every call
//! crossed a boundary the app could not see into: renders were queued against
//! canvases, a document was torn down by a message that resolved whenever the
//! worker got to it, and the app's idea of what was open could disagree with
//! the engine's. Natively the same shape is one OS thread and a pair of
//! channels: the reading surface posts [`Request`]s, the worker answers with
//! [`Event`]s, and the two share neither a lock, a document nor a bitmap.
//!
//! What the thread owns, and why each thing is here rather than in the app:
//!
//! * **the binding** — Pdfium binds once per process (see
//!   [`crate::platform::pdfium`]), so the thread that binds is the thread every
//!   later call comes from;
//! * **the document** — the only place in the app that may own one, because a
//!   document's life is a load and a drop and the surface must never be the one
//!   deciding when;
//! * **nothing else.** No caches, no queues, no priorities. A page is
//!   rasterised when the reader asks for it, and the reader drops the answer
//!   when it has moved on. The worker is a document with a door.
//!
//! The thread starts on the **first request**, not at launch: a reader who
//! never opens a PDF never loads a 7 MB shared library, and the smoke lane can
//! boot the whole app on a runner that has no Pdfium at all. Its first word is
//! still [`Event::Bound`], because the reader asks for a document and the
//! engine answers with the reason it cannot be served rather than with silence.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use iced::Subscription;
use pdfium_render::prelude::{
    PdfBitmapFormat, PdfBookmark, PdfDocument, PdfDocumentMetadataTagType, PdfPage,
    PdfRenderConfig, Pdfium,
};

use super::channel::{self, EventSink, SharedEvents};
use super::protocol::{Event, FrameKey, Opened, PageBox, Request};
use super::text::{self, RawChar, Run};

/// The UI's handle on the engine: a door to the worker, and the events it
/// answers with. The app owns exactly one, for the whole run.
pub struct Engine {
    /// The send half of the request channel. Created at launch, so a request
    /// posted before the worker exists is still a request the worker will find
    /// waiting when it starts.
    requests: Sender<Request>,
    /// The receive half, parked until the worker starts. Behind a lock because
    /// taking it is what makes the start happen once, and the start is what
    /// needs it.
    inbox: Mutex<Option<Receiver<Request>>>,
    /// The sink the worker answers into.
    sink: EventSink,
    /// The subscription side of that sink.
    events: SharedEvents,
    /// Whether the worker has been started.
    started: AtomicBool,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An engine that has not started its worker yet. Nothing here touches
    /// Pdfium, so this is safe to call while the app boots on a machine that
    /// has no engine at all.
    pub fn new() -> Self {
        let (sink, events) = channel::channel();
        Self::from_parts(sink, events)
    }

    fn from_parts(sink: EventSink, events: SharedEvents) -> Self {
        let (requests, inbox) = std::sync::mpsc::channel::<Request>();
        Self {
            requests,
            inbox: Mutex::new(Some(inbox)),
            sink,
            events,
            started: AtomicBool::new(false),
        }
    }

    /// Post a request. The first one starts the worker; a closed request
    /// channel means the worker is gone (a binding failure ends it), so the
    /// request is dropped — there is nothing left that could answer it.
    pub fn send(&self, request: Request) {
        self.start_worker();
        let _ = self.requests.send(request);
    }

    /// The answers this engine produces, for the app's subscription.
    pub fn subscription(&self) -> Subscription<Event> {
        channel::subscription(Arc::clone(&self.events))
    }

    /// Start the worker, once.
    fn start_worker(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(inbox) = self.inbox.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let sink = Arc::clone(&self.sink);
        // Named, so a stack dump taken during a render says where it was. A
        // spawn failure (the OS is out of threads) is not reported: the
        // receiver drops with the closure, the channel closes, and the app's
        // next request goes nowhere — which is the truth, and nothing is left
        // waiting on an answer.
        let _ = std::thread::Builder::new()
            .name("mareader-pdf".to_string())
            .spawn(move || worker(inbox, &sink));
    }

    /// An engine whose answers go to a sink of the caller's choosing: what the
    /// engine's own test uses to block on events instead of subscribing to
    /// them.
    #[cfg(test)]
    fn with_sink(sink: EventSink) -> Self {
        let (_, events) = channel::channel();
        Self::from_parts(sink, events)
    }

    /// An engine that can never start a worker: what the reader's own tests
    /// build, and the reason is Pdfium's binding, which is process-global.
    /// There is exactly one engine test in this binary, and it owns the
    /// binding; a reader test that started a worker of its own would race it
    /// for the one bind the process is allowed, and the loser would be told
    /// the library is already initialised.
    ///
    /// The door is shut the same way a started engine's is: the worker is
    /// started by taking the receiver out of this slot, and there is no
    /// receiver to take. A request then falls into a channel nobody holds —
    /// which is exactly what a request does in the app after the worker has
    /// gone: dropped, with nothing left waiting on an answer.
    #[cfg(test)]
    pub fn inert() -> Self {
        let engine = Self::new();
        if let Ok(mut slot) = engine.inbox.lock() {
            let _ = slot.take();
        }
        engine
    }
}

/// The document the worker is serving, and the session it belongs to. A
/// `PdfDocument` borrows the `Pdfium` it came from, so it can only live inside
/// the worker's own scope — which is exactly where it is.
struct Open<'a> {
    stamp: u64,
    document: PdfDocument<'a>,
}

/// The worker's life: bind once, then serve documents until the UI lets go.
fn worker(inbox: Receiver<Request>, events: &EventSink) {
    let pdfium = match crate::platform::pdfium::bind() {
        Ok(pdfium) => {
            events(Event::Bound { result: Ok(()) });
            pdfium
        }
        Err(message) => {
            // No engine: the app is told once and the worker exits. Its
            // channel closes, and every later request is dropped on the floor —
            // the honest outcome, because nothing here could answer one.
            events(Event::Bound {
                result: Err(message),
            });
            return;
        }
    };

    let mut open: Option<Open<'_>> = None;
    while let Ok(request) = inbox.recv() {
        match request {
            Request::Open { stamp, path } => {
                open = load(&pdfium, stamp, &path, events);
            }
            // A close for a document that is not open is the ordinary case
            // after a failed open: the reader let go of something it never
            // held, and holding nothing is already the state.
            Request::Close => open = None,
            request => {
                if let Some(session) = open.as_ref() {
                    serve(&session.document, session.stamp, request, events);
                }
                // A render or a text ask for a document that is not open is
                // dropped whole: the reader only ever asks about a document it
                // opened, so this is a request that outlived its document — an
                // open that failed, or a frame asked for while a close was in
                // flight. Answering it would mean inventing a page.
            }
        }
    }
}

/// Load a document and read everything the open needs from it while it is in
/// hand. A refusal leaves nothing behind to serve, and the engine stays alive:
/// the reader may ask for another document without the app restarting.
fn load<'a>(pdfium: &'a Pdfium, stamp: u64, path: &Path, events: &EventSink) -> Option<Open<'a>> {
    match pdfium.load_pdf_from_file(path, None) {
        Ok(document) => {
            let opened = read_open(path, &document);
            events(Event::Opened { stamp, opened });
            Some(Open { stamp, document })
        }
        Err(error) => {
            events(Event::OpenFailed {
                stamp,
                message: readable_error(path, &error.to_string()),
            });
            None
        }
    }
}

/// Serve one request against the open document. The session check is a second
/// line of defence: the reader drops answers from a session it has left, and
/// the engine refuses to work for one.
fn serve(document: &PdfDocument<'_>, session: u64, request: Request, events: &EventSink) {
    match request {
        Request::Frame { stamp, key } if stamp == session => match frame(document, key) {
            Ok((width, height, pixels)) => events(Event::Frame {
                stamp,
                key,
                width,
                height,
                pixels,
            }),
            Err(message) => events(Event::FrameFailed {
                stamp,
                key,
                message,
            }),
        },
        Request::Text { stamp, page } if stamp == session => match text_of(document, page) {
            Ok(runs) => events(Event::PageText { stamp, page, runs }),
            Err(message) => events(Event::TextFailed {
                stamp,
                page,
                message,
            }),
        },
        Request::Outline { stamp } if stamp == session => events(Event::Outline {
            stamp,
            entries: outline_of(document),
        }),
        _ => {}
    }
}

/// Everything the open flow needs, read while the document is in hand: the page
/// count, the geometry of every page, and the document's own idea of its title
/// and author — so no later request has to ask again for a fact that was free
/// at open time.
fn read_open(path: &Path, document: &PdfDocument<'_>) -> Opened {
    let pages = document.pages();
    let num_pages = pages.len().max(0) as u32;
    let mut page_sizes = Vec::with_capacity(num_pages as usize);
    for index in 0..num_pages as i32 {
        // Per page rather than in one sweep, because one page whose box Pdfium
        // refuses must not cost the book its geometry: the vector is filled in
        // order, and a page that fails carries a zero box the reader reads as
        // "unmeasured".
        //
        // The box is the page as the reader will see it: Pdfium reports the
        // media box *after* the page's own `/Rotate`, so a scanned landscape
        // sheet arrives as 792 × 612 rather than as the 612 × 792 its
        // dictionary spells. That is also why nothing here rotates anything.
        let box_ = pages
            .page_size(index)
            .map(|rect| PageBox {
                width: f64::from(rect.width().value),
                height: f64::from(rect.height().value),
            })
            .unwrap_or(PageBox {
                width: 0.0,
                height: 0.0,
            });
        page_sizes.push(box_);
    }
    Opened {
        path: path.to_path_buf(),
        num_pages,
        page_sizes,
        title: metadata_field(document, PdfDocumentMetadataTagType::Title),
        author: metadata_field(document, PdfDocumentMetadataTagType::Author),
    }
}

/// One field of the document's information dictionary, trimmed, and absent when
/// the document carries nothing usable there.
///
/// It reads through the tag list rather than through named accessors because
/// that is the only door the crate leaves open in 0.9 — `metadata().title()` and
/// friends were removed along with the rest of the deprecated surface — and
/// because a document whose `/Title` is a single space has no title: the reader
/// then falls back to the library's own name for the book.
fn metadata_field(
    document: &PdfDocument<'_>,
    tag: PdfDocumentMetadataTagType,
) -> Option<String> {
    let metadata = document.metadata();
    let found = metadata.get(tag)?;
    usable(Some(found.value()))
}

/// Rasterise one page into the key's device-pixel box.
///
/// The page's own rotation is **not** applied here, deliberately, and that is
/// worth stating because applying it looks reasonable and is wrong: Pdfium's
/// `FPDF_GetPageWidthF`/`HeightF` already answer the rotated geometry, and
/// `FPDF_RenderPageBitmap` with `rotate = 0` draws the page upright in that
/// box. Passing the page's rotation through as well turns `/Rotate 90` into a
/// half turn — the fixture's third page and the ctypes verifier beside it exist
/// to keep that from coming back.
fn frame(document: &PdfDocument<'_>, key: FrameKey) -> Result<(u32, u32, Vec<u8>), String> {
    let page = page_of(document, key.page)?;
    let config = PdfRenderConfig::new()
        // Both sides, not just the width: the raster is allocated at exactly
        // the box the host is about to draw, so no side is left for Pdfium to
        // choose and the aspect cannot drift from the rectangle it was
        // measured for.
        .set_target_size(key.width as i32, key.height as i32)
        // Spelled out rather than left to the default, because the whole
        // pipeline downstream assumes it — and because `as_rgba_bytes()` below
        // is only a pass-through when the format really is BGRA.
        .set_format(PdfBitmapFormat::BGRA);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|error| error.to_string())?;
    // The bitmap's own size, because that is what the texture is allocated
    // with. It is the requested box in every case but a cap, and reporting the
    // truth is what keeps a capped raster from being drawn as a distorted page.
    let width = bitmap.width().max(0) as u32;
    let height = bitmap.height().max(0) as u32;
    Ok((width, height, bitmap.as_rgba_bytes()))
}

/// One page's text, grouped into runs in the reader's own space — see
/// [`super::text`] for the flip and the grouping rule.
fn text_of(document: &PdfDocument<'_>, page: u32) -> Result<Vec<Run>, String> {
    let page = page_of(document, page)?;
    let page_height = f64::from(page.height().value);
    let text = page.text().map_err(|error| error.to_string())?;
    let chars = text.chars();
    // `len()` is already a `usize` (Pdfium's own character index type), so
    // this is a capacity hint and nothing else.
    let mut raw = Vec::with_capacity(chars.len());
    for ch in chars.iter() {
        // A character Pdfium cannot box is skipped rather than inserted at the
        // origin: one broken glyph would otherwise drag a run's rectangle
        // across the whole page.
        let Ok(bounds) = ch.loose_bounds() else {
            continue;
        };
        let (y, _) = text::flip(
            f64::from(bounds.top().value),
            f64::from(bounds.bottom().value),
            page_height,
        );
        raw.push(RawChar {
            ch: ch.unicode_char().unwrap_or(text::UNMAPPED),
            x: f64::from(bounds.left().value),
            y,
            w: f64::from(bounds.width().value),
            h: f64::from(bounds.height().value),
            size: f64::from(ch.scaled_font_size().value),
        });
    }
    Ok(text::runs(&raw))
}

/// The document's chapter tree, flattened in document order with each
/// chapter's depth — the shape `pdf_core::outline::to_nodes` cleans into the
/// reader's own nodes.
///
/// Walked by hand rather than through the crate's own depth-first iterator, for
/// the one thing that iterator does not report: depth. Two ceilings keep a
/// malformed outline from becoming a liability — a graph that points at itself
/// stops at [`MAX_DEPTH`], and a pathological book stops at [`MAX_ENTRIES`].
fn outline_of(document: &PdfDocument<'_>) -> Vec<pdf_core::outline::OutlineEntry> {
    let mut entries = Vec::new();
    let Some(root) = document.bookmarks().root() else {
        return entries;
    };
    // `root()` answers the first bookmark of the top level; its siblings are
    // the rest of that level.
    let siblings = root.iter_siblings();
    for node in std::iter::once(root).chain(siblings) {
        push_bookmark(&node, 0, &mut entries);
    }
    entries
}

const MAX_DEPTH: u32 = 8;
const MAX_ENTRIES: usize = 2_000;

fn push_bookmark(
    node: &PdfBookmark<'_>,
    depth: u32,
    entries: &mut Vec<pdf_core::outline::OutlineEntry>,
) {
    if entries.len() >= MAX_ENTRIES || depth > MAX_DEPTH {
        return;
    }
    let title = node.title().unwrap_or_default().trim().to_string();
    if !title.is_empty() {
        // A bookmark whose destination never resolved arrives as page 0, which
        // `to_nodes` drops: a chapter nobody can jump to is not a chapter the
        // panel should offer.
        let page = node
            .destination()
            .and_then(|destination| destination.page_index().ok())
            .map(|index| index.max(0) as u32 + 1)
            .unwrap_or(0);
        entries.push(pdf_core::outline::OutlineEntry { title, page, depth });
    }
    for child in node.iter_direct_children() {
        push_bookmark(&child, depth + 1, entries);
    }
}

/// The page at a 1-based number, as the reader counts pages.
///
/// The returned page carries the document's own lifetime rather than the
/// borrow of the reference: Pdfium's handles are the document's, and the
/// caller only ever uses the page while the document it came from is in hand.
fn page_of<'a>(document: &PdfDocument<'a>, page: u32) -> Result<PdfPage<'a>, String> {
    let index = page.saturating_sub(1).min(i32::MAX as u32) as i32;
    document
        .pages()
        .get(index)
        .map_err(|error| error.to_string())
}

/// A metadata field worth showing: trimmed, and absent when blank. A document
/// whose title is a single space has no title, and the reader then falls back
/// to the library's own name for the book.
fn usable(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Pdfium reports errors as codes; the reader gets a sentence, and Pdfium's own
/// words are kept inside it for a bug report.
fn readable_error(path: &Path, error: &str) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| PathBuf::from(path).display().to_string());
    format!("Could not open {name}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The fixture: three US Letter pages, the third carrying `/Rotate 90`, a
    /// two-level chapter tree and both kinds of link. Written by
    /// `tests/fixtures/make_three_pages.py` and checked by
    /// `verify_three_pages.py` against a real Pdfium.
    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/three-pages.pdf")
    }

    /// Wait for an answer, and say what was actually seen when none comes.
    fn next(rx: &Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(30)).expect("the engine answered")
    }

    /// The engine against a real Pdfium library and a real file: one test
    /// rather than five, because Pdfium binds once per process and a second
    /// engine in the same binary would be refused. Driving the whole protocol
    /// in one place is also the only way to prove it end to end — the worker is
    /// the thing under test, not a helper beside it.
    ///
    /// Skipped when no Pdfium is found, so a contributor on a clean machine
    /// still gets green tests; CI sets `MAREAEDER_REQUIRE_PDFIUM=1`, which
    /// turns that skip into a failure — a lane that passes because the engine
    /// was absent proves nothing.
    #[test]
    fn the_engine_opens_measures_renders_and_reads_a_document() {
        let path = fixture();
        // The sink a worker may call from its own thread: `Send + Sync` by
        // construction, because the sender the test listens on is behind a
        // lock rather than asked to be shareable.
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Arc::new(Mutex::new(tx));
        let engine = Engine::with_sink(Arc::new(move |event| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(event);
            }
        }));

        // The worker starts on the first request, so the first word is still
        // the bind result — it just arrives after the ask rather than before.
        engine.send(Request::Open {
            stamp: 1,
            path: path.clone(),
        });
        let opened = loop {
            match next(&rx) {
                Event::Bound { result: Ok(()) } => continue,
                Event::Bound { result: Err(message) } => {
                    if std::env::var("MAREAEDER_REQUIRE_PDFIUM").is_ok() {
                        panic!("Pdfium was required but could not be bound: {message}");
                    }
                    eprintln!("skipping the engine test — no Pdfium: {message}");
                    return;
                }
                Event::Opened { stamp, opened } => {
                    assert_eq!(stamp, 1, "the answer names the session that asked");
                    break opened;
                }
                Event::OpenFailed { message, .. } => panic!("the fixture did not open: {message}"),
                _ => continue,
            }
        };

        assert_eq!(opened.num_pages, 3);
        assert_eq!(opened.page_sizes.len(), 3);
        let first = opened.first_page();
        assert!(
            (first.width - 612.0).abs() < 1.0 && (first.height - 792.0).abs() < 1.0,
            "page 1 is US Letter: {first:?}"
        );
        // The rotated page: Pdfium reports its box after the quarter turn, and
        // the engine must not turn it again when it renders.
        let third = opened.page_sizes[2];
        assert!(
            (third.width - 792.0).abs() < 1.0 && (third.height - 612.0).abs() < 1.0,
            "page 3 is landscape because of its own /Rotate: {third:?}"
        );
        assert_eq!(opened.title.as_deref(), Some("Three Pages"));
        assert_eq!(opened.author.as_deref(), Some("Mareader"));

        // A raster in a box the reader chose, at whole device pixels.
        let key = FrameKey::of_css(1, first.width, first.height, 1.0);
        engine.send(Request::Frame { stamp: 1, key });
        let (width, height, pixels) = loop {
            match next(&rx) {
                Event::Frame {
                    key: answered,
                    width,
                    height,
                    pixels,
                    ..
                } => {
                    assert_eq!(answered, key, "the frame answers the key that asked");
                    break (width, height, pixels);
                }
                Event::FrameFailed { message, .. } => panic!("page 1 did not render: {message}"),
                _ => continue,
            }
        };
        assert_eq!((width, height), (key.width, key.height));
        assert_eq!(
            pixels.len(),
            key.width as usize * key.height as usize * 4,
            "RGBA, four bytes to the pixel"
        );
        // The fixture's page 1 carries a filled bar and a line of text: a frame
        // with no ink is a frame of blank paper, which is exactly what a size
        // assertion cannot see.
        let inked = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[0] < 0x80 || px[1] < 0x80 || px[2] < 0x80);
        assert!(inked, "the frame is blank: nothing drew");

        // The rotated page, in its own box: the same check the ctypes verifier
        // makes, so a rotation bug fails here too.
        let rotated = FrameKey::of_css(3, third.width * 0.5, third.height * 0.5, 1.0);
        engine.send(Request::Frame {
            stamp: 1,
            key: rotated,
        });
        let pixels = loop {
            match next(&rx) {
                Event::Frame {
                    key: answered,
                    pixels,
                    ..
                } => {
                    assert_eq!(answered, rotated);
                    break pixels;
                }
                Event::FrameFailed { message, .. } => panic!("page 3 did not render: {message}"),
                _ => continue,
            }
        };
        // The fixture's square was drawn at the top-left of the *unrotated*
        // sheet. Rendered into the landscape box with no extra rotation, the
        // ink lands in the right-hand half; a page rotated twice would put it
        // on the left.
        let right_half = {
            let (w, h) = (rotated.width as usize, rotated.height as usize);
            let mut dark = 0usize;
            for row in 0..h {
                for col in w / 2..w {
                    let px = &pixels[(row * w + col) * 4..][..4];
                    if px[0] < 0x80 || px[1] < 0x80 || px[2] < 0x80 {
                        dark += 1;
                    }
                }
            }
            dark > 100
        };
        assert!(right_half, "page 3 was rotated twice: its ink is not where /Rotate 90 puts it");

        // Text, in the reader's own space: the fixture prints "Mareader" on
        // page 1, and the run carries a sane box rather than the origin.
        engine.send(Request::Text { stamp: 1, page: 1 });
        let runs = loop {
            match next(&rx) {
                Event::PageText { runs, page, .. } => {
                    assert_eq!(page, 1);
                    break runs;
                }
                Event::TextFailed { message, .. } => panic!("page 1 has no text: {message}"),
                _ => continue,
            }
        };
        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        assert!(
            text.contains("Mareader"),
            "page 1's text was {text:?}, which does not contain the fixture's word"
        );
        let run = runs
            .iter()
            .find(|run| run.text.contains("Mareader"))
            .expect("the run carrying the word");
        assert!(run.w > 0.0 && run.h > 0.0 && run.x >= 0.0 && run.y >= 0.0);
        assert!(run.y < first.height, "the run sits on the page");

        // The chapter tree: two levels, in document order, with their depths.
        engine.send(Request::Outline { stamp: 1 });
        let entries = loop {
            match next(&rx) {
                Event::Outline { entries, .. } => break entries,
                _ => continue,
            }
        };
        let listed: Vec<(&str, u32, u32)> = entries
            .iter()
            .map(|entry| (entry.title.as_str(), entry.page, entry.depth))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("Chapter One", 1, 0),
                ("Section One", 2, 1),
                ("Chapter Two", 3, 0),
            ]
        );

        // Answers for a session the engine has left are not served at all.
        let stale = FrameKey {
            page: 1,
            width: 8,
            height: 8,
        };
        engine.send(Request::Frame { stamp: 99, key: stale });

        // A close is the reader going back to the shelf, and the engine stays
        // ready: a second open after it answers exactly as the first did.
        engine.send(Request::Close);
        engine.send(Request::Open {
            stamp: 2,
            path,
        });
        let reopened = loop {
            match next(&rx) {
                Event::Opened { stamp, opened } => {
                    assert_eq!(stamp, 2);
                    break opened;
                }
                Event::Frame { .. } => panic!("the engine served a session it had left"),
                Event::OpenFailed { message, .. } => {
                    panic!("the fixture did not reopen: {message}")
                }
                _ => continue,
            }
        };
        assert_eq!(reopened.num_pages, 3);
    }
}
