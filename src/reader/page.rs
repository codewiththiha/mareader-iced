//! The reading surface: the page under the reader's eyes, and the bar that
//! moves it.
//!
//! One page at a time is what 3a paints — the four view modes share this host,
//! and the strip's own layout arrives with 3c. What the host does promise and
//! keep is the geometry contract the later modes build on: the page occupies
//! exactly the box the fit resolved, and it occupies it whether or not its
//! raster has landed (a page that grew the moment its frame arrived would make
//! every page turn a twitch).
//!
//! The web app's reader chrome was a floating bottom bar — prev/next and a
//! page readout in a glass pill — and that is what this is, minus the
//! hover-reveal machine (the same reveal the title bar runs), which arrives
//! with the controls increment. Until then the bar stands, because a bar that
//! cannot be found is worse than one that is always there.

use iced::widget::{button, column, container, image, row, stack, text, Space};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Shadow, Vector,
};

use reader_core::outline::active_entry;

use super::{DocStatus, Message, Reader};
use crate::chrome::icons::{IconName, icon};
use crate::chrome::titlebar;
use crate::theme::{Tokens, fade};

/// The surface.
pub(super) fn view(reader: &Reader, tokens: Tokens) -> Element<'_, Message> {
    let sheet = stack(vec![reading_area(reader, tokens), bottom_bar(reader, tokens)]);
    container(sheet)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.paper)),
            ..container::Style::default()
        })
        .into()
}

/// The page, centred in the reading area.
fn reading_area<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let content: Element<'a, Message> = match reader.document.status {
        DocStatus::Error => error_card(reader, tokens),
        DocStatus::Opening => column![page_card(reader, tokens), waiting_line(tokens)]
            .align_x(Alignment::Center)
            .spacing(12)
            .into(),
        _ => page_card(reader, tokens),
    };
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// The page itself: the raster when it has landed, and blank paper in the box
/// it will land in until then.
///
/// The card is white in every base mode, deliberately: a PDF page is white
/// paper, and the appearance system that tints a page (P5) tints the raster
/// rather than the card behind it. A dark base therefore reads as a white
/// sheet on a dark desk, which is what the web app looked like.
fn page_card<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let (width, height) = reader.page_box_px();
    let content: Element<'a, Message> = match reader.frame_here() {
        Some(frame) => image(frame.handle.clone())
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            // The frame is already the page at the right size: filling the box
            // rather than containing it keeps the page's edge exactly on the
            // box's edge, which is what a page turn must not shift.
            .content_fit(ContentFit::Fill)
            .into(),
        None => Space::new()
            .width(Length::Fixed(width))
            .height(Length::Fixed(height))
            .into(),
    };
    container(content)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border {
                color: tokens.line,
                width: 1.0,
                radius: 2.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.22),
                offset: Vector::new(0.0, 3.0),
                blur_radius: 16.0,
            },
            ..container::Style::default()
        })
        .into()
}

/// One muted line under the blank sheet while the engine is still reading the
/// file. The web app had a loading mark here; a sentence is the same
/// information without an animation to keep alive.
fn waiting_line(tokens: Tokens) -> Element<'static, Message> {
    text("Opening…").size(12).color(tokens.muted).into()
}

/// A document that did not open: the sentence, and the way back.
fn error_card<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let sentence = reader
        .document
        .error
        .clone()
        .unwrap_or_else(|| "This document could not be opened.".to_string());
    let name = reader.name();
    column![
        text(name).size(16).color(tokens.ink),
        container(text(sentence).size(13).color(tokens.muted))
            .max_width(460.0)
            .center_x(Length::Fill),
        button(text("Back to the shelf").size(13).color(tokens.ink))
            .padding(Padding {
                top: 8.0,
                right: 14.0,
                bottom: 8.0,
                left: 14.0,
            })
            .style(move |_, status| titlebar::ghost_button_style(tokens, 1.0, status))
            .on_press(Message::Close),
    ]
    .align_x(Alignment::Center)
    .spacing(14)
    .width(Length::Shrink)
    .into()
}

/// The floating bar: the way back, the page the reader is on (with the chapter
/// it belongs to, when the tree has resolved), and the two page turns.
fn bottom_bar<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let ready = reader.document.status.is_ready();
    let page = reader.viewer.page.max(1);
    let last = reader.document.num_pages.max(1);
    let chapter = active_entry(&reader.document.outline, page)
        .and_then(|index| reader.document.outline.get(index))
        .map(|node| node.title.clone());

    let readout: Element<'a, Message> = match chapter {
        Some(chapter) if !chapter.is_empty() => row![
            text(chapter).size(11).color(tokens.muted),
            text(format!("{page} / {last}")).size(12).color(tokens.ink),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
        _ => text(format!("{page} / {last}"))
            .size(12)
            .color(tokens.ink)
            .into(),
    };

    // The way back stands first in the pill, where the web app's reader bar
    // kept its own: a reader who wants the shelf should not have to find the
    // overflow menu to get there. The reader's own bar — sidebar, zoom, view
    // modes, search — arrives with the controls those increments add, and this
    // bar becomes its page half.
    let pill = container(
        row![
            bar_button(tokens, IconName::Library, true, Message::Close),
            bar_button(tokens, IconName::Prev, ready && page > 1, Message::Turn(-1)),
            container(readout).padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 6.0,
            }),
            bar_button(
                tokens,
                IconName::Next,
                ready && page < last,
                Message::Turn(1)
            ),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 5.0,
        right: 7.0,
        bottom: 5.0,
        left: 7.0,
    })
    .style(move |_| container::Style {
        background: Some(Background::Color(fade(tokens.surface, 0.96))),
        border: Border {
            color: tokens.line,
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    });

    container(pill)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(Alignment::End)
        .center_x(Length::Fill)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 18.0,
            left: 0.0,
        })
        .into()
}

/// One bar button: an icon, a ghost wash under the pointer, and nothing at all
/// when there is nowhere to go.
fn bar_button<'a>(
    tokens: Tokens,
    name: IconName,
    enabled: bool,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        container(icon(name, 16, tokens.ink))
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0)),
    )
    .padding(2.0)
    .style(move |_, status| titlebar::ghost_button_style(tokens, 1.0, status))
    .on_press_maybe(enabled.then_some(on_press))
    .into()
}
