//! The paginated modes: one sheet, or the two halves of a spread, in a viewport
//! that pans when a zoomed page is bigger than the window.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{container, row, scrollable};
use iced::{Alignment, Element, Length};

use reader_core::view::{self, ViewMode};
use super::sheet;
use super::super::{Message, Reader};
use crate::theme::Tokens;

/// The page under the reader's eyes, centred — and pannable, so a hand-picked
/// zoom that overflows the window has somewhere to go rather than being clipped.
pub(super) fn view<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let (width, height) = reader.page_box_px();
    let spread = reader.viewer.mode == ViewMode::Spread;
    let content: Element<'a, Message> = if spread {
        pair(reader, width, height, tokens)
    } else {
        sheet::view(width, height, reader.frame_here(reader.viewer.page), tokens)
    };
    // The viewport holds the page (or the pair) centred when it fits and pans to
    // it when it does not: the content box is at least the window, so the page
    // can never be clipped by the window's edge.
    let (box_w, box_h) = if spread {
        (width * 2.0, height)
    } else {
        (width, height)
    };
    let viewport = reader.viewer.container;
    let content = container(content)
        .width(Length::Fixed(box_w.max(viewport.width)))
        .height(Length::Fixed(box_h.max(viewport.height)))
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    scrollable(content)
        // Both axes, because either one can overflow on its own — the west
        // wing of a plate, the foot of a tall page — and the scrollbar is the
        // overlay the web app drew rather than a band that would resize the
        // page under the reader's eyes.
        .direction(Direction::Both {
            vertical: Scrollbar::hidden(),
            horizontal: Scrollbar::hidden(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The two halves of a spread, in the order a book reads them: the left page the
/// reader's own page names, the right its neighbour — absent in a book that ends
/// on an odd page, which then shows alone.
fn pair<'a>(reader: &'a Reader, width: f32, height: f32, tokens: Tokens) -> Element<'a, Message> {
    let left = view::spread_start(reader.viewer.page);
    let mut halves = row![sheet::view(
        width,
        height,
        reader.frame_here(left),
        tokens
    )]
    .spacing(0.0)
    .align_y(Alignment::Start);
    let right = left + 1;
    if right <= reader.document.num_pages {
        halves = halves.push(sheet::view(width, height, reader.frame_here(right), tokens));
    }
    halves.into()
}
