//! The document the reader has open, in the fields every format shares.
//!
//! A port of the web app's `DocumentState` — the status ladder, the identity,
//! the page count and the geometry — with the parts that belonged to other
//! subsystems left where they live: the search model is `pdf_core`'s, the
//! gloss marks are P7's, and the reflowable content is P4's.
//!
//! The ladder is the whole reason this is a type rather than a handful of
//! fields: `Opening` is what tells the route to show the reading surface
//! before there is anything to read, `Ready` is the only state in which a
//! frame is asked for, and `Error` is a sentence the reader can act on. The
//! web app kept the same three, for the same reasons.

use std::path::PathBuf;

use reader_core::outline::OutlineNode;

use crate::formats::pdf::{PageBox, fallback_box, measured};

use super::Open;

/// How far along a document's open is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocStatus {
    /// Nothing is open: the shelf is what is on screen.
    #[default]
    Idle,
    /// The engine has the file and has not answered yet. The reading surface
    /// is already mounted — a placeholder is what a reader expects to see the
    /// moment they open a book — and no frame has been asked for.
    Opening,
    /// The engine answered: the geometry is known and the pages can be read.
    Ready,
    /// The file could not be read, or there is no engine to read it with. The
    /// sentence is the reader's; the surface shows it and keeps the way back.
    Error,
}

impl DocStatus {
    /// Whether the reading surface may ask the engine for a page. The one
    /// guard in front of every render request.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// The open document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub status: DocStatus,
    /// The address on disk. `None` while nothing is open.
    pub path: Option<PathBuf>,
    /// The page the library remembers for this book, read before the engine
    /// was asked for anything and clamped to the book that actually opened.
    pub resume: u32,
    /// The library row this open belongs to, when the reader named one. The
    /// shelf's Open passes the row it clicked; a drop or a dialog passes
    /// nothing, and the app settles the address onto a shared row before the
    /// open — the web app's rule, kept so a private book of its own is never
    /// hijacked by a drop of the same file.
    pub book_id: Option<String>,
    /// The library's own name for the row, when the reader opened one: the
    /// row's *display* name, so a book copied into the store never reads
    /// "source" in the title bar. It answers for the title only when the
    /// document itself supplied nothing worth showing.
    pub row_title: Option<String>,
    /// The document's own title, when it carries one worth showing.
    pub title: Option<String>,
    pub author: Option<String>,
    pub num_pages: u32,
    /// The page each sheet measured at, in document order. Empty for a
    /// document whose pages could not be measured — the reader then answers
    /// with the A4 fallback for every page rather than laying out against a
    /// zero.
    pub sizes: Vec<PageBox>,
    /// The chapter tree, resolved after the open (the web app asked for it only
    /// once page 1 was on screen, because resolving destinations is not free).
    /// Empty until it answers, and empty for a book without one.
    pub outline: Vec<OutlineNode>,
    /// What went wrong, in the reader's own words.
    pub error: Option<String>,
    /// The name the surfaces show — the bar's centre, the error card, the
    /// window's own title. Kept rather than resolved per frame, because the
    /// rule walks three inputs through a filter and the bar asks for it on
    /// every frame; it is written by the two writes that move the identity,
    /// the open and the seed.
    name: String,
}

impl Document {
    /// What a document being opened looks like before the engine has said
    /// anything about it: the address, the row and its name, and the resume
    /// page the library remembered. Written under `Opening`, so the surface can
    /// paint the book's name and a placeholder at once.
    pub fn begin(&mut self, open: &Open) {
        self.status = DocStatus::Opening;
        self.path = Some(open.path.clone());
        self.book_id = open.book_id.clone();
        self.row_title = open.row_title.clone();
        self.title = None;
        self.author = None;
        self.num_pages = 0;
        self.sizes.clear();
        self.outline.clear();
        self.error = None;
        self.resume = open.resume;
        // The book already has a name at this point — the row's, or the
        // file's — which is what the placeholder shows while the engine reads
        // the file.
        self.refresh_name();
    }

    /// Back to nothing open. Called when the reader leaves for the shelf.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The box of a 1-based page: its measured size, else page 1's, else the
    /// A4 fallback. Never a zero, because the fit arithmetic divides by it.
    pub fn page_box(&self, page: u32) -> PageBox {
        let first = self
            .sizes
            .first()
            .copied()
            .filter(|box_| measured(*box_))
            .unwrap_or_else(fallback_box);
        page.checked_sub(1)
            .and_then(|index| self.sizes.get(index as usize).copied())
            .filter(|box_| measured(*box_))
            .unwrap_or(first)
    }

    /// Re-resolve the name the surfaces show.
    ///
    /// Called wherever the identity moves: the open, which writes the address
    /// and the row's name, and the seed, which writes the document's own title.
    pub fn refresh_name(&mut self) {
        self.name = self.resolve_name();
    }

    /// The name the title bar, the error card and the window title read.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether a document is open at all — the address is written the moment
    /// an open starts, so this is true from the first placeholder frame.
    pub fn is_open(&self) -> bool {
        self.path.is_some()
    }

