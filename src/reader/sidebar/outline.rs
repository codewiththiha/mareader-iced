//! The chapter panel: one row per outline entry, the reader's own chapter marked,
//! and the arithmetic that keeps that row inside the panel's window.
//!
//! The rows are uniform, which is what makes a reveal arithmetic rather than a
//! measurement of each row — the web app queried its DOM for the active row and
//! retried for four frames while the list rebuilt. The panel reports the two
//! things only it knows: how tall its window is, and where its list sits.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, sensor, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Pixels, Shadow};

use reader_core::outline::{OutlineNode, active_entry};

use super::Action;
use crate::library::card::{chars_per_line, elide};
use crate::reader::{Message, Reader};
use crate::theme::Tokens;

/// The outline list's own scroller: the one widget the app posts a reveal to.
pub(crate) const OUTLINE_ID: &str = "reader-outline";

/// A row's height: the web app's `min-h-7` row with `py-1` and `leading-5`.
pub(super) const ROW_H: f32 = 28.0;

/// The row's own text line — the web row's `leading-5`, and the number that keeps
/// a title from growing the row it is in.
const ROW_LINE: Pixels = Pixels(20.0);
const ROW_PAD_Y: f32 = 4.0;
/// `px-3`'s right side; the left one is the row's indent.
const ROW_PAD_R: f32 = 12.0;
/// The 2px the active row's accent lives in, reserved on every row.
const MARK_W: f32 = 2.0;
const TEXT_SIZE: f32 = 13.0;

/// The air a reveal leaves around the row it brings into view.
const REVEAL_MARGIN: f32 = 24.0;

/// The left inset of a row at `depth` — the web app's `8 + depth * 12`, capped.
///
/// The cap keeps a deep tree from eating the row: past it the tree order and the
/// marked row still say where an entry sits, and a title that has been indented
/// off the rail says nothing at all.
pub(super) fn indent(depth: u32) -> f32 {
    const BASE: f32 = 8.0;
    const STEP: f32 = 12.0;
    const CAP: f32 = 120.0;
    BASE + (depth as f32 * STEP).min(CAP)
}

/// The top edge of a row within the list.
fn row_top(index: usize) -> f32 {
    index as f32 * ROW_H
}

/// The scroll that brings a row wholly into a window `view` px tall and `offset`
/// px down the list, or `None` when it is already there: a list that scrolled on
/// every page change would drag itself out from under a reader reading it.
pub(super) fn reveal(index: usize, view: f32, offset: f32) -> Option<f32> {
    let top = row_top(index);
    let bottom = top + ROW_H;
    if top >= offset && bottom <= offset + view {
        return None;
    }
    if top < offset {
        // Above the window: back off by the margin rather than pinning the row
        // to the very edge of the panel.
        return Some((top - REVEAL_MARGIN).max(0.0));
    }
    Some((bottom + REVEAL_MARGIN - view).max(0.0))
}

/// The scroll that centres a row: the deliberate "take me to where I am", which
/// moves whether or not the row is on screen.
pub(super) fn centre(index: usize, view: f32) -> f32 {
    (row_top(index) - (view - ROW_H) / 2.0).max(0.0)
}

