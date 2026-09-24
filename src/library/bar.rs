//! The shelf's titlebar slots: the breadcrumb on the left, the search pill
//! in the centre.
//!
//! The breadcrumb is the library page's only prose, and it is navigation:
//! one crumb per level, `Home` first and the open shelf last, the `Next`
//! glyph between them. The web bar folded long chains behind an ellipsis
//! and armed every crumb as a drop target, and both have arrived: the
//! fold's arithmetic is [`fold`], the ellipsis and its packed panel live
//! below, and every crumb answers the drag session while one is live.
//!
//! ## Crumbs are drop targets
//!
//! The way back to a level is also the way to file onto it from anywhere
//! in the library, and `Home`, with the library's own spelling of "no
//! shelf", takes books off the shelf they were dragged out of. The crumb
//! under the drag wears the accent UNDER it rather than around it — a
//! crumb is a line of prose in a bar, and a ring around one would be a
//! ring around a word.
//!
//! The search pill narrows the shelf where it stands: the placeholder names
//! the library's count, each keystroke filters the level, and a filled pill
//! wears its own clear button.

use iced::widget::{button, container, mouse_area, row, stack, text, text_input, Id, Row, Space};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Point, Size};

use library_core::shelf::ALL_SHELF;
use library_core::text as lib_text;

use crate::app::{MenuKind, Message};
use crate::chrome::icons::{icon, IconName};
use crate::chrome::desktop;
use crate::ui::buttons;
use crate::library::fold::{self, FoldPlan};
use crate::theme::{wash, Tokens};
use crate::ui::menu::{self as popover, PanelSize};

/// The crumb's ink at rest, weighted for the level it names.
const MEDIUM: Font = Font { weight: iced::font::Weight::Medium, ..Font::DEFAULT };

/// The rename field's identity, so the app can hand it focus the moment it
/// appears.
pub const RENAME_INPUT: Id = Id::new("shelf-rename");

/// One crumb's character budget — long shelf names cut here rather than
/// pushing the clusters apart.
fn crumb_label(name: &str) -> String {
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
fn armed<'a>(
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

/// The ellipsis: the affordance standing for the levels the bar has
/// folded. A hover opens its panel one beat behind the pointer (the app's
/// own intent grace), a press opens it outright, and while a drag is live
/// it is the session's hover target — a hover, never a drop, because it
/// stands for several levels and cannot say which one the hold would
/// choose.
fn ellipsis(tokens: Tokens, factor: f32, open: bool) -> Element<'static, Message> {
    let face = button(icon(
        IconName::More,
        14,
        wash(if open { tokens.ink } else { tokens.muted }, factor),
    ))
    .padding(Padding { top: 2.0, right: 4.0, bottom: 2.0, left: 4.0 })
    .style(move |_, status| buttons::ghost(tokens, factor, status))
    .on_press(Message::EllipsisPressed);
    mouse_area(face)
        .on_enter(Message::EllipsisHover(true))
        .on_exit(Message::EllipsisHover(false))
        .into()
}

/// One packed row's height in the panel: the crumb's own box plus the
/// row's breathing room.
const PANEL_ROW_H: f32 = 30.0;

/// The packed panel's box: the widest packed row is the panel's width and
/// every row charges its height. One arithmetic both the panel's face and
/// the app's geometry check ride, so the leave test and the drawn panel
/// can never disagree about where the panel is.
pub fn panel_size(plan: &FoldPlan, budget: f32) -> Size {
    let widths: Vec<f64> =
        plan.widths[1..=plan.split].iter().map(|width| f64::from(*width)).collect();
    let counts = fold::pack_rows(&widths, f64::from(budget));
    let wide = fold::row_widths(&widths, &counts).into_iter().fold(0.0, f64::max) as f32;
    let mut size = PanelSize::new(wide);
    for _ in &counts {
        size = size.row(PANEL_ROW_H);
    }
    size.size()
}