    /// The name to show for this document, in the order the web app resolved
    /// it: `services::document::open::enter::identity` wrote the document's own
    /// title when it was one worth showing, and the library's name for the row
    /// when it was not; both surfaces then rendered that through
    /// `reader_core::filename::display_name`, which is where the placeholder
    /// filter and the address's stem live.
    ///
    /// The row arrives as its *display* name — the library's `Book::title`,
    /// which falls back to the name the file was imported under rather than the
    /// address it now lives at. That is what keeps a book copied into the store
    /// from reading `source` in the title bar: the copy's address is the
    /// store's own `source.pdf`, and the row's name is the only thing that
    /// knows better.
    fn resolve_name(&self) -> String {
        // The document's own title, filtered: a PDF's `/Title` is free-form
        // and frequently a download's file name, which is the whole reason
        // `is_usable_title` exists.
        if let Some(own) = reader_core::filename::document_title(self.title.as_deref()) {
            return own;
        }
        // Then the library's name for the row, through the same filter: a
        // stored name is a promise, but it still has to look like a name.
        if let Some(row) = self.row_title.as_deref().map(str::trim).filter(|t| !t.is_empty())
            && let Some(named) = reader_core::filename::document_title(Some(row))
        {
            return named;
        }
        // Last, the address: the file's stem where the open had no row to name
        // it — a drop, a picker, a recent address.
        let address = self.path.as_deref().and_then(std::path::Path::to_str);
        reader_core::filename::display_name(None, address)
            .unwrap_or_else(|| "the document".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use crate::formats::pdf;
    use crate::formats::pdf::{Event, FrameKey};
    use crate::reader::kit::{reader, reader_of, seeded, tick};
    use crate::reader::kit::open as kit_open;
    use iced::Size;
    use reader_core::settings::Settings;
    use reader_core::zoom_math::FitMode;
    fn open() -> Open {
        Open {
            path: PathBuf::from("/store/items/b1/source.pdf"),
            book_id: Some("b1".to_string()),
            row_title: Some("Dune".to_string()),
            resume: 42,
        }
    }

    /// The name as a surface reads it: refresh, then read — the two writes the
    /// reader makes whenever the identity moves.
    fn shown(document: &mut Document) -> String {
        document.refresh_name();
        document.name().to_string()
    }

    #[test]
    fn a_row_name_answers_when_the_document_has_none_of_its_own() {
        // A stored copy's address is the store's own `source.pdf`; the row's
        // name is what keeps every copied book from reading "source".
        let mut document = Document::default();
        document.begin(&open());
        assert_eq!(document.name(), "Dune", "the open names the book at once");
        document.row_title = None;
        assert_eq!(shown(&mut document), "source");
    }

    #[test]
    fn the_documents_own_title_outranks_the_rows_name() {
        // The web app's order, and the one the reader's title bar and its
        // floating label both read: the document's `/Title` when it is one
        // worth showing, and the library's name for the row only when it is
        // not.
        let mut document = Document::default();
        document.begin(&open());
        document.title = Some("Dune (deluxe edition)".to_string());
        assert_eq!(shown(&mut document), "Dune (deluxe edition)");
        // A title the filter refuses hands the name back to the row.
        document.title = Some("0321894073.pdf".to_string());
        assert_eq!(shown(&mut document), "Dune");
    }

    #[test]
    fn a_document_that_was_never_measured_still_has_a_box() {
        // The reader lays out against this before the engine has answered, so
        // it must never be a zero — and A4 is what a document with nothing to
        // say is measured as.
        let document = Document::default();
        let box_ = document.page_box(3);
        assert!(measured(box_));
        assert!((box_.width - 595.276).abs() < 0.01);
    }

    #[test]
    fn a_blank_row_name_does_not_win() {
        let mut document = Document::default();
        let mut named = open();
        named.row_title = Some("   ".to_string());
        document.begin(&named);
        assert_eq!(shown(&mut document), "source");
    }

    #[test]
    fn a_fresh_document_is_idle_and_holds_nothing() {
        let mut document = Document::default();
        document.begin(&open());
        assert_eq!(document.status, DocStatus::Opening);
        document.reset();
        assert_eq!(document.status, DocStatus::Idle);
        assert!(document.path.is_none());
        assert_eq!(document.num_pages, 0);
    }

    #[test]
    fn an_open_that_never_finished_writes_nothing_to_the_shelf() {
        // A failed kit_open has no position worth writing: the library already
        // holds the resume point it handed in, and a write here would move the
        // book to page 1.
        let mut reader = reader();
        let effects = reader.begin_open(kit_open());
        assert!(effects.is_empty(), "a document that is still opening is not an error");
        assert!(reader.close().is_empty());
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
        reader.begin_open(kit_open());
        reader.turn(1);
        assert_eq!(reader.viewer.page, 1);
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
        reader.document.begin(&kit_open());
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
        reader.document.begin(&kit_open());
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
    fn the_blank_state_before_an_open_has_one_open_page() {
        // Nothing kit_open: the viewer still answers with a page 1 box, because the
        // surface is built before the first answer arrives.
        let reader = reader();
        assert_eq!(reader.viewer.page, 1);
        assert!(reader.frame_here().is_none());
        let (w, h) = reader.page_box_px();
        assert!(w > 0.0 && h > 0.0);
        // Nothing is kit_open, so nothing names the reader's route: the bar falls
        // back to the app's own title.
        assert!(!reader.document.is_open());
        assert_eq!(reader.name(), "");
    }
}
