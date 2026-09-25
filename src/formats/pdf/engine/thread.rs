//! The engine's thread: it drains the inbox one request at a time and holds
//! the document its thread owns.

use pdfium_render::prelude::{PdfDocument, Pdfium};
use std::path::Path;
use std::sync::mpsc::Receiver;
use super::read::{outline_of, read_open, readable_error};
use super::render::{frame, text_of};
use crate::formats::pdf::channel::EventSink;
use crate::formats::pdf::protocol::{Event, Request};

/// The document the worker is serving, and the session it belongs to. A
/// `PdfDocument` borrows the `Pdfium` it came from, so it can only live inside
/// the worker's own scope — which is exactly where it is.
struct Open<'a> {
    stamp: u64,
    document: PdfDocument<'a>,
}

/// The worker's life: bind once, then serve documents until the UI lets go.
pub(super) fn worker(inbox: Receiver<Request>, events: &EventSink) {
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