/// The fold's panel: the elided chain packed into the rows the window's
/// width decides, drawn the way the bar draws it — name, chevron, name —
/// so a reader who understands the breadcrumb already understands the
/// panel. Every crumb in it is a way back and a drop target, and a hover
/// anywhere inside keeps the panel open.
pub fn ellipsis_panel(
    tokens: Tokens,
    plan: &FoldPlan,
    hot: Option<&str>,
    budget: f32,
) -> (Element<'static, Message>, Size) {
    let size = panel_size(plan, budget);
    let elided = &plan.chain[..plan.split];
    let widths: Vec<f64> =
        plan.widths[1..=plan.split].iter().map(|width| f64::from(*width)).collect();
    let counts = fold::pack_rows(&widths, f64::from(budget));
    let packed = fold::split_by_counts(elided.to_vec(), &counts);
    let newest = elided.last().map(|crumb| crumb.id.clone());

    let mut row_els: Vec<Element<'static, Message>> = Vec::with_capacity(packed.len());
    for line_crumbs in packed {
        let mut line = Row::new().spacing(2).align_y(Alignment::Center);
        for crumb in &line_crumbs {
            let face = button(
                container(text(crumb_label(&crumb.name)).size(13).color(tokens.muted))
                    .max_width(128.0),
            )
            .padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 })
            .style(move |_, status| buttons::ghost(tokens, 1.0, status))
            .on_press(Message::PanelCrumb(crumb.id.clone()));
            line = line.push(armed(
                tokens,
                1.0,
                hot == Some(crumb.id.as_str()),
                face.into(),
                crumb.id.clone(),
            ));
            // The chevron is the spacing: the newest elided crumb trails
            // into the shown chain and wears none.
            if Some(crumb.id.as_str()) != newest.as_deref() {
                line = line.push(
                    container(icon(IconName::Next, 13, tokens.muted))
                        .padding(Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 2.0 }),
                );
            }
        }
        row_els.push(
            container(line)
                .width(Length::Fill)
                .padding(Padding { top: 3.0, right: 0.0, bottom: 3.0, left: 0.0 })
                .into(),
        );
    }

    let panel = popover::popover(tokens, row_els, size.width);
    let panel = mouse_area(panel)
        .on_enter(Message::EllipsisHover(true))
        .on_exit(Message::EllipsisHover(false));
    (panel.into(), size)
}

/// Where the panel hangs: under the ellipsis, which sits one Home crumb
/// and one gap into the cluster. The same estimate the fold runs on
/// places it, and the app's `place` clamps it into the viewport.
pub fn ellipsis_anchor(left_inset: f32) -> Point {
    Point::new(
        left_inset + fold::crumb_px("Home", true) + 2.0 + fold::ELLIPSIS_PX * 0.5,
        desktop::TITLE_BAR_H + 2.0,
    )
}

/// The crumb, mid-rename: the field the new name is typed into. Enter
/// commits; Escape backs out.
fn rename_field(tokens: Tokens, draft: &str, factor: f32) -> Element<'static, Message> {
    let input = text_input("Shelf name", draft)
        .id(RENAME_INPUT)
        .on_input(Message::RenameDraft)
        .on_submit(Message::CommitRename)
        .size(13)
        .width(176.0)
        .padding(Padding { top: 3.0, right: 8.0, bottom: 3.0, left: 8.0 })
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(wash(wash(tokens.paper, 0.9), factor)),
            border: Border { color: wash(tokens.accent, factor), width: 1.0, radius: 6.0.into() },
            icon: tokens.muted,
            placeholder: wash(tokens.muted, factor),
            value: wash(tokens.ink, factor),
            selection: tokens.accent_soft,
        });
    input.into()
}

/// The search pill: an always-present filter over the shelf.
pub fn search<'a>(
    tokens: Tokens,
    book_count: usize,
    query: &'a str,
    factor: f32,
) -> Element<'a, Message> {
    let placeholder = if book_count == 0 {
        "Search the library".to_string()
    } else {
        format!("Search {}", lib_text::plural(book_count, "book", "books"))
    };

    let input = text_input(&placeholder, query)
        .on_input(Message::Query)
        .size(13)
        .width(Length::Fill)
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 0.0.into() },
            icon: Color::TRANSPARENT,
            placeholder: wash(tokens.muted, factor),
            value: wash(tokens.ink, factor),
            selection: tokens.accent_soft,
        });

    let mut face = row![icon(IconName::Search, 15, wash(tokens.muted, factor)), input]
        .spacing(8)
        .align_y(Alignment::Center);
    if !query.is_empty() {
        face = face.push(
            button(icon(IconName::Close, 12, wash(tokens.muted, factor)))
                .padding(4.0)
                .style(move |_, status| buttons::ghost(tokens, factor, status))
                .on_press(Message::Query(String::new())),
        );
    }

    container(face)
        .width(Length::Fill)
        .max_width(576.0)
        .padding(Padding { top: 6.0, right: 12.0, bottom: 6.0, left: 12.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(wash(tokens.surface, 0.70), factor))),
            border: Border { color: wash(tokens.line, factor), width: 1.0, radius: 999.0.into() },
            ..container::Style::default()
        })
        .into()
}