/// The panel.
pub(super) fn view<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let outline = &reader.document.outline;
    let active = active_entry(outline, reader.viewer.page);
    let body: Element<'a, Message> = if outline.is_empty() {
        empty(reader.document.outline_pending, tokens)
    } else {
        let rows: Vec<Element<'a, Message>> = outline
            .iter()
            .enumerate()
            .map(|(index, node)| chapter(index, node, active == Some(index), tokens))
            .collect();
        scrollable(column(rows).width(Length::Fill))
            .id(OUTLINE_ID)
            .on_scroll(|viewport| {
                Message::Sidebar(Action::Scrolled(viewport.absolute_offset().y))
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };
    // The panel is the only thing that knows how tall its window is, so it says:
    // a reveal measured against a guess would scroll to the wrong row.
    sensor(body)
        .on_resize(|size| Message::Sidebar(Action::Measured(size.height)))
        .into()
}

/// A book with nothing to show: the tree is resolved after the first page is up,
/// so an empty one is a "not yet" while the engine is still looking.
fn empty<'a>(pending: bool, tokens: Tokens) -> Element<'a, Message> {
    let sentence = if pending {
        "Resolving chapters…"
    } else {
        "No outline"
    };
    container(text(sentence).size(TEXT_SIZE).color(tokens.muted))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// One chapter.
///
/// Jumping does not close the rail: an outline is a map the reader keeps open
/// while moving around the book, and the mark only has somewhere to show if the
/// panel stays.
fn chapter(index: usize, node: &OutlineNode, active: bool, tokens: Tokens) -> Element<'static, Message> {
    let inset = indent(node.depth);
    let room = (super::INNER_W - MARK_W - inset - ROW_PAD_R).max(40.0);
    let title = elide(&node.title, chars_per_line(room, TEXT_SIZE));
    button(
        row![
            container(Space::new().width(Length::Fixed(MARK_W)).height(Length::Fill)).style(
                move |_| container::Style {
                    background: Some(Background::Color(if active {
                        tokens.accent
                    } else {
                        Color::TRANSPARENT
                    })),
                    ..container::Style::default()
                }
            ),
            Space::new().width(Length::Fixed(inset)),
            text(title)
                .size(TEXT_SIZE)
                .line_height(ROW_LINE)
                .wrapping(Wrapping::None)
                .width(Length::Fixed(room))
                .color(if active { tokens.ink } else { tokens.muted }),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: ROW_PAD_Y,
        right: ROW_PAD_R,
        bottom: ROW_PAD_Y,
        left: 0.0,
    })
    .width(Length::Fill)
    .style(move |_, status| row_style(tokens, active, status))
    .on_press(Message::Sidebar(Action::Chapter(index)))
    .into()
}

/// A row's face: the marked row on its tinted ground, the others quiet until the
/// pointer reaches them.
fn row_style(tokens: Tokens, active: bool, status: button::Status) -> button::Style {
    let ground = match (active, status) {
        (true, _) => Some(tokens.line),
        (false, button::Status::Hovered | button::Status::Pressed) => Some(tokens.line),
        _ => None,
    };
    let ink = match (active, status) {
        (true, _) => tokens.ink,
        (false, button::Status::Hovered | button::Status::Pressed) => tokens.ink,
        _ => tokens.muted,
    };
    button::Style {
        background: ground.map(Background::Color),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        text_color: ink,
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel is a `w-72` rail less its own right border.
    const VIEW: f32 = 500.0;

    #[test]
    fn the_indent_grows_with_depth_and_stops_before_it_eats_the_title() {
        assert_eq!(indent(0), 8.0);
        assert!(indent(0) < indent(1));
        assert!(indent(9) < indent(10));
        assert_eq!(indent(10), indent(100), "the indent is capped");
    }

    #[test]
    fn a_reveal_leaves_a_row_that_is_already_on_screen_where_it_is() {
        // The window shows rows 0..17 of a 500px panel, and row 17 is the last
        // of them that fits whole.
        assert_eq!(reveal(3, VIEW, 0.0), None);
        assert_eq!(reveal(16, VIEW, 0.0), None, "the last whole row on screen");
        assert!(reveal(17, VIEW, 0.0).is_some(), "a row cut by the edge comes in");
    }

    #[test]
    fn a_reveal_brings_a_row_in_from_above_and_from_below() {
        // Above the window: the row lands just inside the top margin.
        let up = reveal(2, VIEW, 200.0).expect("off screen above");
        assert!((up - (row_top(2) - REVEAL_MARGIN)).abs() < 1e-6);
        // Below it: the row lands just inside the bottom margin.
        let down = reveal(30, VIEW, 0.0).expect("off screen below");
        let bottom = row_top(30) + ROW_H;
        assert!((down - (bottom + REVEAL_MARGIN - VIEW)).abs() < 1e-6);
        assert!(down > 0.0);
    }

    #[test]
    fn a_reveal_at_the_top_never_scrolls_above_the_list() {
        assert_eq!(reveal(0, VIEW, 0.0), None, "the first row starts at the top");
        assert_eq!(reveal(0, VIEW, 40.0), Some(0.0), "and a list above it goes back");
    }

    #[test]
    fn the_centre_gesture_moves_even_when_the_row_is_visible() {
        assert_eq!(centre(0, VIEW), 0.0);
        let middle = centre(10, VIEW);
        assert!((middle - (row_top(10) - (VIEW - ROW_H) / 2.0)).abs() < 1e-6);
        assert!(middle > 0.0);
    }
}
