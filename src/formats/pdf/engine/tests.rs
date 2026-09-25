//! The engine's own cases: a real document opened, measured, rendered and read.

use super::*;
use crate::formats::pdf::protocol::FrameKey;
use std::path::{Path, PathBuf};
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
