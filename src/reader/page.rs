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
//!
//! The zoom cluster is the web app's reader menu, kept where the reader's hands
//! already are: the ladder's two steps with the readout between them — the
//! percentage is the DISPLAY scale, the one under the reader's eyes, so it moves
//! while a zoom runs — and the two fit choices beside them, marked the way the
//! app marks every chosen control. Nothing here decides a scale: the bar posts
//! intents, and the pipeline resolves them (which is also how the bar knows
//! whether the ladder has anywhere to go).

use iced::widget::{button, column, container, image, row, stack, text, Space};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Shadow, Vector,
};

use reader_core::outline::active_entry;
use reader_core::zoom_math::FitMode;

use super::zoom::{self, Command};
use super::{DocStatus, Message, Reader};
use crate::chrome::icons::{IconName, icon};
use crate::chrome::titlebar;
use crate::theme::{Tokens, wash};

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
/// it belongs to, when the tree has resolved), the two page turns, the zoom
/// cluster, and the fit choices.
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
    // overflow menu to get there. The reader's own bar — sidebar, view modes,
    // search — arrives with the controls those increments add, and this bar
    // becomes its page half.
    let percent = format!("{}%", (reader.zoom.display * 100.0).round() as u32);
    let pill = container(
        row![
            bar_button(tokens, IconName::Library, true, false, Message::Close),
            bar_button(
                tokens,
                IconName::Prev,
                ready && page > 1,
                false,
                Message::Turn(-1)
            ),
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
                false,
                Message::Turn(1)
            ),
            hairline(tokens),
            bar_button(
                tokens,
                IconName::ZoomOut,
                can_step(reader, -1),
                false,
                Message::Zoom(Command::Step(-1))
            ),
            // A box of its own width, so the pill does not breathe as the
            // digits change under a zoom.
            container(text(percent).size(11).color(tokens.ink))
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(22.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
            bar_button(
                tokens,
                IconName::ZoomIn,
                can_step(reader, 1),
                false,
                Message::Zoom(Command::Step(1))
            ),
            hairline(tokens),
            bar_button(
                tokens,
                IconName::FitWidth,
                true,
                reader.viewer.fit == FitMode::Width,
                Message::Fit(FitMode::Width)
            ),
            bar_button(
                tokens,
                IconName::FitPage,
                true,
                reader.viewer.fit == FitMode::Page,
                Message::Fit(FitMode::Page)
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
        background: Some(Background::Color(wash(tokens.surface, 0.96))),
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

/// One bar button: an icon, the accent wash when it is the chosen one, a ghost
/// wash under the pointer, and nothing at all when there is nowhere to go.
fn bar_button<'a>(
    tokens: Tokens,
    name: IconName,
    enabled: bool,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        container(icon(name, 16, if selected { tokens.accent } else { tokens.ink }))
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0)),
    )
    .padding(2.0)
    .style(move |_, status| {
        if selected {
            // The app's mark for a chosen control, the same one the shelf's
            // menus put on the active row: a soft accent surface with the
            // accent's own ink on it.
            return button::Style {
                background: Some(Background::Color(tokens.accent_soft)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 6.0.into(),
                },
                text_color: tokens.accent,
                shadow: Shadow::default(),
                snap: false,
            };
        }
        titlebar::ghost_button_style(tokens, 1.0, status)
    })
    .on_press_maybe(enabled.then_some(on_press))
    .into()
}

/// The hairline between the bar's halves: the page, and the page's size. The
/// air around it is its own space rather than the line's padding, which would
/// paint the wash across the gap.
fn hairline<'a>(tokens: Tokens) -> Element<'a, Message> {
    let line = container(Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(16.0)))
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.line)),
            ..container::Style::default()
        });
    row![
        Space::new().width(Length::Fixed(5.0)),
        line,
        Space::new().width(Length::Fixed(5.0)),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// Whether the ladder has anywhere to go in `dir`.
///
/// Asked of the resolver rather than guessed: the same question the press
/// itself will ask, so a button that looks live is one that will move the page
/// — including at the ladder's ends, where a press would otherwise do nothing
/// at all.
fn can_step(reader: &Reader, dir: i32) -> bool {
    let sheet = reader.document.page_box(reader.viewer.page);
    zoom::resolve(&reader.viewer, &reader.zoom, sheet, Command::Step(dir)).is_some()
}
