//! The shelf: the library's surface, from the level it is standing on down
//! to the cells it paints.
//!
//! The web app's library page, restated natively:
//!
//!   * one ORDER per level — the folders at this level, then the books in
//!     the level's own order (the shelf's list on a shelf, the unfiled
//!     everywhere else), sorted by the view's key, filtered by the query;
//!   * one SHAPE for the order — the grid of cards or the list of rows, and
//!     the add door that closes both;
//!   * a library with nothing in it shows the import door alone, and a
//!     level that narrows to nothing says so with one quiet line.
//!
//! The bar's slots (breadcrumb, search, menus) live in [`bar`] and
//! [`menus`]; the cells in [`card`] and [`list`].

pub mod arrange;
pub mod bar;
pub mod card;
pub mod conflicts;
pub mod departure;
pub mod drag;
pub mod duplicate;
pub mod facts;
pub mod fold;
pub mod list;
pub mod menus;
pub mod reveal;

use std::collections::HashSet;

use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow};

use library_core::blob::LibraryBlob;
use library_core::book::{book_rows, Row};
use library_core::query;
use library_core::shelf::{find, members_of, ALL_SHELF};
use library_core::sort;
use library_core::sort::SortKey;
use library_core::view::LibraryView;

use crate::app::{ContextTarget, LIBRARY_SCROLL, MenuKind, Message};
use crate::library::drag::{DragPayload, DropEffect};

use crate::chrome::icons::{icon, IconName};
use crate::chrome::desktop;
use crate::theme::{mix, Tokens};

/// What the level's cells read about a selection: the mode, and the set.
/// One value threaded through both layouts so a grid and a list cannot
/// disagree about who is dimmed, who wears the ring, and where a
/// right-click lands. Borrowed on purpose — the set outlives every frame
/// that paints it.
#[derive(Clone, Copy)]
pub struct SelectionFacts<'a> {
    pub selecting: bool,
    pub selected: &'a HashSet<String>,
    /// What the reveal lit: the cell wears the membership's own ring on the
    /// level the reader just arrived at, so the answer flashes where the
    /// eye was taken. The reveal's own light, in the sheet's own promise.
    pub lit: Option<&'a str>,
}

/// What the level's cells read about a drag in flight: the payload, so
/// every held cell can fade, and the effect a release would commit right
/// now, so the seam, the ring and the tint land on exactly the cells the
/// commit would write. Recomputed per frame by the app from one table —
/// the cells ask questions, they never answer them. Borrowed like the
/// selection's set: both outlive every frame that paints them.
#[derive(Clone, Copy)]
pub struct DragFacts<'a> {
    pub payload: Option<&'a DragPayload>,
    pub effect: Option<&'a DropEffect>,
}

impl DragFacts<'_> {
    /// A drag is in flight: the list's band sensors wear themselves, and
    /// the floor stops taking presses.
    pub fn live(&self) -> bool {
        self.payload.is_some()
    }

    pub fn holds(&self, id: &str) -> bool {
        self.payload.is_some_and(|held| held.contains(id))
    }

    pub fn inserts_before(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::insert_at) == Some((id, false))
    }

    pub fn inserts_after(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::insert_at) == Some((id, true))
    }

    pub fn sibling_before(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::sibling_at) == Some((id, false))
    }

    pub fn sibling_after(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::sibling_at) == Some((id, true))
    }

    pub fn nests_into(&self, id: &str) -> bool {
        self.effect.and_then(DropEffect::nest_into) == Some(id)
    }

    pub fn folds_with(&self, id: &str) -> bool {
        self.effect.is_some_and(|effect| match effect {
            DropEffect::CreateFolder { with_book_id } => with_book_id.as_str() == id,
            _ => false,
        })
    }
}

/// The grid's geometry, straight off grid.css: 152px tracks with a 24px
/// gutter decide how many columns a width holds, rows sit 32px apart, and
/// the level's content keeps to a 1152px column with 24px of air either
/// side.
const TRACK_MIN: f32 = 152.0;
const COL_GAP: f32 = 24.0;
const ROW_GAP: f32 = 32.0;
const CONTENT_MAX: f32 = 1152.0;
const CONTENT_PAD: f32 = 24.0;

