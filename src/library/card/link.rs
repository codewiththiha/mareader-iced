//! The link cell: a tile for a linked file the library already holds a book for.

use iced::widget::{button, column, container, text, Stack};
use iced::{Background, Border, Element, Length};
use crate::app::{ContextTarget, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::{DragFacts, SelectionFacts};
use crate::theme::{wash, Tokens};
use super::kit::{
    COVER_RATIO, card_button_style, chars_per_line, check_corner, drag_bands, elide, fold_ring,
    right_target, seam_corner, sensed, stack_layers, state_fade,
};

/// A row that points at a shelf rather than being a book: the link glyph on
/// the cover's tile. A link whose target is not a shelf is listed but
/// quiet — nothing to open until the shelves it may name exist.
#[allow(clippy::too_many_arguments)]
pub fn link_card(
    tokens: Tokens,
    id: String,
    name: String,
    target: String,
    width: f32,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let selected = selection.selected.contains(id.as_str());
    let held = drag.holds(id.as_str());
    let lit = selection.lit == Some(id.as_str());
    let seam = drag.inserts_before(id.as_str());
    let fold = drag.folds_with(id.as_str());
    let sensor_id = id.clone();
    let mut layers: Vec<Element<'static, Message>> = vec![
        container(icon(IconName::Link, 28, wash(tokens.muted, 0.90)))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
    ];
    if selection.selecting {
        layers.push(check_corner(tokens, selected));
    }
    let face = container(Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .width(width)
        .height(cover_h)
        .style(move |_| container::Style {
            background: Some(Background::Color(wash(tokens.surface, 0.60))),
            border: if selected || lit {
                Border { color: tokens.accent, width: 2.0, radius: 6.0.into() }
            } else {
                Border { color: wash(tokens.line, 0.80), width: 1.0, radius: 6.0.into() }
            },
            ..container::Style::default()
        });
    let body = column![
        face,
        text(elide(&name, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
    ]
    .spacing(8)
    .width(width);

    let hover_id = id.clone();
    let right = right_target(selection, selected, ContextTarget::Row(id.clone()));
    let action = button(body)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status));
    let action = if library_core::id::is_shelf(&target) {
        action.on_press(Message::CardTap(id))
    } else {
        // Listed but dead: no press, nothing to open.
        action
    };
    let cell = sensed(action, &hover_id, right);
    let mut dressed: Vec<Element<'static, Message>> = vec![cell];
    if let Some(fade) = state_fade(tokens, held, selection.selecting, selected, hovered) {
        dressed.push(fade);
    }
    if seam {
        dressed.push(seam_corner(tokens, cover_h));
    }
    if fold {
        dressed.push(fold_ring(tokens, cover_h));
    }
    if let Some(bands) = drag_bands(drag, &sensor_id, false) {
        dressed.push(bands);
    }
    stack_layers(dressed)
}
