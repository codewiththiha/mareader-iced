//! The iced events the app answers: the keyboard, the mouse and the window,
//! read into messages.
use iced::{event, keyboard, mouse, window};

use crate::reader;
use super::message::Message;

/// The runtime's event firehose, narrowed to what the state tree consumes.
/// A plain `fn` — `listen_with` takes one by design, so the filter cannot
/// smuggle captured state into the subscription's identity.
pub(super) fn on_event(event: iced::Event, status: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(event) => Some(Message::WindowEvent(id, event)),
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::Cursor(Some(position)))
        }
        iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::Cursor(None)),
        // The hold machine listens to EVERY left press and release, not
        // just the ones no widget claimed: a hold starts on a card, and a
        // card's button claims the press. The machine's own guards decide
        // which presses are holds.
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::PressStarted)
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::PressEnded)
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        }) => Some(Message::EscapePressed),
        // Enter only while nothing captured it: a focused field owns the
        // key, and the shelf hears what the fields decline.
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Enter),
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            Some(if modifiers.shift() { Message::ShiftEnter } else { Message::EnterPressed })
        }
        // The page turns and the zoom ladder, and they come last on purpose:
        // `Escape` and `Enter` are matched above, and a key this arm declines
        // falls through to the same `None` every unhandled key does. A focused
        // field keeps its own keys — that is what the status guard says — so the
        // reader hears only what the fields have no use for.
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            // The zoom ladder's keys are the web app's own, and they are PLAIN
            // presses there: with a modifier the key belongs to the window's
            // shortcuts (⌘F, ⌘O, ⌘←/→) and the reader never sees it, so the
            // same guard holds here.
            let plain = !(modifiers.control() || modifiers.alt() || modifiers.logo());
            if plain {
                let zoom = match key.as_ref() {
                    keyboard::Key::Character("+") | keyboard::Key::Character("=") => Some(1),
                    keyboard::Key::Character("-") | keyboard::Key::Character("_") => Some(-1),
                    _ => None,
                };
                if let Some(dir) = zoom {
                    return Some(Message::Reader(reader::Message::Zoom(reader::Command::Step(
                        dir,
                    ))));
                }
            }
            let step = match key {
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft)
                | keyboard::Key::Named(keyboard::key::Named::PageUp) => -1,
                keyboard::Key::Named(keyboard::key::Named::ArrowRight)
                | keyboard::Key::Named(keyboard::key::Named::PageDown) => 1,
                _ => 0,
            };
            // Both directions are one message, so the surface's own turn logic
            // — the clamp at either end of the book, and the fit that follows a
            // differently sized sheet — stays the only place a turn is decided.
            (step != 0).then_some(Message::Reader(reader::Message::Turn(step)))
        }
        _ => None,
    }
}
