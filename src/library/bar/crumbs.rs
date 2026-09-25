//! The crumb row: one button per level, each armed for the drag it accepts.

use crate::app::{MenuKind, Message};
use crate::chrome::icons::{IconName, icon};
use crate::library::fold::FoldPlan;

use super::overflow::ellipsis;
use super::search::rename_field;
use crate::theme::{Tokens, wash};
use crate::ui::buttons;
use iced::widget::{Row, Space, button, container, mouse_area, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding};
use library_core::shelf::ALL_SHELF;

/// The crumb's ink at rest, weighted for the level it names.
const MEDIUM: Font = Font { weight: iced::font::Weight::Medium, ..Font::DEFAULT };

/// One crumb's character budget — long shelf names cut here rather than
/// pushing the clusters apart.
pub(super) fn crumb_label(name: &str) -> String {
    let count = name.chars().count();
    if count <= 24 {
        return name.to_owned();
    }
    let cut: String = name.chars().take(23).collect();
    format!("{cut}…")
}

fn crumb_button(
    tokens: Tokens,
    label: &str,
    factor: f32,
    current: bool,
    hot: bool,
    message: Message,
    hover_id: String,
) -> Element<'static, Message> {
    let color = wash(if current { tokens.ink } else { tokens.muted }, factor);
    // crumb_label owns its answer: no borrow of `label` reaches the tree.
    let mut face = text(crumb_label(label)).size(13).color(color);
    if current {
        face = face.font(MEDIUM);
    }
    let button = button(container(face).max_width(160.0))
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(move |_, status| buttons::ghost(tokens, factor, status))
        .on_press(message);
    armed(tokens, factor, hot, button.into(), hover_id)
}

/// A crumb's drag face: the button as it was, plus the enter-and-exit
/// fact the session's hot target reads, and — while this crumb IS the hot
/// target — the accent's wash under the word with the accent's own line
/// along its foot (drag.css's `crumb-drop`). The mouse_area carries no
/// press of its own, so the button underneath still owns every click.
pub(super) fn armed<'a>(
    tokens: Tokens,
    factor: f32,
    hot: bool,
    inner: Element<'a, Message>,
    hover_id: String,
) -> Element<'a, Message> {
    let cell: Element<'a, Message> = if hot {
        stack![
            container(inner).style(move |_| container::Style {
                background: Some(Background::Color(wash(wash(tokens.accent, 0.12), factor))),
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 6.0.into() },
                ..container::Style::default()
            }),
            container(
                container(Space::new().width(Length::Fill).height(2.0)).style(move |_| {
                    container::Style {
                        background: Some(Background::Color(wash(tokens.accent, factor))),
                        ..container::Style::default()
                    }
                }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::End),
        ]
        .into()
    } else {
        inner
    };
    mouse_area(cell)
        .on_enter(Message::CrumbHover(Some(hover_id)))
        .on_exit(Message::CrumbHover(None))
        .into()
}

/// What the breadcrumb reads about the drag and the fold: the crumb under
/// the drag (the session's hot target, worn with the accent under it), and
/// whether the fold's panel is open (the ellipsis keeps its woken face
/// while it is).
pub struct CrumbFacts<'a> {
    pub hot: Option<&'a str>,
    pub ellipsis_open: bool,
}

/// The breadcrumb chain for the level the shelf is standing on, folded by
/// the plan the app measured for it. The last crumb is a button for a
/// second reason: it hangs the shelf's own menu — or, mid-rename, it
/// becomes the field the new name is typed into.
pub fn breadcrumb<'a>(
    tokens: Tokens,
    plan: &FoldPlan,
    factor: f32,
    renaming: bool,
    draft: &'a str,
    facts: CrumbFacts<'_>,
) -> Element<'a, Message> {
    // A fresh element per gap: `Element` is not `Clone`, and a closure is
    // the honest spelling of "the same separator, again".
    let separator = |tokens: Tokens, factor: f32| -> Element<'a, Message> {
        container(icon(IconName::Next, 13, wash(tokens.muted, factor)))
            .padding(Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 2.0 })
            .into()
    };

    // `Home` is a target with an empty id — the library's spelling of "no
    // shelf", which makes a drop there take a book off the shelf it was
    // dragged out of.
    let mut items: Vec<Element<'a, Message>> = vec![crumb_button(
        tokens,
        "Home",
        factor,
        plan.chain.is_empty(),
        facts.hot == Some(ALL_SHELF),
        Message::Navigate(ALL_SHELF.to_string()),
        String::new(),
    )];

    // The fold hides the chain's oldest levels, never its newest: the
    // ellipsis stands for the hidden ones and opens the panel that shows
    // them.
    if plan.split > 0 {
        items.push(separator(tokens, factor));
        items.push(ellipsis(tokens, factor, facts.ellipsis_open));
    }
    let shown = &plan.chain[plan.split..];
    let last = shown.len().saturating_sub(1);
    for (at, crumb) in shown.iter().enumerate() {
        items.push(separator(tokens, factor));
        if at != last {
            items.push(crumb_button(
                tokens,
                &crumb.name,
                factor,
                false,
                facts.hot == Some(crumb.id.as_str()),
                Message::Navigate(crumb.id.clone()),
                crumb.id.clone(),
            ));
            continue;
        }
        if renaming {
            items.push(rename_field(tokens, draft, factor));
            continue;
        }
        // Still a drop target: releasing a held book here files it onto
        // the level the reader is already looking at.
        let trigger = button(
            row![
                text(crumb_label(&crumb.name))
                    .size(13)
                    .font(MEDIUM)
                    .color(wash(tokens.ink, factor)),
                icon(IconName::ChevronDown, 11, wash(tokens.muted, factor)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
        .style(move |_, status| buttons::ghost(tokens, factor, status))
        .on_press(Message::ToggleMenu(MenuKind::Shelf));
        items.push(armed(
            tokens,
            factor,
            facts.hot == Some(crumb.id.as_str()),
            trigger.into(),
            crumb.id.clone(),
        ));
    }

    Row::with_children(items).spacing(2).align_y(Alignment::Center).into()
}
