//! The name question: a level that already holds the arriving name, the
//! answers that place it, and the queue the answers drain.

use super::{LitNote, Mareader, Sheet};
use crate::library::conflicts::{self, ConflictAsk, ShelfConflictAsk};
use crate::theme::{Tokens, wash};
use crate::ui::sheet;
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow, Task};
use iced::widget::{Column, button, container, row, text};
use library_core::conflict::Placement;
use crate::app::message::Message;

#[allow(clippy::too_many_lines)]
/// The "apply to all" row: one switch that gives every waiting question of
/// the same kind the answer being clicked.
fn apply_all_row(tokens: Tokens, waiting: usize, on: bool) -> Element<'static, Message> {
    let knob = button(
        text(if on { "On" } else { "Off" })
            .size(12)
            .color(if on { tokens.ink } else { tokens.muted }),
    )
    .padding(Padding { top: 5.0, right: 14.0, bottom: 5.0, left: 14.0 })
    .style(move |_, status| {
        let background = if on {
            tokens.accent_soft
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => wash(tokens.line, 0.45),
                _ => Color::TRANSPARENT,
            }
        };
        button::Style {
            background: Some(Background::Color(background)),
            border: Border {
                color: if on { tokens.accent } else { tokens.line },
                width: 1.0,
                radius: 999.0.into(),
            },
            text_color: if on { tokens.ink } else { tokens.muted },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .on_press(Message::ToggleApplyAll);
    container(
        row![
            container(
                text(format!("Apply to all {}", waiting + 1)).size(12).color(tokens.muted)
            )
            .width(Length::Fill),
            knob,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding(Padding { top: 8.0, right: 12.0, bottom: 8.0, left: 12.0 })
    .style(move |_| container::Style {
        border: Border { color: tokens.line, width: 1.0, radius: 10.0.into() },
        ..container::Style::default()
    })
    .into()
}

impl Mareader {
    pub(in crate::app) fn raise_conflict(&mut self, asks: Vec<ConflictAsk>) {
            if asks.is_empty() {
                return;
            }
            if self.sheet.is_some() {
                self.conflict_waiting.extend(asks);
                return;
            }
            let mut asks = asks;
            let first = asks.remove(0);
            self.conflict_waiting.extend(asks);
            self.sheet = Some(Sheet::Conflict { ask: first });
            self.apply_all = false;
        }

    pub(in crate::app) fn advance_conflict(&mut self) {
            if self.sheet.is_some() || self.conflict_waiting.is_empty() {
                return;
            }
            let ask = self.conflict_waiting.remove(0);
            self.sheet = Some(Sheet::Conflict { ask });
            self.apply_all = false;
        }

    pub(in crate::app) fn apply_placement(&mut self, ask: &ConflictAsk, choice: Placement) -> Task<Message> {
            let offers = conflicts::offers_for(&self.library.books, &self.library.folders, ask);
            if !offers.contains(&choice) {
                return Task::none();
            }
            match choice {
                Placement::Open => self.reveal_existing(&ask.existing_id),
                Placement::KeepBoth => self.as_new(ask),
                Placement::LinkOnly => self.link_to_row(ask),
                Placement::Merge => self.merge_into_row(ask),
                Placement::Replace => self.replace_row(ask),
            }
        }

    pub(super) fn conflict_panel(&self, ask: &ConflictAsk) -> Element<'_, Message> {
        let spec = conflicts::describe(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            ask,
            &self.conflict_waiting,
        );
        let apply_all = self.apply_all;
        let choices = spec
            .choices
            .iter()
            .map(|choice| {
                sheet::choice_row(
                    self.tokens,
                    choice.label,
                    choice.note.clone(),
                    Message::AnswerPlacement(choice.placement, apply_all),
                )
            })
            .collect();
        let mut body = Column::new().spacing(10);
        body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
        body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
        body = body.push(sheet::choice_group(self.tokens, choices));
        // Rendered only when questions are actually waiting — a
        // switch offering to answer nothing is a control that lies
        // about its reach.
        if spec.apply_all && spec.waiting > 0 {
            body = body.push(apply_all_row(self.tokens, spec.waiting, apply_all));
        }
        sheet::panel_sized(
            self.tokens,
            sheet::CONFLICT_W,
            spec.heading.clone(),
            body.into(),
            vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
        )
    }

    pub(super) fn shelf_conflict_panel(&self, ask: &ShelfConflictAsk) -> Element<'_, Message> {
        // Asked before the walk rather than after it: the answers
        // name a run rather than a placement, so the sheet renders
        // the same chrome and routes its rows down a second lane.
        let spec = conflicts::describe_shelf(
            &self.library.books,
            &self.library.shelves,
            &self.library.folders,
            ask,
        );
        let choices = spec
            .choices
            .iter()
            .map(|choice| {
                sheet::choice_row(
                    self.tokens,
                    choice.label,
                    choice.note.clone(),
                    Message::AnswerShelf(choice.placement),
                )
            })
            .collect();
        let mut body = Column::new().spacing(10);
        body = body.push(text(spec.subtitle.clone()).size(12).color(self.tokens.muted));
        body = body.push(text(spec.question.clone()).size(12).color(self.tokens.muted));
        body = body.push(sheet::choice_group(self.tokens, choices));
        sheet::panel_sized(
            self.tokens,
            sheet::CONFLICT_W,
            spec.heading.clone(),
            body.into(),
            vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)],
        )
    }

    pub(super) fn note_panel(&self, note: &LitNote) -> Element<'_, Message> {
        // Not a question — an answer: the shelf is named why, and
        // closing is the label on the button.
        let sentence = conflicts::note_sentence(note.kind, &note.name);
        let mut body = Column::new().spacing(10);
        body = body.push(
            text(note.kind.sublabel().to_string()).size(12).color(self.tokens.muted),
        );
        body = body.push(text(sentence).size(12).color(self.tokens.muted));
        sheet::panel_sized(
            self.tokens,
            sheet::CONFLICT_W,
            note.name.clone(),
            body.into(),
            vec![sheet::confirm_button(
                self.tokens,
                "Show the shelf",
                Message::CloseAlreadyImported,
                false,
            )],
        )
    }
}
