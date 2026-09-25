//! The application: one state tree, one message enum, one view.
//!
//! `Mareader` holds the route on screen, the window's identity and metrics,
//! the chrome's hover machine, the theme the tokens resolved to, the two
//! persisted blobs, the shelf's navigation facts, the toast slot and the
//! filesystem runs in flight. The subjects under it — the walk, the moves,
//! the sheets, the session — are the modules in this folder: each owns its
//! own types, its own methods and its own helpers.

mod copies;
mod events;
mod ghosts;
mod land;
mod landing;
mod message;
mod moves;
mod reading;
mod restore;
mod rows;
mod selection;
mod session;
mod sheets;
mod shelves;
mod update;
mod view;
mod walk;

pub use message::{ContextTarget, MenuKind, Message};
use std::collections::HashSet;
use std::path::PathBuf;

use copies::QueuedImport;
use iced::time::{Duration, Instant};
use iced::{window, Point, Size, Theme};
use library_core::blob::LibraryBlob;
use library_core::folder::FolderOpts;
use message::ContextRequest;
use reader_core::settings::Settings;
use selection::{Drag, Press};
use session::window_settings;
use message::MovedAsk;
use sheets::Sheet;
use walk::FsRun;

use crate::chrome::titlebar::Titlebar;
use crate::library::conflicts::ConflictAsk;
use crate::library::duplicate::Duplicated;
use crate::library::reveal::Reveal;
use crate::reader;
use crate::route::Route;
use crate::theme::Tokens;
use crate::ui::toast::ToastHost;

/// The shelf grid's scroll, for the reveal's own way to a cell.
pub const LIBRARY_SCROLL: &str = "library-shelf";

/// The app's own name: the window title and the bar's centre both fall back to
/// it while no document owns them.
const APP_TITLE: &str = "Mareader";

/// The light's own clock: a reveal's ring stands for this long on the
/// level the answer walked the reader to.
const FLASH_DWELL: Duration = Duration::from_millis(1600);

/// Runs the reader.
pub fn run() -> iced::Result {
    iced::application(Mareader::boot, Mareader::update, Mareader::view)
        .title(Mareader::title)
        .theme(Mareader::theme)
        .style(Mareader::style)
        .subscription(Mareader::subscription)
        .window(window_settings())
        .run()
}

/// The boot proof: build the whole app state and both surfaces, report what
/// was built, and return without opening a window.
///
/// This is what CI's build lane runs on every OS (`mareader --smoke`). It is
/// deliberately the app's own shape rather than a bespoke list: the surfaces it
/// builds are the routes the app has, so a phase that adds one without adding
/// it here fails the lane that exists to notice. Nothing here needs a display,
/// a window server, or a PDF engine — which is the point, because the lane
/// proves the app *starts* on machines that have none of them.
pub fn smoke() -> iced::Result {
    let (mut state, _boot) = Mareader::boot();
    let mut built: Vec<&str> = Vec::new();

    // Both routes, built as widget trees. Setting the route and calling the
    // same `view` the runtime calls is the whole check: a panic in layout
    // arithmetic, a broken token, a missing icon — anything the shelf or the
    // reading surface does while being built — lands here.
    for (route, name) in [(Route::Library, "shelf"), (Route::Reader, "reader")] {
        state.route = route;
        let _surface = state.view();
        built.push(name);
    }

    // The reading surface with a document under it: the ladder's `Opening`
    // state is what a reader sees the moment they open a book, and it is the
    // one state whose placeholders and waiting lines only exist between the ask
    // and the answer.
    // The document step is a struct literal rather than a door of its own:
    // the open the reader runs on is exactly this — an address, no row, and
    // the first page — and a shortcut here would be a second place to keep
    // the shape of an open.
    let effects = state.reader.update(
        reader::Message::Open(reader::Open {
            path: PathBuf::from("smoke.pdf"),
            book_id: None,
            row_title: None,
            resume: 1,
        }),
        Instant::now(),
    );
    // Nothing is written: an open that has not answered records no read, and
    // `apply_reader_effects` is what owns every write there is.
    let _ = state.apply_reader_effects(effects);
    state.route = Route::Reader;
    let _opening = state.view();
    built.push("reader (opening)");

    println!("mareader --smoke: booted; surfaces built: {}", built.join(", "));
    Ok(())
}

