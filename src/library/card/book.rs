//! The book cell: a cover frame, the info block under it, and the progress hairline.

use iced::widget::{button, column, container, text, Space, Stack};
use iced::{Alignment, Background, Border, Color, Element, Length};
use library_core::book::Book;
use library_core::text as lib_text;
use crate::app::{ContextTarget, Message};
use crate::chrome::icons::{icon, IconName};
use crate::library::{DragFacts, SelectionFacts};
use crate::theme::{wash, Tokens};
use super::kit::{
    COVER_RATIO, card_button_style, chars_per_line, check_corner, cover_style, drag_bands, elide,
    fold_ring, right_target, seam_corner, sensed, stack_layers, state_fade,
};

/// One book's card in the grid: cover, info, and the progress hairline.
/// The card OWNS its book — the level's rows are computed fresh on every
/// view, and an element may not borrow a vec that dies with the function
/// that built it.
pub fn book_card(
    tokens: Tokens,
    book: Book,
    width: f32,
    hovered: bool,
    selection: SelectionFacts,
    drag: DragFacts,
) -> Element<'static, Message> {
    let cover_h = width * COVER_RATIO;
    let title = book.title();
    let sub = book
        .author()
        .unwrap_or_else(|| lib_text::page_line(book.page, book.num_pages));
    let progress = book.progress();
    let missing = book.missing;
    let selected = selection.selected.contains(book.id.as_str());
    // The reveal's own light: the cell the sheet's answer named wears the
    // membership ring, from the ground the light already paints.
    let lit = selection.lit == Some(book.id.as_str());
    // The drag's three facts about this card, read before the id moves
    // into the right-click's answer.
    let held = drag.holds(book.id.as_str());
    let seam = drag.inserts_before(book.id.as_str());
    let fold = drag.folds_with(book.id.as_str());
    let sensor_id = book.id.clone();

    // The cover face: the title centred on the gradient, and — for a book
    // the library has lost sight of — the wash and the badge that say so.
    let title_layer = container(
        text(elide(&title, chars_per_line(width - 20.0, 13.0) * 5))
            .size(13)
            .color(wash(tokens.ink, 0.80)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(10.0)
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    // The cover's layers by depth: the title, then the missing wash and
    // badge when the library has lost sight of the book, then the
    // selection's mark — the corner pieces never overlap.
    let mut layers: Vec<Element<'static, Message>> = vec![title_layer.into()];
    if missing {
        layers.push(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .style(move |_| container::Style {
                    background: Some(Background::Color(wash(tokens.paper, 0.55))),
                    ..container::Style::default()
                })
                .into(),
        );
        layers.push(
            container(
                container(icon(IconName::Close, 11, tokens.muted))
                    .padding(4.0)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(tokens.surface)),
                        border: Border {
                            color: tokens.line,
                            width: 1.0,
                            radius: 999.0.into(),
                        },
                        ..container::Style::default()
                    }),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(6.0)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .into(),
        );
    }
    if selection.selecting {
        layers.push(check_corner(tokens, selected));
    }

    let cover = container(Stack::with_children(layers).width(Length::Fill).height(Length::Fill))
        .width(width)
        .height(cover_h)
        .style(move |_| cover_style(tokens, hovered, selected || lit));

    let info: Element<'static, Message> = column![
        text(elide(&title, chars_per_line(width, 13.6) * 2)).size(13.6).color(tokens.ink),
        text(elide(&sub, chars_per_line(width, 12.0))).size(12).color(tokens.muted),
    ]
    .spacing(2)
    .width(width)
    .into();

    let mut card = column![cover, info].spacing(8).width(width);
    if let Some(fraction) = progress
        && fraction > 0.0
    {
        card = card.push(progress_bar(tokens, width, fraction));
    }

    // One tap message for every cell: the app decides what a tap means —
    // a membership while choosing, an open otherwise — and what the hold
    // that started on this card swallowed.
    let tap_id = book.id.clone();
    let hover_id = book.id.clone();
    let right = right_target(selection, selected, ContextTarget::Row(book.id));
    let click = button(card)
        .padding(0)
        .style(move |_, status| card_button_style(tokens, status))
        .on_press(Message::CardTap(tap_id));
    let cell = sensed(click, &hover_id, right);
    // The cell's overlays by depth: the step back or the hold's fade, then
    // the seam, then the fold's ring — each a non-interactive wash, so the
    // button underneath still owns every press.
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

/// The 3px progress hairline: the line at 60% as the track, the accent as
/// the fill.
fn progress_bar(tokens: Tokens, width: f32, fraction: f64) -> Element<'static, Message> {
    let fill = (width * fraction.clamp(0.0, 1.0) as f32).max(3.0);
    container(
        container(Space::new().width(fill).height(3.0)).style(move |_| container::Style {
            background: Some(Background::Color(tokens.accent)),
            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 1.5.into() },
            ..container::Style::default()
        }),
    )
    .width(width)
    .height(3.0)
    .style(move |_| container::Style {
        background: Some(Background::Color(wash(tokens.line, 0.60))),
        border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 1.5.into() },
        ..container::Style::default()
    })
    .into()
}
