//! The selection's bar and the panel its shelf answer opens.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::selection::SELECT_POP_W;
use crate::chrome::icons::IconName;
use crate::theme::{Elevation, Tokens, wash};
use crate::ui::menu as popover;
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow};
use iced::widget::{Column, button, container, mouse_area, row, text};
use library_core::shelf::{self, ALL_SHELF};

/// The bar's own pill: the count readout and the four answers, in the
/// ActionBar's chrome — the surface, the hairline, the float's shadow.
fn select_pill(tokens: Tokens, count: usize, pop_open: bool) -> Element<'static, Message> {
    let face = row![
        container(text(format!("{count} selected")).size(12).color(tokens.muted))
            .padding(Padding { top: 0.0, right: 8.0, bottom: 0.0, left: 0.0 }),
        pill_button(tokens, "All", false, Some(Message::SelectAll)),
        pill_button(
            tokens,
            "Add to shelf",
            pop_open,
            Some(Message::ToggleSelectPop),
        ),
        pill_button(
            tokens,
            &format!("Remove ({count})"),
            false,
            (count > 0).then_some(Message::AskRemoveSelection),
        ),
        pill_button(tokens, "Done", false, Some(Message::ClearSelection)),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    container(face)
        .padding(Padding { top: 6.0, right: 6.0, bottom: 6.0, left: 16.0 })
        .style(move |_| container::Style {
            background: Some(Background::Color(tokens.surface)),
            border: Border { color: tokens.line, width: 1.0, radius: 999.0.into() },
            shadow: Elevation::Pill.shadow(),
            ..container::Style::default()
        })
        .into()
}

/// One answer on the bar: quiet text in a round ghost, red for the
/// dangerous one, washed while open for the one holding the popover.
/// `None` for the message renders the answer disabled — listed, but not
/// quietly dropped.
fn pill_button(
    tokens: Tokens,
    label: &str,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let danger = message.as_ref().is_some_and(|message| {
        matches!(message, Message::AskRemoveSelection)
    });
    let ink = if danger { crate::theme::DANGER } else { tokens.ink };
    let action = button(text(label.to_string()).size(12).color(ink))
        .padding(Padding { top: 5.0, right: 12.0, bottom: 5.0, left: 12.0 })
        .style(move |_, status| {
            let wash_of = if danger { crate::theme::DANGER } else { tokens.accent };
            let background = match status {
                button::Status::Hovered => Some(Background::Color(wash(wash_of, 0.12))),
                button::Status::Pressed => Some(Background::Color(wash(wash_of, 0.20))),
                _ => active.then_some(Background::Color(wash(tokens.accent, 0.12))),
            };
            button::Style {
                background,
                border: Border { color: Color::TRANSPARENT, width: 0.0, radius: 999.0.into() },
                text_color: ink,
                shadow: Shadow::default(),
                snap: false,
            }
        });
    match message {
        Some(message) => action.on_press(message).into(),
        None => action.into(),
    }
}

impl Mareader {
    /// The selection's bar: the count, All, the shelf answer, Remove, and
    /// Done in a pill at the shelf's bottom-right — the ActionBar's shape,
    /// surface and hairline and fully round. The bar and its popover sit in
    /// mouse areas that capture and answer nothing, so a press on their
    /// chrome cannot fall through to the floor underneath and leave the
    /// mode the bar is for.
    pub(super) fn select_bar(&self) -> Element<'_, Message> {
        let count = self.selected.len();
        let mut face = Column::new().spacing(8).align_x(Alignment::End);
        if self.select_pop {
            face = face
                .push(mouse_area(self.select_pop_panel()).on_press(Message::KeepSelection));
        }
        face = face.push(
            mouse_area(select_pill(self.tokens, count, self.select_pop))
                .on_press(Message::KeepSelection),
        );
        container(face)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding { top: 0.0, right: 20.0, bottom: 20.0, left: 0.0 })
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .into()
    }
}
impl Mareader {
    /// The bar's "Add to shelf" panel: every shelf the WHOLE set may land
    /// on — one any chosen folder would end up inside itself on answers
    /// for none of them, and renders disabled rather than vanishing — and
    /// the door to a shelf minted for the occasion.
    fn select_pop_panel(&self) -> Element<'static, Message> {
        let (_, folder_ids) = self.split_selection();
        let mut rows: Vec<Element<'static, Message>> = Vec::new();
        rows.push(popover::section(self.tokens, "Add to shelf"));
        for shelf in &self.library.shelves {
            if shelf.id == ALL_SHELF {
                continue;
            }
            let nestable = folder_ids
                .iter()
                .all(|id| shelf::can_nest(&self.library.shelves, id, &shelf.id));
            rows.push(popover::item(
                self.tokens,
                Some(IconName::Folder),
                shelf.name.clone(),
                None,
                false,
                nestable.then(|| Message::FileSelection(shelf.id.clone())),
            ));
        }
        rows.push(popover::separator(self.tokens));
        rows.push(popover::item(
            self.tokens,
            Some(IconName::Plus),
            "New shelf",
            None,
            false,
            Some(Message::FileSelectionOnNewShelf),
        ));
        popover::popover(self.tokens, rows, SELECT_POP_W)
    }
}