/// The order the level paints, in the four steps the web app ran them:
/// membership first (a shelf reads its own list; the library's root reads
/// the unfiled, or everything when a search is on), then the view's sort,
/// then the query's filter.
pub fn level_rows(library: &LibraryBlob, shelf: &str, terms: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = if shelf == ALL_SHELF {
        if query::is_active(terms) {
            library.books.clone()
        } else {
            let unfiled: HashSet<String> = members_of(&library.books, &library.shelves, ALL_SHELF)
                .into_iter()
                .map(str::to_string)
                .collect();
            library
                .books
                .iter()
                .filter(|row| unfiled.contains(row.id()))
                .cloned()
                .collect()
        }
    } else {
        let members = find(&library.shelves, shelf)
            .map(|shelf| shelf.books.clone())
            .unwrap_or_default();
        sort::ordered(&library.books, &members, SortKey::Manual, true)
    };
    sort::sort_rows(&mut rows, library.view.sort, library.view.sort_asc);
    query::filter(&rows, terms)
}

/// The shelves hanging off the level the page is on, narrowed by the query
/// when one is on — the same rule both layouts draw against.
pub fn level_folders(library: &LibraryBlob, shelf: &str, terms: &str) -> Vec<library_core::shelf::Shelf> {
    let parent = (shelf != ALL_SHELF).then_some(shelf);
    library_core::shelf::children_of(&library.shelves, parent)
        .into_iter()
        .filter(|child| !query::is_active(terms) || query::matches_terms(&child.name, terms))
        .cloned()
        .collect()
}

/// How many tracks fit a width: the grid.css `minmax(9.5rem, 1fr)`
/// auto-fit rule, floored.
pub fn auto_columns(width: f32) -> usize {
    let tracks = ((width + COL_GAP) / (TRACK_MIN + COL_GAP)).floor() as i64;
    tracks.max(1) as usize
}

/// The shelf's surface: the level's body under the bar's height.
///
/// The level is computed here and handed to the layouts BY OWNERSHIP: the
/// element tree the window lays out must never borrow a vec that dies with
/// this function, so the rows and the folders move into the cards that
/// paint them. `width` is the window's live width — the grid counts its
/// tracks against it the same way the web grid's auto-fit counted against
/// its measured box.
#[allow(clippy::too_many_arguments)]
pub fn view(
    tokens: Tokens,
    library: &LibraryBlob,
    shelf: &str,
    terms: &str,
    hovered: Option<&str>,
    width: f32,
    selection: SelectionFacts<'_>,
    drag: DragFacts<'_>,
) -> Element<'static, Message> {
    let rows = level_rows(library, shelf, terms);
    let folders = level_folders(library, shelf, terms);
    let has_books = book_rows(&library.books).count() > 0;
    let has_anything = has_books || !folders.is_empty();

    let body: Element<'static, Message> = if !has_anything {
        empty_state(tokens)
    } else {
        // The quiet line: a level narrowed to nothing while the library
        // still holds books says why.
        let quiet = if rows.is_empty() && folders.is_empty() && has_books {
            Some(if query::is_active(terms) {
                "No books match this search."
            } else {
                "This shelf is empty."
            })
        } else {
            None
        };

        let inner_width = content_width(width);
        let layout: Element<'static, Message> = if library.view.is_list() {
            list::view(tokens, library, rows, folders, hovered, selection, drag)
        } else {
            grid(
                tokens,
                library,
                library.view.columns,
                rows,
                folders,
                hovered,
                inner_width,
                selection,
                drag,
            )
        };

        let mut inner: iced::widget::Column<'static, Message> = column![layout];
        if let Some(line) = quiet {
            inner = inner.push(
                container(text(line).size(13).color(tokens.muted))
                    .width(Length::Fill)
                    .padding(Padding { top: 64.0, right: 0.0, bottom: 64.0, left: 0.0 })
                    .center_x(Length::Fill),
            );
        }
        framed(inner.into())
    };

    // The level's own floor: a press on the empty ground leaves the
    // selection and closes a context menu, and a right-click on it asks
    // the level's menu. Card and row presses are captured by their own
    // widgets first, so the floor only hears the presses nothing claimed.
    let floor = mouse_area(body)
        .on_press(Message::FloorPressed)
        .on_right_press(Message::ContextMenu(ContextTarget::Level));
    container(floor)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: desktop::TITLE_BAR_H,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
}

/// The level's frame: the scroll, and the 1152px column of content centred
/// inside it with the web page's own air around it.
fn framed<'a>(inner: Element<'a, Message>) -> Element<'a, Message> {
    scrollable(
        container(
            container(inner)
                .width(Length::Fill)
                .max_width(CONTENT_MAX)
                .padding(Padding {
                    top: 32.0,
                    right: CONTENT_PAD,
                    bottom: 32.0,
                    left: CONTENT_PAD,
                }),
        )
        .width(Length::Fill)
        .center_x(Length::Fill),
    )
    .id(LIBRARY_SCROLL)
    .on_scroll(Message::ShelfViewport)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// The grid's own pair of layout facts: the track count and the cell the
