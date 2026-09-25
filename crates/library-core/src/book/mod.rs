//! The book schema: [`Fingerprint`], [`Origin`], [`Book`] and the [`Row`] a
//! library list holds. The rules that act on rows live one module per question —
//! [`read`], [`merge`], [`check`], [`sanitize`], [`naming`], [`query`] — with the
//! cases for all of it in `tests.rs`.

pub mod check;
pub mod merge;
pub mod naming;
pub mod query;
pub mod read;
pub mod sanitize;
pub use check::{add_book, apply_check, drop_dangling_links, drop_dead_shelf_links, remove_row};
pub use merge::{fold_books, further_point};
pub use naming::{duplicate_title, stem_of};
pub use query::{find_book_mut, find_by_id, find_by_path, index_by_id, resume_point};
pub use read::{ReadPoint, record_read, record_read_row, rows_for_read};
pub use sanitize::{sanitize};

use reader_core::format::Format;
use serde::{Deserialize, Serialize};

/// Storage guard on the library's size, not a "recent books" cap: it keeps the
/// persisted blob inside the browser's quota. Past it, the least-recently-read
/// books go.
pub const BOOKS_CAP: usize = 2000;

/// A book's content identity, compared against files on disk during a rescan.
/// `size` + `mtime_ms` survive a move; `head_hash` separates two different
/// books with the same length and stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

/// How the app holds a book's bytes: [`Origin::Linked`] is the path itself
/// (the default), [`Origin::Stored`] is a copy inside the app's own store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// The book is the path; the app never moves, renames or deletes it. A
    /// vanished path is a missing book with a Relink affordance.
    Linked { src: String },
    /// A copy in the store. `src` is provenance — what the copy was made from,
    /// kept so Relink can copy again while the source still exists.
    Stored { src: Option<String>, store: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// Stable through a relink, a rename and a move between shelves; it is the
    /// drag payload and the shelf member.
    pub id: String,
    pub fp: Fingerprint,
    /// The reader's rename ([`Book::title_locked`]), else the title captured at
    /// open time. `None` until one of the two happens, which is why
    /// [`Book::title`] falls back to the stem rather than storing a guess.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    pub format: Format,
    pub origin: Origin,
    /// The "Date added" sort's key.
    #[serde(default)]
    pub added_ms: u64,
    /// `0` for a book imported but never read, which sorts last rather than first.
    #[serde(default)]
    pub last_read_ms: u64,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    /// Written only while stream mode is live; `None` elsewhere, where `page`
    /// is the whole truth.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// Set by a path check, never by a scan: a missing book keeps its row and
    /// shelf membership so Relink can heal it.
    #[serde(default)]
    pub missing: bool,
    /// Placeholder mark a migrated book carries until the first path check
    /// replaces it. No rescan may run while one is set: a real fingerprint
    /// never matches a placeholder, so the folder would re-add books the reader
    /// already has.
    #[serde(default)]
    pub fp_pending: bool,
    /// Marks a copy the conflict sheet answered *as new*. Two rows of one
    /// address are otherwise twins (reads, path checks, highlights and covers
    /// are all keyed by the address); an independent row reads, resumes and
    /// moves on its own.
    #[serde(default)]
    pub independent: bool,
    /// A name the reader typed or accepted from the conflict sheet.
    /// [`crate::book::sanitize`] treats it as a promise: its rule that drops
    /// filename-shaped titles heals download debris, and a chosen name is not
    /// debris.
    #[serde(default)]
    pub title_locked: bool,
}

fn default_page() -> u32 {
    1
}

/// One row of the library's list: a [`Book`], or a [`Row::Link`] pointing at
/// one. A link is the row a second copy of one file would otherwise have been;
/// it has no fingerprint, address or resume point of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    Book(Book),
    /// A pointer at a book. Nothing measures it; its name is the target's at
    /// the moment the link was made.
    #[serde(rename_all = "camelCase")]
    Link {
        id: String,
        name: String,
        target: String,
        #[serde(default)]
        added_ms: u64,
    },
}

