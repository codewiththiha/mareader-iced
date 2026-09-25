//! The iced events the app answers: the keyboard, the mouse and the window,
//! read into messages.
use iced::{event, keyboard, mouse, window};
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

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
        // A wheel the surface did not take: a horizontal strip is the one place
        // a vertical notch has nowhere to go, and the reader translates it. The
        // scroller captures the events it moves on, so this arm hears only the
        // ones it could not.
        iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { y, .. },
        }) if matches!(status, event::Status::Ignored) => {
            Some(Message::Reader(reader::Message::Wheel(-y)))
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
        // The keys the reader answers, and they come last on purpose: `Escape`
        // and `Enter` are matched above, and a key this arm declines falls
        // through to the same `None` every unhandled key does. A focused field
        // keeps its own keys — that is what the status guard says — so the
        // reader hears only what the fields have no use for.
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) if matches!(status, event::Status::Ignored) => {
            // The combos are the web app's own Cmd/Ctrl set, and the keys
            // themselves are PLAIN presses there: with a modifier the key
            // belongs to the window, and the reader never sees it.
            if modifiers.control() || modifiers.logo() {
                let chose = match key.as_ref() {
                    keyboard::Key::Character("1") => {
                        Some(reader::Message::Mode(ViewMode::Single))
                    }
                    keyboard::Key::Character("2") => {
                        Some(reader::Message::Mode(ViewMode::ScrollVertical))
                    }
                    keyboard::Key::Character("0") => {
                        Some(reader::Message::Fit(FitMode::Width))
                    }
                    _ => None,
                };
                return chose.map(Message::Reader);
            }
            if modifiers.alt() {
                return None;
            }
            // The zoom ladder's keys are the web app's own.
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
            // Navigation goes to the reader WHOLE, key and modifiers together:
            // an arrow turns a page in the paginated modes and nudges the strip
            // in the continuous ones, and which one it means is a question about
            // the mode — the reader's own, answered in one place.
            navigation_key(&key).then_some(Message::Reader(reader::Message::Key {
                key,
                shift: modifiers.shift(),
            }))
        }
        _ => None,
    }
}

/// Whether a plain keypress is one the reader's keymap has an opinion about:
/// the arrows, the two page keys, and Space.
fn navigation_key(key: &keyboard::Key) -> bool {
    matches!(
        key,
        keyboard::Key::Named(
            keyboard::key::Named::ArrowLeft
                | keyboard::key::Named::ArrowRight
                | keyboard::key::Named::ArrowUp
                | keyboard::key::Named::ArrowDown
                | keyboard::key::Named::PageUp
                | keyboard::key::Named::PageDown
                | keyboard::key::Named::Space
        )
    )
}
