//! The ellipsis and the panel of levels it hides.

use crate::app::Message;
use crate::chrome::desktop;
use crate::chrome::icons::{IconName, icon};
use crate::library::fold::{self, FoldPlan};
use crate::theme::{Tokens, wash};
use crate::ui::buttons;
use crate::ui::popover::{self, PanelSize};

use super::crumbs::{armed, crumb_label};
use iced::widget::{Row, button, container, mouse_area, text};
use iced::{Alignment, Element, Length, Padding, Point, Size};

/// The ellipsis: the affordance standing for the levels the bar has
/// folded. A hover opens its panel one beat behind the pointer (the app's
/// own intent grace), a press opens it outright, and while a drag is live
/// it is the session's hover target — a hover, never a drop, because it
/// stands for several levels and cannot say which one the hold would
/// choose.
pub(super) fn ellipsis(tokens: Tokens, factor: f32, open: bool) -> Element<'static, Message> {
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
pub(super) const PANEL_ROW_H: f32 = 30.0;

/// The packed panel's box: the widest packed row is the panel's width and
/// every row charges its height. One arithmetic both the panel's face and
/// the app's geometry check ride, so the leave test and the drawn panel
/// can never disagree about where the panel is.
pub fn panel_size(plan: &FoldPlan, budget: f32) -> Size {
    let (counts, widths) = fold::pack_elided(plan, budget);
    panel_box(&counts, &widths)
}

/// The box a packing takes: as wide as its widest row, as tall as its rows
/// plus the popover's own padding.
fn panel_box(counts: &[usize], widths: &[f64]) -> Size {
    let wide = fold::row_widths(widths, counts).into_iter().fold(0.0, f64::max) as f32;
    let mut size = PanelSize::new(wide);
    for _ in counts {
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
    let (counts, widths) = fold::pack_elided(plan, budget);
    let size = panel_box(&counts, &widths);
    let elided = &plan.chain[..plan.split];
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

    let panel = popover::panel(tokens, row_els, size.width);
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