/// What every content rule walks: a link has no fingerprint to compare, no
/// address to check and no resume point to write.
pub fn book_rows(rows: &[Row]) -> impl Iterator<Item = &Book> {
    rows.iter().filter_map(Row::book)
}

pub fn book_rows_mut(rows: &mut [Row]) -> impl Iterator<Item = &mut Book> {
    rows.iter_mut().filter_map(Row::as_book_mut)
}

pub fn find_row<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.id() == id)
}

pub fn find_row_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Row> {
    rows.iter_mut().find(|r| r.id() == id)
}

impl Fingerprint {
    pub fn of(size: u64, mtime_ms: u64, head: &[u8]) -> Self {
        Self {
            size,
            mtime_ms,
            head_hash: crate::hash::head_hash(head),
        }
    }

    /// Stand-in fingerprint for a book known by address only (a row migrated
    /// from the `v1` schema, which measured nothing). Derived from the address
    /// so two books never share one; `mtime_ms == 0` marks it as a placeholder.
    pub fn placeholder(path: &str) -> Self {
        let bytes = path.as_bytes();
        Self {
            size: bytes.len() as u64,
            mtime_ms: 0,
            head_hash: crate::hash::head_hash(bytes),
        }
    }
}

impl Origin {
    /// The address every other layer speaks: cover cache keys, resume-point
    /// lookups and the shell's `read_file_*` gate all use it.
    pub fn path(&self) -> &str {
        match self {
            Origin::Linked { src } => src,
            Origin::Stored { store, .. } => store,
        }
    }

    pub fn source(&self) -> Option<&str> {
        match self {
            Origin::Linked { src } => Some(src),
            Origin::Stored { src, .. } => src.as_deref(),
        }
    }

    pub fn is_stored(&self) -> bool {
        matches!(self, Origin::Stored { .. })
    }

    /// Whether this is the library's own copy of `path`. A folder's moved-out
    /// log is only honest when the survivor really is a copy of the file the
    /// dissolving row read.
    pub fn is_store_copy_of(&self, path: &str) -> bool {
        matches!(self, Origin::Stored { src: Some(src), .. } if src == path)
    }
}

impl Row {
    pub fn link(id: String, name: String, target: String, added_ms: u64) -> Self {
        Row::Link {
            id,
            name,
            target,
            added_ms,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Row::Book(b) => &b.id,
            Row::Link { id, .. } => id,
        }
    }

    /// The name the shelf shows and a collision compares: a book's title or
    /// address stem (never empty), a link's made-with name.
    pub fn display_name(&self) -> String {
        match self {
            Row::Book(b) => b.title(),
            Row::Link { name, .. } => name.clone(),
        }
    }

    pub fn is_link(&self) -> bool {
        matches!(self, Row::Link { .. })
    }

    pub fn book(&self) -> Option<&Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn as_book_mut(&mut self) -> Option<&mut Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn fp(&self) -> Option<Fingerprint> {
        match self {
            Row::Book(b) => Some(b.fp),
            Row::Link { .. } => None,
        }
    }

    pub fn target(&self) -> Option<&str> {
        match self {
            Row::Link { target, .. } => Some(target),
            Row::Book(_) => None,
        }
    }

    pub fn added_ms(&self) -> u64 {
        match self {
            Row::Book(b) => b.added_ms,
            Row::Link { added_ms, .. } => *added_ms,
        }
    }
}

impl Book {
    /// One constructor for every importing path, so a joined book always
    /// starts at page 1, with no title of its own and not missing.
    pub fn new(id: String, fp: Fingerprint, format: Format, origin: Origin, added_ms: u64) -> Self {
        Self {
            id,
            fp,
            title: None,
            author: None,
            format,
            origin,
            added_ms,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
            title_locked: false,
        }
    }

    pub fn path(&self) -> &str {
        self.origin.path()
    }

