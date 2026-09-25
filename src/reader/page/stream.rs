//! The continuous strip: the mounted window of pages, laid out at the offsets the
//! ported windowing answers, in a viewport that scrolls along the strip's axis.
//!
//! The surface is a spacer walk rather than an absolute canvas — iced lays a
//! column out from its head — so each page is preceded by the space between it
//! and the one before, and the walk's own extent is set to the total the
//! windowing reports, which is the number the reader's belief is clamped to.
//! Nothing off screen is built.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{column, row, scrollable, Space};
use iced::{Alignment, Element, Length, Padding};

use reader_core::view::Axis;
use super::sheet;
use super::super::{Message, Reader, SCROLL_ID};
use crate::theme::Tokens;

/// The book at the live scale, of which the mounted window is built: the pages
/// around it are the space their offsets ask for.
pub(super) fn view<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let Some(strip) = reader.strip_view() else {
        return Space::new().into();
    };
    let vertical = strip.axis() == Axis::Vertical;
    let margin = reader.viewer.margin as f32;
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    // Where the walk has reached, so each spacer is the distance the windowing
    // put between the last thing drawn and the page that follows it.
    let mut drawn = 0.0_f64;
    // The cross axis is the pages' own: the strip scrolls along one axis and the
    // other one only has travel when a page is zoomed past the window, which is
    // why the content box has to be sized for a page rather than filled.
    let mut cross = 0.0_f32;
    if let Some(window) = strip.mounted() {
        for index in window.iter() {
            let page = (index + 1) as u32;
            let (width, height) = reader.box_of(page, reader.zoom.display);
            let (start, size) = strip.span(index);
            items.push(spacer(vertical, (start - drawn) as f32));
            items.push(sheet::view(width, height, reader.frame_here(page), tokens));
            drawn = start + size;
            cross = cross.max(if vertical { width } else { height });
        }
    }
    let total = strip.total() as f32;
    items.push(spacer(vertical, total - drawn as f32));
    // The column's margin is the page's own inset; the horizontal strip's is
    // already in its offsets (two of them are the spacing), so nothing is added
    // there. Either way the inset sits on the cross axis, never on the axis the
    // strip runs along.
    let window = reader.viewer.container;
    let main = Length::Fixed(total);
    let (content, axis): (Element<'a, Message>, Axis) = if vertical {
        let cross = (cross + 2.0 * margin).max(window.width);
        (
            column(items)
                .align_x(Alignment::Center)
                .padding(Padding {
                    left: margin,
                    right: margin,
                    ..Padding::ZERO
                })
                .width(Length::Fixed(cross))
                .height(main)
                .into(),
            Axis::Vertical,
        )
    } else {
        let cross = cross.max(window.height);
        (
            row(items)
                .align_y(Alignment::Center)
                .width(main)
                .height(Length::Fixed(cross))
                .into(),
            Axis::Horizontal,
        )
    };
    scrollable(content)
        .id(SCROLL_ID)
        // Both axes scroll: the cross axis has travel whenever a page is zoomed
        // past the window, and the web app's own scroller had it for the same
        // reason (its `overflow-y: auto` computes to both).
        .direction(Direction::Both {
            vertical: Scrollbar::hidden(),
            horizontal: Scrollbar::hidden(),
        })
        // The reader's own scrolling, told from the one axis the strip runs
        // along: the other axis never decides a page.
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            Message::Scrolled(f64::from(match axis {
                Axis::Vertical => offset.y,
                Axis::Horizontal => offset.x,
            }))
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The space a page is not: the chrome between two pages, or the lead-in above
/// the first one the windowing mounts.
fn spacer<'a>(vertical: bool, along: f32) -> Element<'a, Message> {
    let along = Length::Fixed(along.max(0.0));
    if vertical {
        Space::new().height(along).into()
    } else {
        Space::new().width(along).into()
    }
}
