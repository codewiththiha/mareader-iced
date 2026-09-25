//! The departure's question: the copies a move costs, and its answers.

use crate::app::Mareader;
use crate::library::departure::CopyAsk;
use crate::ui::sheet;
use iced::Element;
use iced::widget::{Column, text};
use crate::app::message::Message;


impl Mareader {
    pub(super) fn copy_panel(&self, ask: &CopyAsk) -> Element<'_, Message> {
        let mut body = Column::new().spacing(6);
        body = body.push(text(ask.subject.clone()).size(13).color(self.tokens.muted));
        for line in &ask.lines {
            body = body.push(text(line.clone()).size(12).color(self.tokens.muted));
        }
        let mut actions =
            vec![sheet::cancel_button(self.tokens, "Cancel", Message::SheetCancel)];
        for option in &ask.options {
            actions.push(if option.primary {
                sheet::confirm_button(
                    self.tokens,
                    option.label.clone(),
                    Message::AnswerCopy(option.answer),
                    false,
                )
            } else {
                sheet::cancel_button(
                    self.tokens,
                    option.label.clone(),
                    Message::AnswerCopy(option.answer),
                )
            });
        }
        sheet::panel(self.tokens, ask.action.clone(), body.into(), actions)
    }
}