    /// The name to show: the document's title, else the file stem, else the
    /// address — never empty. A stored book falls back to its source's stem,
    /// since store bytes are always named `source.<ext>`.
    pub fn title(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), self.name_source())
    }

    /// The stem of this book's address: the name an untitled book shows and a
    /// collision compares.
    pub fn stem(&self) -> String {
        stem_of(self.name_source())
    }

    /// One spelling so [`Book::title`] and [`Book::stem`] cannot fall back to
    /// different names.
    fn name_source(&self) -> &str {
        match &self.origin {
            Origin::Stored { src: Some(src), .. } => src,
            origin => origin.path(),
        }
    }

    pub fn author(&self) -> Option<String> {
        crate::text::non_blank(self.author.as_deref()).map(str::to_string)
    }

    pub fn progress(&self) -> Option<f64> {
        if self.num_pages == 0 {
            return self.fraction.filter(|f| (0.0..=1.0).contains(f));
        }
        Some((self.page.min(self.num_pages) as f64 / self.num_pages as f64).clamp(0.0, 1.0))
    }

    /// Take a measurement of this book's own bytes as its identity. A stored
    /// row's fingerprint is the copy's, which leaves the source address free
    /// for whatever folder reads it — a departure can leave the book on its
    /// shelf and still hand the OS file back to the folder's ledger.
    pub fn adopt_measurement(&mut self, measured: Option<Fingerprint>) {
        match measured {
            Some(fp) => {
                self.fp = fp;
                self.fp_pending = false;
            }
            None => self.fp_pending = true,
        }
    }

    /// Make this row the library's own copy of the file it reads. Everything
    /// the reader put into the row travels with it; the store name is minted
    /// from the row's id rather than its title.
    pub fn become_stored(&mut self, src: &str, store: String, measured: Option<Fingerprint>) {
        if self.title.is_none() {
            self.title = Some(crate::text::display_or_stem(None, src));
        }
        self.origin = Origin::Stored {
            src: Some(src.to_string()),
            store,
        };
        self.adopt_measurement(measured);
        self.missing = false;
    }

    /// The heal every path check and folder walk goes through: the row's
    /// identity becomes the measurement, the placeholder mark goes, and the
    /// address is not missing. A `v1` placeholder never matched a measurement,
    /// so this is also what stops a rescan from adding a second copy.
    pub fn heal(&mut self, fp: Fingerprint) {
        self.fp = fp;
        self.fp_pending = false;
        self.missing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::check::{add_book, apply_check};
    use crate::book::kit::{at, check, fp, linked, private, rows};
    use crate::book::query::{find_book_mut, find_by_id};
    use crate::book::read::{ReadPoint, record_read};
    use crate::book::sanitize::sanitize;
    use reader_core::format::Format;

    #[test]
    fn a_stored_book_opens_from_the_store_and_remembers_its_source() {
        let b = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/pdf/dune_1a2b3c4d.pdf".into(),
            },
            ..linked("a", "/ignored")
        };
        assert_eq!(b.path(), "/app/Library/pdf/dune_1a2b3c4d.pdf");
        assert_eq!(b.origin.source(), Some("/downloads/dune.pdf"));
        assert!(b.origin.is_stored());
    }

    #[test]
    fn the_title_falls_back_to_the_stem_and_never_to_nothing() {
        let mut b = linked("a", "/books/Dune.pdf");
        assert_eq!(b.title(), "Dune");
        b.title = Some("  ".into());
        assert_eq!(b.title(), "Dune", "a blank title is no title");
        b.origin = Origin::Linked { src: "/".into() };
        assert_eq!(b.title(), "/", "the address is the last resort");
    }

    #[test]
    fn two_rows_of_one_file_share_its_reading_truth() {
        // A kept duplicate is a second row, not a second file: reads, checks
        // and heals reach every row at the address, or a twin's placeholder
        // would hold a folder's rescan off forever.
        let mut books = rows([
            Book {
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
            Book {
                id: "b".into(),
                title: Some("dune_1".into()),
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
        ]);
        assert!(record_read(
            &mut books,
            "/books/dune.pdf",
            Some("Dune".into()),
            None,
            ReadPoint { page: 90, num_pages: 400, fraction: None },
            700,
        )
        .is_none());
        for book in book_rows(&books) {
            assert_eq!(book.page, 90);
            assert_eq!(book.last_read_ms, 700);
        }
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(at(&books, 1).title.as_deref(), Some("dune_1"));
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2,
            "one measurement heals every row at the address"
        );
        assert!(book_rows(&books).all(|b| !b.fp_pending && b.fp == fp(20, 2, 8)));
    }

    #[test]
    fn an_import_resolves_to_a_shared_row_and_never_to_a_private_one() {
        // An import must not resolve to a private book.
        let mut books = rows([private("a", "/one/dune.pdf")]);
        let arrival = Book {
            origin: Origin::Linked { src: "/two/dune.pdf".into() },
            ..linked("new", "/two/dune.pdf")
        };
        assert_eq!(add_book(&mut books, arrival), "new", "a private row holds nothing back");
        assert_eq!(books.len(), 2);
        let again = linked("new2", "/three/dune.pdf");
        assert_eq!(add_book(&mut books, again), "new");
        assert_eq!(books.len(), 2);
    }

    #[test]
    fn a_relink_heals_a_missing_book() {
        let mut books = rows([Book {
            missing: true,
            ..linked("a", "/gone/one.pdf")
        }]);
        assert!(crate::ledger::relink(&mut books, "a", "/books/one.pdf"));
        assert!(!at(&books, 0).missing);
        assert_eq!(at(&books, 0).path(), "/books/one.pdf");
    }

    #[test]
    fn the_same_content_is_the_same_book_whatever_its_address() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        let twin = Book {
            id: "twin".into(),
            origin: Origin::Linked {
                src: "/other/one.pdf".into(),
            },
            ..linked("a", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, twin), "a");
        assert_eq!(books.len(), 1, "a second address never makes a second book");
    }

    #[test]
    fn an_import_files_in_the_order_the_walk_produced() {
        // Inserting each file at the front would reverse a whole folder.
        let mut books: Vec<Row> = Vec::new();
        for name in ["a", "b", "c", "d"] {
            let book = Book {
                fp: fp(name.as_bytes()[0] as u64, 1, 1),
                ..linked(name, &format!("/books/{name}.pdf"))
            };
            add_book(&mut books, book);
        }
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn sanitize_dedupes_by_id_and_clamps_the_resume() {
        let mut books = rows([
            Book {
                page: 0,
                ..linked("a", "/books/one.pdf")
            },
            Book {
                origin: Origin::Linked {
                    src: "/copies/one.pdf".into(),
                },
                ..linked("a", "/books/one.pdf")
            },
            // Same content under two ids is a duplicate the reader chose to keep; a fingerprint dedupe would take it back.
            Book {
                id: "dup".into(),
                title: Some("one_1".into()),
                ..linked("dup", "/books/one.pdf")
            },
            Book {
                id: "  ".into(),
                ..linked("blank", "/books/three.pdf")
            },
            Book {
                fp: fp(99, 9, 9),
                fraction: Some(2.0),
                ..linked("c", "/books/four.md")
            },
        ]);
        sanitize(&mut books);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "dup", "c"]);
        assert_eq!(at(&books, 0).page, 1, "page 0 clamps to 1");
        assert_eq!(at(&books, 1).title.as_deref(), Some("one_1"), "a duplicate keeps the name it was minted with");
        assert_eq!(at(&books, 2).fraction, None, "an impossible fraction is dropped");
    }

    #[test]
    fn a_title_the_reader_chose_survives_the_debris_rule() {
        // The title rule hunts document-supplied debris; a reader-typed name is locked.
        let mut books = rows([
            Book {
                title: Some("0321894073.pdf".into()),
                ..linked("doc", "/books/one.pdf")
            },
            Book {
                id: "ren".into(),
                title: Some("my_report_final.pdf".into()),
                title_locked: true,
                ..linked("ren", "/books/two.pdf")
            },
        ]);
        sanitize(&mut books);
        assert_eq!(
            at(&books, 0).title, None,
            "a document's download debris still goes"
        );
        assert_eq!(
            at(&books, 1).title.as_deref(),
            Some("my_report_final.pdf"),
            "the reader's own name stays, whatever it looks like"
        );
    }

    #[test]
    fn a_stored_book_is_named_by_the_file_it_came_from() {
        // Store bytes are named `source.pdf`, so the store address's stem is a layout artifact.
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            ..linked("b1", "/app/Library/items/b1/source.pdf")
        };
        assert_eq!(stored.title(), "dune");
        assert_eq!(stored.stem(), "dune");
        let orphan = Book {
            origin: Origin::Stored {
                src: None,
                store: "/app/Library/items/b2/source.pdf".into(),
            },
            ..linked("b2", "/app/Library/items/b2/source.pdf")
        };
        assert_eq!(orphan.title(), "source");
        let named = Book {
            title: Some("Dune".into()),
            ..stored
        };
        assert_eq!(named.title(), "Dune");
    }

    #[test]
    fn a_store_stem_burnt_into_a_title_is_healed_and_a_readers_own_is_not() {
        // A stored book's open-address stem is the store's own "source": a
        // burn-in the load drops.
        let mut books = rows([
            Book {
                title: Some("source".into()),
                origin: Origin::Stored {
                    src: Some("/downloads/dune.pdf".into()),
                    store: "/app/Library/items/b1/source.pdf".into(),
                },
                ..linked("b1", "/app/Library/items/b1/source.pdf")
            },
            Book {
                title: Some("source".into()),
                title_locked: true,
                origin: Origin::Stored {
                    src: Some("/downloads/foundation.pdf".into()),
                    store: "/app/Library/items/b2/source.pdf".into(),
                },
                ..linked("b2", "/app/Library/items/b2/source.pdf")
            },
        ]);
        sanitize(&mut books);
        assert_eq!(at(&books, 0).title, None, "the burn-in goes");
        assert_eq!(at(&books, 0).title(), "dune", "and the source's stem shows");
        assert_eq!(
            at(&books, 1).title.as_deref(),
            Some("source"),
            "the reader's own name stays, however plain"
        );
    }

    #[test]
    fn a_link_to_a_shelf_survives_the_row_sweep() {
        // A shelf id is never a book id; which shelves exist is the shelf sweep's question.
        let mut books = rows([linked("a", "/books/one.pdf")]);
        books.push(Row::link("l1".into(), "Books".into(), "s1".into(), 1));
        books.push(Row::link("l2".into(), "Gone".into(), "zz".into(), 1));
        sanitize(&mut books);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "l1"], "the book link at a gone book goes; the shelf link stays");
    }

    #[test]
    fn the_storage_cap_evicts_the_least_recently_read() {
        let mut books: Vec<Row> = (0..(BOOKS_CAP + 2))
            .map(|i| {
                Row::Book(Book {
                    last_read_ms: i as u64,
                    ..linked(&format!("b{i}"), &format!("/books/{i}.pdf"))
                })
            })
            .collect();
        sanitize(&mut books);
        assert_eq!(books.len(), BOOKS_CAP);
        assert!(
            book_rows(&books).all(|b| b.last_read_ms >= 2),
            "the two never-read books go first"
        );
    }

    #[test]
    fn a_book_survives_a_round_trip_through_storage() {
        let book = Book {
            id: "b1".into(),
            fp: fp(1024, 1_700_000_000_000, 0xdead_beef),
            title: Some("Dune".into()),
            author: Some("Frank Herbert".into()),
            format: Format::Pdf,
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/store/pdf/dune_b1.pdf".into(),
            },
            added_ms: 5,
            last_read_ms: 9,
            page: 12,
            num_pages: 400,
            fraction: None,
            missing: true,
            fp_pending: false,
            independent: true,
            title_locked: false,
        };
        let json = serde_json::to_string(&book).unwrap();
        assert!(json.contains("\"kind\":\"stored\""), "{json}");
        assert!(json.contains("\"format\":\"pdf\""), "{json}");
        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(back, book);
    }

    #[test]
    fn a_blob_from_before_a_field_existed_still_loads() {
        // Every field has a default, so a row from an older build loads rather than dropping the library.
        let b: Book = serde_json::from_str(
            r#"{"id":"b1","fp":{"size":1,"mtimeMs":2,"headHash":3},
                "format":"markdown","origin":{"kind":"linked","src":"/n.md"}}"#,
        )
        .unwrap();
        assert_eq!(b.page, 1);
        assert!(!b.missing);
        assert_eq!(b.title, None);
        assert_eq!(b.path(), "/n.md");
    }

    #[test]
    fn the_mutable_book_lookup_steps_over_a_link() {
        let mut list = vec![
            Row::link("l1".into(), "Dune".into(), "b1".into(), 1),
            Row::Book(linked("b1", "/a.pdf")),
        ];
        // A writer of a book's facts must not reach a link row.
        assert!(find_row_mut(&mut list, "l1").is_some());
        assert!(find_book_mut(&mut list, "l1").is_none());
        find_book_mut(&mut list, "b1").unwrap().missing = true;
        assert!(find_by_id(&list, "b1").is_some_and(|b| b.missing));
        assert!(find_book_mut(&mut list, "gone").is_none());
    }

    #[test]
    fn a_stored_book_knows_the_address_it_was_copied_from() {
        let copy = Book::new(
            "b1".into(),
            fp(1, 1, 1),
            Format::Pdf,
            Origin::Stored {
                src: Some("/src/a.pdf".into()),
                store: "/store/b1.pdf".into(),
            },
            1,
        );
        assert!(copy.origin.is_store_copy_of("/src/a.pdf"));
        assert!(!copy.origin.is_store_copy_of("/store/b1.pdf"), "the copy is not its own provenance");
        assert!(!copy.origin.is_store_copy_of("/src/other.pdf"));
        // A linked book is the address, and a copy whose source is gone has no provenance to match.
        assert!(!linked("b2", "/src/a.pdf").origin.is_store_copy_of("/src/a.pdf"));
        let orphan = Book::new(
            "b3".into(),
            fp(1, 1, 1),
            Format::Pdf,
            Origin::Stored {
                src: None,
                store: "/store/b3.pdf".into(),
            },
            1,
        );
        assert!(!orphan.origin.is_store_copy_of("/src/a.pdf"));
    }

    #[test]
    fn a_departure_keeps_the_name_the_shelf_showed() {
        // The store file is named after the row's id, so an untitled row would
        // read as "b1c2d3" the moment it left.
        let mut b = linked("b1", "/src/Dune.pdf");
        b.become_stored("/src/Dune.pdf", "/store/b1.pdf".into(), Some(fp(9, 9, 9)));
        assert_eq!(b.title.as_deref(), Some("Dune"));
        assert_eq!(
            b.origin,
            Origin::Stored {
                src: Some("/src/Dune.pdf".into()),
                store: "/store/b1.pdf".into()
            }
        );
        assert_eq!(b.fp, fp(9, 9, 9), "the copy's own measurement is the identity");
        assert!(!b.fp_pending);
        assert!(!b.missing);
        // The opened address is the copy's now and the source is provenance,
        // leaving the original fingerprint free for the folder that reads it.
        assert_eq!(b.path(), "/store/b1.pdf");
        assert_eq!(b.origin.source(), Some("/src/Dune.pdf"));
    }
}

#[cfg(test)]
mod kit;