/// The whole application state.
pub struct Mareader {
    /// The main window's identity, resolved once at boot — every window
    /// command (drag, minimize, close) is addressed to it.
    window: Option<window::Id>,
    /// The maximize state the caption glyph swaps on.
    maximized: bool,
    /// Which surface is on screen. The document pipeline that arrives with
    /// the engines drives this exactly as the web app's URL did: a document
    /// Ready is the reader, nothing open is the shelf.
    route: Route,
    /// The titlebar's hover machine.
    titlebar: Titlebar,
    /// The tokens of the current base, and the iced theme built from them.
    tokens: Tokens,
    theme: Theme,
    /// The persisted look and its neighbours — loaded at boot, saved on
    /// every change, the same contract the web app's storage layer kept.
    settings: Settings,
    /// The persisted shelf: rows, shelves, watched folders, and the view
    /// that paints them. Loaded at boot, saved at the moment of every
    /// change.
    library: LibraryBlob,
    /// The level the shelf is standing on — a shelf's id, or the library's
    /// own spelling of "no shelf".
    shelf: String,
    /// The query narrowing the level.
    query: String,
    /// The bar's open panel, if any.
    menu: Option<MenuKind>,
    /// The add menu's confirm face: the Moved row the reader asked about,
    /// holding the panel open as a two-choice question.
    menu_confirm: Option<MovedAsk>,
    /// The paths the add menu's gone check answered missing: a restore row
    /// whose file is no longer on disk renders disabled rather than
    /// promising a landing that cannot happen.
    restore_gone: HashSet<String>,
    /// The last known pointer position in window coordinates — the anchor
    /// a menu is placed at.
    cursor: Point,
    /// The window's size — the viewport a menu is clamped into.
    viewport: Size,
    /// The card the pointer is over: the grid's hover truth.
    hovered_card: Option<String>,
    /// Whether the shelf is in the choosing mode a hold starts.
    selecting: bool,
    /// The chosen set: row ids and shelf ids on the level.
    selected: HashSet<String>,
    /// The hold in flight, if the press landed on a cell.
    press: Option<Press>,
    /// The drag in flight, if the press became one.
    drag: Option<Drag>,
    /// The crumb the pointer is over: the bar's hover truth.
    hovered_crumb: Option<String>,
    /// Whether the fold's panel is showing.
    ellipsis_open: bool,
    /// When the panel closes if the pointer stays away: the leave's grace.
    /// A leave that lands inside the panel's own box arms nothing — the
    /// bar's layers queue the panel's enter BEFORE the ellipsis's exit, so
    /// the geometry, not the message order, decides who is right.
    ellipsis_close_at: Option<Instant>,
    /// The reveal in flight: what the answer lit, and when its flash
    /// began — a second reveal of the same thing is a second reveal, by
    /// the nonce the answer bumps.
    reveal: Option<(Reveal, Instant)>,
    /// The grid's viewport height, read off the scroll's own report so the
    /// reveal's offset is the viewport's own geometry.
    shelf_viewport_h: f32,
    /// The duplicate queue's remaining entries: one at a time, because each
    /// counter name counts against the level as the last landing left it.
    dup_queue: Vec<String>,
    /// What the running batch has landed so far, for the report it ends
    /// with.
    dup_landed: Vec<Duplicated>,
    /// The tap a hold swallowed: the release after a hold still belongs to
    /// the cell's button, and this flag tells that one tap to stay quiet.
    /// Cleared at the START of the next press rather than at the release,
    /// so the order of widget and listener messages inside one event can
    /// never matter.
    tap_swallow: Option<String>,
    /// Whether the selection bar's shelf popover is open.
    select_pop: bool,
    /// Whether the last crumb is a rename field right now.
    renaming: bool,
    /// What the rename field holds, mid-typing.
    rename_draft: String,
    /// The right-click menu in flight, if any.
    context: Option<ContextRequest>,
    /// The modal question in flight, if any.
    sheet: Option<Sheet>,
    /// The name questions waiting behind the sheet's current one: two drops
    /// in flight owe two answers, and an answered sheet stays up while the
    /// queue lasts.
    conflict_waiting: Vec<ConflictAsk>,
    /// The apply-to-all switch's state, reset every time a new question
    /// takes the sheet: a batch is a promise made per question, not a mood
    /// the reader left switched on.
    apply_all: bool,
    /// The import sheet's options. They outlive the sheet: a second folder
    /// is usually imported the same way as the first.
    import_opts: FolderOpts,
    /// The app-global toast slot.
    toasts: ToastHost,
    /// The reading surface: the open document, the viewer's geometry and the
    /// engine behind them. Alive from boot, but it loads no Pdfium until the
    /// first document is opened, so a run that never reads a PDF never touches
    /// the engine at all.
    reader: reader::Reader,
    /// The filesystem runs in flight: folder walks, their store batches,
    /// and the loose-file runs. One dock pill per run.
    runs: Vec<FsRun>,
    /// An explicit import waiting for a focus walk of the same folder to
    /// release it: an ask outranks a rescan, but never races its ledger
    /// write.
    queued_ask: Option<QueuedImport>,
    /// Whether the boot-and-focus measure pass is in flight.
    verifying: bool,
    /// The id the next filesystem run wears. Progress beats carry it, so
    /// two runs' answers can never mix.
    next_task: u64,
}
