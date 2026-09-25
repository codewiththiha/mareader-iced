//! The rail's composition: the ground, the header, the open book's identity, the
//! panel in the middle, the switcher at its foot — and the window all of it
//! slides in through.
//!
//! The web app opened the docked rail by animating the aside's width over a
//! fixed-width inner block, and faded the floating one. iced neither hit-tests
//! children where a parent clips them nor composites a faded subtree, so both
//! become one motion here: the rail slides in, drawn by a `Float`, the widget
//! that translates a whole subtree and maps the pointer back through it. Its
//! chrome starts below the window's band, which the title bar keeps.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, float, mouse_area, row, text, tooltip, Space};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow, Vector};

use super::{Action, INNER_W, Panel, RAIL_W};
use crate::chrome::desktop;
use crate::chrome::icons::{icon, IconName};
use crate::library::card::{chars_per_line, elide};
use crate::reader::{Message, Reader};
use crate::theme::Tokens;
use crate::ui::buttons;

/// The window's band, above everything the rail draws — the title bar is an
/// overlay on this route, so the band is left to it.
const BAND_H: f32 = desktop::TITLE_BAR_H;

/// The header row — the web header's `h-12` — and its leading inset.
const HEADER_H: f32 = 48.0;
const HEADER_LEAD: f32 = 12.0;
const HEADER_ICON: u16 = 16;

/// The identity row: the cover's frame, the gap beside it, the info glyph at the
/// row's end, and the row's own air. `pb-3` and no top padding, so the row is as
/// tall as the cover plus the line under it.
const COVER_W: f32 = 40.0;
const COVER_H: f32 = 48.0;
const IDENTITY_GAP: f32 = 12.0;
const IDENTITY_PAD: f32 = 12.0;
const INFO_SIZE: u16 = 14;
const INFO_W: f32 = INFO_SIZE as f32;
const INFO_LEAD: f32 = 8.0;
const TITLE_SIZE: f32 = 13.0;
const AUTHOR_SIZE: f32 = 12.0;

/// The switcher's tabs — the web rail's `h-9 w-14` chips — and the air around
/// them.
const TAB_W: f32 = 56.0;
const TAB_H: f32 = 36.0;
const TAB_RADIUS: f32 = 8.0;
const TAB_ICON: u16 = 16;
const TAB_GAP: f32 = 4.0;
const FOOT_PAD: f32 = 6.0;

/// The strip along the window's left edge that opens a floating rail.
const EDGE_STRIP_W: f32 = 1.5;

/// The rail as the reading area's sibling: the slot the page gives up width for,
/// empty while the rail is closed.
pub(crate) fn docked<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let slot = Length::Fixed(RAIL_W * reader.sidebar.factor());
    match window(reader, tokens) {
        Some(rail) => container(rail).width(slot).height(Length::Fill).into(),
        None => Space::new().width(slot).height(Length::Fill).into(),
    }
}

/// The rail as a layer over the reading area. Docked, the page has already given
/// up the width it takes (`Sidebar::slot`); floating, the page has kept it.
pub(crate) fn floating<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    match window(reader, tokens) {
        Some(rail) => rail,
        None => Space::new().width(Length::Fixed(0.0)).into(),
    }
}

/// The floating rail's own affordance: a 1.5px strip along the window's left
/// edge. In the tree only while a floating rail is closed — an open one covers
/// it, and a docked rail has the bar's toggle instead.
pub(crate) fn edge<'a>(reader: &'a Reader) -> Element<'a, Message> {
    let rail = &reader.sidebar;
    if rail.docks() || rail.present() {
        return Space::new().width(Length::Fixed(0.0)).into();
    }
    mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fixed(EDGE_STRIP_W))
            .height(Length::Fill),
    )
    .on_enter(Message::Sidebar(Action::Toggle))
    .into()
}

