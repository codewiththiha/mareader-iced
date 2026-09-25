//! The floating bar: the way back, the page the reader is on with the chapter it
//! belongs to, the two page turns, and the reader menu's own clusters — the zoom
//! ladder, the four view modes, and the fit choices.
//!
//! The web app kept these in a menu behind a three-dash button; natively they
//! stand where the reader's hands already are, one pill across the foot of the
//! window, with the chosen control marked the way the app marks every one.

use iced::widget::{button, container, row, text, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow};

use reader_core::outline::active_entry;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
use super::super::zoom::{self, Command};
use super::super::{Message, Reader};
use crate::chrome::icons::{icon, IconName};
use crate::theme::{wash, Elevation, Tokens};
use crate::ui::buttons;

/// The bar.
pub(super) fn view<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
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

    // The way back stands first in the pill, where the web app's reader bar kept
    // its own: a reader who wants the shelf should not have to find an overflow
    // menu to get there.
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
            mode_button(reader, tokens, ViewMode::Single, IconName::SinglePage),
            mode_button(reader, tokens, ViewMode::Spread, IconName::DualPage),
            mode_button(
                reader,
                tokens,
                ViewMode::ScrollVertical,
                IconName::Continuous
            ),
            mode_button(
                reader,
                tokens,
                ViewMode::ScrollHorizontal,
                IconName::HScroll
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
        shadow: Elevation::Bar.shadow(),
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

/// One view mode, marked while it is the one on screen.
fn mode_button<'a>(
    reader: &Reader,
    tokens: Tokens,
    mode: ViewMode,
    name: IconName,
) -> Element<'a, Message> {
    bar_button(
        tokens,
        name,
        reader.document.status.is_ready(),
        reader.viewer.mode == mode,
        Message::Mode(mode),
    )
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
        buttons::ghost(tokens, 1.0, status)
    })
    .on_press_maybe(enabled.then_some(on_press))
    .into()
}

/// The hairline between the bar's clusters. The air around it is its own space
/// rather than the line's padding, which would paint a wash across the gap.
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
