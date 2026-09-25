//! The reading route: the reader's own effects, the resize that follows the
//! window, and the doors that open a document.
use std::path::PathBuf;

use iced::time::Instant;
use iced::widget::{operation, scrollable};
use iced::{Size, Task};
use library_core::book::{self, Book};
use reader_core::view::Axis;

use crate::platform::{fs, now_ms};
use crate::reader;
use crate::route::Route;
use crate::ui::toast::Tone;
use super::Mareader;
use super::message::Message;

impl Mareader {
    /// The window's new size, to the reading surface. The chrome is an overlay,
    /// so the reading area is the window itself; only the reader's own rail
    /// takes a bite out of it, and the reader is the one holding its width.
    pub(super) fn reader_resize(&mut self, size: Size) -> Task<Message> {
        let effects = self
            .reader
            .update(reader::Message::Resized(size), Instant::now());
        self.apply_reader_effects(effects)
    }

    /// The reader's reports, written: a read goes into the library blob and the
    /// disk, a sentence into the toast slot. The reader never touches either —
    /// it says what happened, and the app owns every byte that lands.
    pub(super) fn apply_reader_effects(&mut self, effects: Vec<reader::Effect>) -> Task<Message> {
        let mut wrote = false;
        let mut created: Option<String> = None;
        let mut scrolls = Vec::new();
        for effect in effects {
            match effect {
                reader::Effect::Record(read) => {
                    let address = read.path.to_string_lossy().into_owned();
                    // A PDF's resume point is a page and a page count: it never
                    // inherits the fraction a reflowable book left in the same
                    // slot.
                    let point = book::ReadPoint {
                        page: read.page,
                        num_pages: read.num_pages,
                        fraction: None,
                    };
                    let landed = match read.book_id.as_deref() {
                        // The reader named a row: an id is the only thing that
                        // tells two rows of one address apart, and it is what
                        // distinguishes an independent book from a shared one.
                        Some(id) => book::record_read_row(
                            &mut self.library.books,
                            id,
                            &address,
                            read.title,
                            read.author,
                            point,
                            now_ms(),
                        ),
                        None => book::record_read(
                            &mut self.library.books,
                            &address,
                            read.title,
                            read.author,
                            point,
                            now_ms(),
                        ),
                    };
                    // A book the library did not know joins it as a linked row
                    // at the front of "All", wearing a placeholder fingerprint
                    // and carrying the name the document gave. It is measured
                    // on the way past, exactly as a freshly imported file is:
                    // otherwise a watched folder would keep refusing to rescan
                    // it until the next launch.
                    if let Some(book) = landed {
                        created = Some(book.path().to_string());
                    }
                    wrote = true;
                }
                reader::Effect::Progress(read) => {
                    let address = read.path.to_string_lossy().into_owned();
                    // The same rows the progress effect wrote in the web app:
                    // the reader's own book when it named one, every shared row
                    // at the address otherwise. Nothing is created here — a
                    // page turn is not a moment to add books to a library —
                    // and nothing but the position is touched.
                    for index in book::rows_for_read(
                        &self.library.books,
                        read.book_id.as_deref(),
                        &address,
                    ) {
                        if let Some(book) = self
                            .library
                            .books
                            .get_mut(index)
                            .and_then(book::Row::as_book_mut)
                        {
                            book.page = read.page.clamp(1, read.num_pages.max(1));
                            book.fraction = None;
                        }
                    }
                    wrote = true;
                }
                reader::Effect::Toast(tone, sentence) => {
                    self.toasts.show(tone, sentence, Instant::now());
                }
                reader::Effect::Scroll { axis, offset } => {
                    // The strip is one widget with one name, and only the app can
                    // post a command to a widget: the reader decides where its
                    // own surface sits, and the operation that puts it there is
                    // written here.
                    let offset = offset as f32;
                    let absolute = scrollable::AbsoluteOffset {
                        x: (axis == Axis::Horizontal).then_some(offset),
                        y: (axis == Axis::Vertical).then_some(offset),
                    };
                    scrolls.push(operation::scroll_to(reader::SCROLL_ID, absolute));
                }
                reader::Effect::Outline(offset) => {
                    // The rail's own list is a second widget with a name of its
                    // own: the same door, another surface.
                    scrolls.push(operation::scroll_to(
                        reader::OUTLINE_ID,
                        scrollable::AbsoluteOffset {
                            x: None,
                            y: Some(offset),
                        },
                    ));
                }
            }
        }
        let mut tasks = vec![];
        if wrote {
            tasks.push(self.persist_library());
        }
        if let Some(address) = created {
            // The one measurement an open owes a row it just created: its
            // fingerprint, so the library stops holding a placeholder.
            tasks.push(Task::perform(
                async move { fs::check_paths(std::slice::from_ref(&address)) },
                Message::ChecksDone,
            ));
        }
        tasks.extend(scrolls);
        Task::batch(tasks)
    }