/// The bar's own way in: the button that opens the rail, where the web app kept
/// it — first in the leading cluster, and only while a docked rail is closed (a
/// floating one is reached by the edge).
pub(crate) fn toggle<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let rail = &reader.sidebar;
    if !rail.docks() || rail.present() {
        return Space::new().width(Length::Fixed(0.0)).into();
    }
    icon_button(tokens, IconName::Sidebar, HEADER_ICON, false, Action::Toggle)
}

/// The rail, sliding: `None` while none of it is on screen.
///
/// The whole subtree moves, so a half-open rail shows the part of itself that
/// fits and nothing else: the slide is the open, and the window it slides through
/// is exactly as wide as the reading area has given up.
fn window<'a>(reader: &'a Reader, tokens: Tokens) -> Option<Element<'a, Message>> {
    let factor = reader.sidebar.factor();
    if factor <= 0.0 {
        return None;
    }
    let offset = -RAIL_W * (1.0 - factor);
    Some(
        float(body(reader, tokens))
            .translate(move |_, _| Vector::new(offset, 0.0))
            .into(),
    )
}

/// Everything the rail paints, at its full width.
///
/// The pointer's own report is taken at the whole rail rather than per row: a
/// floating rail closes when the pointer leaves any of it, and a row that
/// swallowed the news would keep one open.
fn body<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let inner = column![
        Space::new().height(Length::Fixed(BAND_H)),
        header(tokens),
        identity(reader, tokens),
        rule(tokens),
        container(panel(reader, tokens))
            .width(Length::Fill)
            .height(Length::Fill),
        rule(tokens),
        switcher(reader, tokens),
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    mouse_area(
        row![
            container(inner)
                .width(Length::Fixed(INNER_W))
                .height(Length::Fill)
                .style(move |_| ground(tokens)),
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fixed(super::BORDER_W))
                .height(Length::Fill)
                .style(move |_| line(tokens)),
        ],
    )
    .on_enter(Message::Sidebar(Action::Hold(true)))
    .on_exit(Message::Sidebar(Action::Hold(false)))
    .into()
}

/// Row 1: the rail's own controls. The web header also carried the search and
/// settings doors; those arrive with their own increments.
fn header(tokens: Tokens) -> Element<'static, Message> {
    row![
        Space::new().width(Length::Fixed(HEADER_LEAD)),
        icon_button(tokens, IconName::SidebarOpen, HEADER_ICON, false, Action::Close),
    ]
    .align_y(Alignment::Center)
    .height(Length::Fixed(HEADER_H))
    .into()
}

/// Row 2: the book that is open — its cover's frame, its name and its author.
/// The rendered covers arrive with their own increment; until then a book wears
/// the frame and the glyph the web app fell back to.
fn identity<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let room = (INNER_W - 2.0 * IDENTITY_PAD - COVER_W - IDENTITY_GAP - INFO_LEAD - INFO_W)
        .max(40.0);
    let name = elide(reader.name(), chars_per_line(room, TITLE_SIZE));
    let mut words = column![
        text(name)
            .size(TITLE_SIZE)
            .wrapping(Wrapping::None)
            .width(Length::Fixed(room))
            .color(tokens.ink)
    ]
    .width(Length::Fill)
    .spacing(2);
    if let Some(author) = reader.document.author.as_deref().filter(|it| !it.is_empty()) {
        words = words.push(
            text(elide(author, chars_per_line(room, AUTHOR_SIZE)))
                .size(AUTHOR_SIZE)
                .wrapping(Wrapping::None)
                .width(Length::Fixed(room))
                .color(tokens.muted),
        );
    }
    container(
        row![
            cover(tokens),
            Space::new().width(Length::Fixed(IDENTITY_GAP)),
            words,
            Space::new().width(Length::Fixed(INFO_LEAD)),
            info(reader, tokens),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: IDENTITY_PAD,
        bottom: IDENTITY_PAD,
        left: IDENTITY_PAD,
    })
    .width(Length::Fill)
    .into()
}