/// width says — one spelling so the reveal's offset and the scaled cells
/// come out of the same numbers.
pub fn content_metrics(display_width: f32, pinned: Option<u8>) -> (usize, f32) {
    grid_metrics(content_width(display_width), pinned)
}

/// The content column's width for a window: the level's air on both sides,
/// shared by the grid and the reveal's offsets.
fn content_width(display_width: f32) -> f32 {
    (display_width.min(CONTENT_MAX) - CONTENT_PAD * 2.0).max(TRACK_MIN)
}

/// The track count and cell width for a column, by the same arithmetic at
/// every caller: the grid paints with these, and `content_metrics` hands the
/// identical pair to the reveal.
fn grid_metrics(inner_width: f32, pinned: Option<u8>) -> (usize, f32) {
    let tracks = match pinned {
        Some(count) => usize::from(count).max(1),
        None => auto_columns(inner_width),
    };
    let cell = (inner_width - COL_GAP * (tracks as f32 - 1.0)) / tracks as f32;
    (tracks, cell)
}

/// The grid: the folders at this level, then the books, then the add card —
/// drained into rows of `n` tracks. The track count is the view's columns
/// when the reader pinned them, else whatever the live width holds. The
/// rows and the folders arrive by ownership and move, cell by cell, into
/// the cards that paint them.
#[allow(clippy::too_many_arguments)]
fn grid(
    tokens: Tokens,
    library: &LibraryBlob,
    pinned: Option<u8>,
    rows: Vec<Row>,
    folders: Vec<library_core::shelf::Shelf>,
    hovered: Option<&str>,
    width: f32,
    selection: SelectionFacts<'_>,
    drag: DragFacts<'_>,
) -> Element<'static, Message> {
    let (tracks, cell) = grid_metrics(width, pinned);

    let mut cells: Vec<Element<'static, Message>> = Vec::new();
    for shelf in folders {
        let facts = facts::folder_facts(library, &shelf.id);
        let hot = hovered.is_some_and(|id| id == shelf.id.as_str());
        cells.push(card::folder_card(
            tokens, library, shelf, facts, cell, hot, selection, drag,
        ));
    }
    for entry in rows {
        match entry {
            Row::Book(book) => {
                let hot = hovered.is_some_and(|id| id == book.id.as_str());
                cells.push(card::book_card(tokens, book, cell, hot, selection, drag));
            }
            Row::Link { id, name, target, .. } => {
                let hot = hovered.is_some_and(|hovered| hovered == id.as_str());
                cells.push(card::link_card(
                    tokens, id, name, target, cell, hot, selection, drag,
                ));
            }
        }
    }
    cells.push(card::add_card(tokens, cell));

    // Drain rather than chunk: an `Element` is not `Clone`, so the rows
    // take their cells by ownership.
    let mut layout: iced::widget::Column<'static, Message> = column![];
    while !cells.is_empty() {
        let take = tracks.min(cells.len());
        let line: Vec<Element<'static, Message>> = cells.drain(..take).collect();
        layout = layout.push(iced::widget::Row::with_children(line).spacing(COL_GAP));
    }
    container(layout.spacing(ROW_GAP))
        .width(Length::Fill)
        .align_x(Alignment::Start)
        .into()
}

/// The library with nothing in it: one import door and the drop hint beside
/// it. The door opens the same add menu the grid's last cell and the list's
/// last row open.
fn empty_state(tokens: Tokens) -> Element<'static, Message> {
    let door = button(
        row![
            icon(IconName::Plus, 17, tokens.paper),
            text("Import books").size(14).color(tokens.paper),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(Padding { top: 10.0, right: 18.0, bottom: 10.0, left: 18.0 })
    .style(move |_, status| {
        let background = match status {
            button::Status::Hovered => mix(tokens.accent, tokens.ink, 0.10),
            button::Status::Pressed => mix(tokens.accent, tokens.ink, 0.18),
            _ => tokens.accent,
        };
        iced::widget::button::Style {
            background: Some(Background::Color(background)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 8.0.into() },
            text_color: tokens.paper,
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleMenu(MenuKind::Add));

    container(
        column![
            door,
            text("Or drop a document anywhere in the window").size(12).color(tokens.muted),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// The view's live column count for a window width — the report the grid
/// sends back to the view so the stepper's `+` starts from what the shelf
/// shows. Only asked while the columns are on auto.
pub fn report_fit(view: &mut LibraryView, window_width: f32) -> bool {
    if view.columns.is_some() {
        return false;
    }
    let raw = (auto_columns(content_width(window_width)) as i64).clamp(1, 255) as u8;
    let fit = LibraryView::clamped_fit(raw);
    if view.auto_fit == fit {
        return false;
    }
    view.report_auto_fit(fit);
    true
}