    /// Admit a document through the gate, remember it, and read it — the
    /// shape of the web app's open flow: the address is gated first, the
    /// library's row for it is settled second, and only then is the engine
    /// asked for anything.
    pub(super) fn open_document_path(&mut self, path: PathBuf) -> Task<Message> {
        self.open_row(None, path)
    }

    /// The same door, told *which* row the reader meant. An id is what keeps a
    /// book the reader imported privately from resuming at the page a shared
    /// copy of the same file left behind.
    ///
    /// The web app's open flow, in the order it ran it: the gate first, the
    /// row second, the resume point third — read *before* the engine is asked
    /// for anything, so a progress write from the book that is still open
    /// cannot overwrite the page this open begins at.
    pub(super) fn open_row(&mut self, book_id: Option<String>, path: PathBuf) -> Task<Message> {
        let address = path.to_string_lossy().into_owned();

        // A row the library already knows is dead does not open onto a
        // document at all. The web app answers a click on a missing book with
        // the Find-again question, which keeps the row, its shelves and its
        // page exactly as they are; until that sheet lands, the honest answer
        // is a sentence rather than a reader standing on a file that is not
        // there.
        if let Some(book) = book_id
            .as_deref()
            .and_then(|id| book::find_by_id(&self.library.books, id))
            .filter(|book| book.path() == address)
            .filter(|book| book.missing)
        {
            let name = book.title();
            self.toasts.show(
                Tone::Error,
                format!(
                    "{name} is not where the library last saw it. Rescan the folder it \
                     lived in and the shelf will point the book at its new home.",
                ),
                Instant::now(),
            );
            return Task::none();
        }

        // Which row answers for this address: the one the reader named when
        // they named one, else a *shared* row at the address. A book of its
        // own is the reader's private instance of the file, and an open that
        // arrived as nothing but an address has not said it meant that one — a
        // drop must never hijack a private book's resume point.
        let row = book_id
            .as_deref()
            .and_then(|id| book::find_by_id(&self.library.books, id))
            .filter(|book| book.path() == address)
            .or_else(|| {
                book::book_rows(&self.library.books)
                    .find(|book| book.path() == address && !book.independent)
            });

        // The address is gated after the row is settled, so a book the library
        // knows is asked the sharper question first: is the file readable at
        // all? A file that is gone is the missing-book gate's news, not this
        // one's.
        if let Err(error) = fs::ensure_readable_document(&address) {
            self.toasts.show(Tone::Error, error, Instant::now());
            return Task::none();
        }

        // The row the open ends up belonging to, and the name it wears: the
        // reader's own pick when they picked one, else the shared row that
        // answers for the address.
        let settled = row.map(|book| book.id.clone());
        // The row's *display* name, not its raw title: the library answers with
        // the name the file was imported under when the row carries none of its
        // own, and that is what keeps a stored copy from introducing itself by
        // the `source.pdf` it lives at.
        let row_title = row.map(Book::title);

        // The resume point, through the ported rule — read with the SETTLED
        // row, not the one the caller named, because that is the order the web
        // app read it in: a drop of a file whose first row at the address is a
        // private book resumes where the shared copy's reader left off, not
        // where the private copy's did.
        let (resume, _fraction) =
            book::resume_point(&self.library.books, settled.as_deref(), &address);
        let open = reader::Open {
            path,
            book_id: settled,
            row_title,
            // A PDF's resume point is a page and nothing else: no stream
            // position rides along, whatever a reflowable book left in the
            // slot.
            resume,
        };
        self.settings.last_path = Some(address);
        self.open_book(open)
    }

    /// Hand a document to the reading surface and stand on the reader route.
    /// The reading position the library remembers travels in with it, so the
    /// engine can be asked for the right page the first time.
    fn open_book(&mut self, open: reader::Open) -> Task<Message> {
        let effects = self
            .reader
            .update(reader::Message::Open(open), Instant::now());
        self.route = Route::Reader;
        self.menu = None;
        let writes = self.apply_reader_effects(effects);
        Task::batch([self.persist_settings(), writes])
    }

    // ── The boot-and-focus measurements ─────────────────────────────────
}