/// The book's cover, before there is one to draw: the frame the web app fell
/// back to while a cover had not rendered.
fn cover(tokens: Tokens) -> Element<'static, Message> {
    container(icon(IconName::Open, 14, tokens.muted))
        .width(Length::Fixed(COVER_W))
        .height(Length::Fixed(COVER_H))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.surface)),
            border: Border {
                color: tokens.line,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// The row's own trailing glyph: where the file the book came from lives.
fn info<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    let Some(path) = reader.document.path.as_deref() else {
        return Space::new().into();
    };
    tooltip(
        icon(IconName::More, INFO_SIZE, tokens.muted),
        text(path.display().to_string()).size(AUTHOR_SIZE),
        tooltip::Position::Right,
    )
    .into()
}

/// The middle: the panel the rail is showing, or nothing while it is closing
/// from a panel it has already put away.
fn panel<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    match reader.sidebar.panel() {
        Some(Panel::Outline) => super::outline::view(reader, tokens),
        None => Space::new().into(),
    }
}

/// The foot: the panels the rail can show, the chosen one wearing the app's own
/// mark for a chosen control.
fn switcher<'a>(reader: &'a Reader, tokens: Tokens) -> Element<'a, Message> {
    container(row![tab(
        tokens,
        IconName::Outline,
        Panel::Outline,
        reader.sidebar.shows(Panel::Outline),
    )]
    .spacing(TAB_GAP))
    .padding(FOOT_PAD)
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into()
}

/// One panel toggle. Re-clicking the tab of the panel that is already showing is
/// the rail's own "take me to where I am" rather than a close — the web app's
/// rule for both tabs.
fn tab(tokens: Tokens, name: IconName, panel: Panel, showing: bool) -> Element<'static, Message> {
    let ink = if showing { tokens.accent } else { tokens.muted };
    button(
        container(icon(name, TAB_ICON, ink))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(TAB_W))
    .height(Length::Fixed(TAB_H))
    .padding(0)
    .style(move |_, status| tab_style(tokens, showing, status))
    .on_press(Message::Sidebar(if showing {
        Action::Reveal
    } else {
        Action::Open(panel)
    }))
    .into()
}

/// One of the rail's own icon buttons: the app's ghost face, the accent wash
/// while `showing`, at the bar's own size.
fn icon_button(
    tokens: Tokens,
    name: IconName,
    size: u16,
    showing: bool,
    action: Action,
) -> Element<'static, Message> {
    let ink = if showing { tokens.accent } else { tokens.ink };
    button(
        container(icon(name, size, ink))
            .center_x(Length::Fixed(22.0))
            .center_y(Length::Fixed(22.0)),
    )
    .padding(2.0)
    .style(move |_, status| {
        if showing {
            return marked(tokens, 6.0);
        }
        buttons::ghost(tokens, 1.0, status)
    })
    .on_press(Message::Sidebar(action))
    .into()
}

/// The app's mark for a chosen control: a soft accent surface with the accent's
/// own ink on it, the same one the bar and the shelf's menus wear.
fn marked(tokens: Tokens, radius: f32) -> button::Style {
    button::Style {
        background: Some(Background::Color(tokens.accent_soft)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: radius.into(),
        },
        text_color: tokens.accent,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A tab's face: marked while its panel is the one on screen, a ghost wash under
/// the pointer, and nothing else.
fn tab_style(tokens: Tokens, showing: bool, status: button::Status) -> button::Style {
    if showing {
        return marked(tokens, TAB_RADIUS);
    }
    button::Style {
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: TAB_RADIUS.into(),
        },
        ..buttons::ghost(tokens, 1.0, status)
    }
}

/// A hairline between two of the rail's rows.
fn rule(tokens: Tokens) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fixed(super::BORDER_W))
        .style(move |_| line(tokens))
        .into()
}

/// The rail's face: the surface it stands on.
fn ground(tokens: Tokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.surface)),
        ..container::Style::default()
    }
}

/// A hairline's own face — the rail's edge, and the lines between its rows.
fn line(tokens: Tokens) -> container::Style {
    container::Style {
        background: Some(Background::Color(tokens.line)),
        ..container::Style::default()
    }
}
