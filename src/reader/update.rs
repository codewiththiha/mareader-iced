//! The reader's half of the Elm loop: the messages it answers, the effects it
//! hands back, and the element it draws.

use std::time::Instant;

use iced::{Element, Size};
use reader_core::zoom_math::FitMode;

use crate::formats::pdf::Event;
use crate::theme::Tokens;
use crate::ui::toast::Tone;
use super::page;
use super::zoom::Command;
use super::{Open, Read, Reader};

/// Everything the reader can be told.
#[derive(Debug, Clone)]
pub enum Message {
    /// An answer from the engine thread.
    Engine(Event),
    /// Read a document.
    Open(Open),
    /// Step the page: `-1` back, `1` forward. The keyboard's arrows and the
    /// bar's own buttons both speak it.
    Turn(i32),
    /// The window moved. The reading area is the window: the chrome is an
    /// overlay, so nothing is subtracted.
    Resized(Size),
    /// The display's scale factor moved.
    Scale(f64),
    /// One zoom intent, posted by whichever door the reader used — the bar's
    /// own control, the `+`/`-` keys. Resolved against the window, the mode and
    /// the sheet under the reader's eyes, and run through the one transition
    /// pipeline.
    Zoom(Command),
    /// A fit mode was chosen. Written straight to the viewer, because it is a
    /// decision rather than a scale: the scale follows it in the same frame,
    /// untweened — a click answers where it lands instead of easing.
    Fit(FitMode),
    /// One animation frame: the app's frames subscription, alive exactly as
    /// long as a transition is ([`Reader::needs_tick`]).
    Tick,
    /// The surface asked to leave for the shelf. Standing on the shelf is the
    /// app's business — the route is the app's — so the app answers this one by
    /// changing route after the reader has let the document go.
    Close,
}

/// What an update asks the app to do. The reader never touches the library
/// blob, the disk or the toast slot itself: it reports, and the app writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// A read is worth writing down as a row-recorded read: the open, and the
    /// leave. The only one of the two that may *create* the row a drop of an
    /// unknown file deserves.
    Record(Read),
    /// The reader moved within the book that is already open. Written to the
    /// rows that already answer for the address — a page turn is not a moment
    /// to add books to a library.
    Progress(Read),
    /// Tell the reader something went wrong.
    Toast(Tone, String),
}

impl Reader {
    /// The engine's answers, already wearing the surface's own message type —
    /// the app's subscription is one `.map` away from its route.
    pub fn subscription(&self) -> iced::Subscription<Message> {
        self.engine.subscription().map(Message::Engine)
    }

    /// One message, and what the app owes the world because of it.
    pub fn update(&mut self, message: Message, now: Instant) -> Vec<Effect> {
        match message {
            Message::Engine(event) => self.on_event(event),
            Message::Open(open) => self.begin_open(open),
            Message::Turn(step) => self.turn(step),
            Message::Resized(size) => {
                self.resize(size);
                Vec::new()
            }
            Message::Scale(factor) => {
                self.viewer.dpr = factor.max(0.1);
                // Every page's device box moves with the display, so the frame
                // on screen is re-rasterised on the new grid rather than scaled
                // by the compositor: a 1.5× laptop panel would otherwise read
                // text drawn for a 1× screen.
                self.request_frame();
                Vec::new()
            }
            // The doors all ask for the tween: one press is one gesture, and a
            // gesture is what the 120 ms is for. The watchers — a resize, a
            // chosen fit — come in untweened instead, because they must land in
            // the frame they were asked in.
            Message::Zoom(command) => {
                self.zoom_command(command, true);
                Vec::new()
            }
            Message::Fit(fit) => {
                self.viewer.fit = fit;
                // The window's own follow: it resolves to the fit when one is
                // active and to the reader's own scale otherwise, which is the
                // same rule a resize goes through.
                self.zoom_command(Command::Follow, false);
                Vec::new()
            }
            Message::Tick => {
                let delta = now.saturating_duration_since(self.last_tick);
                self.last_tick = now;
                if self.zoom.advance(delta.as_secs_f64() * 1000.0).committed {
                    // The transaction ended: the scale the rasters are drawn
                    // for has moved, and the page is re-asked for at it.
                    self.request_frame();
                }
                Vec::new()
            }
            Message::Close => self.close(),
        }
    }

    /// Whether the reader owes an animation frame. The app subscribes to the
    /// frames it needs and nothing else: an idle reader costs no redraws.
    pub fn needs_tick(&self) -> bool {
        self.zoom.ticking()
    }

    /// The reading surface.
    pub fn view(&self, tokens: Tokens) -> Element<'_, Message> {
        page::view(self, tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;
    use crate::reader::kit::{reader, seeded, tick};
    use crate::reader::zoom::Command;
    use iced::Size;
    use reader_core::zoom_math::FitMode;

    #[test]
    fn the_resume_page_is_clamped_to_the_book_that_opened() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 40);
        assert_eq!(reader.viewer.page, 3, "a book with three pages resumes at 3");
        let effects = reader.close();
        match effects.as_slice() {
            [Effect::Record(read)] => {
                assert_eq!((read.page, read.num_pages), (3, 3));
                assert_eq!(read.path, PathBuf::from("/books/one.pdf"));
                assert_eq!(read.title.as_deref(), Some("One"));
            }
            other => panic!("expected one record, got {other:?}"),
        }
    }

    #[test]
    fn a_turn_reports_the_new_position_and_a_dead_press_reports_nothing() {
        // The web app's progress effect wrote the library as the reader moved;
        // natively the move is the reader's report and the write is the app's.
        // A press with nowhere to go reports nothing, so a shelf the reader
        // never moved on does not reorder itself.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        match reader.turn(1).as_slice() {
            [Effect::Progress(read)] => assert_eq!(read.page, 2),
            other => panic!("expected one progress report, got {other:?}"),
        }
        // At the end of the book: no move, no write.
        reader.turn(1);
        assert!(reader.turn(1).is_empty());
        assert_eq!(reader.viewer.page, 3);
    }

    #[test]
    fn a_step_moves_the_page_at_once_and_sharpens_it_when_it_lands() {
        // The gesture's contract, and the whole reason three scales are kept:
        // the paper grows under the reader's eyes while the raster on it is the
        // one that was already crisp, and the engine is asked exactly once — at
        // the end, for the size the page actually came to rest at.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        let before = reader.zoom.committed;
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        assert!(reader.needs_tick(), "a gesture owes frames");
        assert!(
            (reader.zoom.display - before).abs() < 1e-9,
            "the tween opens where the eye already is"
        );
        // The tween's first frame: the paper grows, and the raster on it is
        // still the one drawn for the size the engine was last asked at.
        tick(&mut reader, 60.0);
        assert!(reader.zoom.display > before, "the page grew");
        assert!(
            (reader.zoom.committed - before).abs() < 1e-9,
            "and the raster is drawn for the size it was asked at"
        );
        assert!(
            reader.page_box_px().0 > reader.frame_box_px().0,
            "the paper on screen is the bigger of the two"
        );
        // The rung above a 800/612 fit width.
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 1.5).abs() < 1e-9);
        assert!((reader.zoom.display - 1.5).abs() < 1e-9);
        assert!(!reader.needs_tick());
    }

    #[test]
    fn a_hand_picked_zoom_is_not_shrunk_back_to_the_fit_ceiling() {
        // The ceiling a manual zoom resolves to is what the reader chose, not
        // fit width: a page zoomed in on keeps its scale and overflows with a
        // scroll affordance (3c's), rather than snapping back to fit.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        assert!((reader.zoom.desired - 1.5).abs() < 1e-9);
        assert_eq!(reader.viewer.fit, FitMode::None, "the gesture dropped the fit");
        // A window the fit would now answer differently: the reader's own zoom
        // is what the page keeps.
        reader.resize(Size::new(1400.0, 1000.0));
        assert!((reader.zoom.display - 1.5).abs() < 1e-9);
        assert!((reader.zoom.committed - 1.5).abs() < 1e-9, "nothing moved, so nothing renders");
    }

    #[test]
    fn choosing_a_fit_answers_in_the_frame_it_lands() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        assert_eq!(reader.viewer.fit, FitMode::None);
        // A click, not a gesture: the page is in its new size on the frame the
        // answer landed (the raster sharpens on the held commit behind it).
        reader.update(Message::Fit(FitMode::Page), Instant::now());
        assert_eq!(reader.viewer.fit, FitMode::Page);
        assert!(
            (reader.zoom.display - 1000.0 / 792.0).abs() < 1e-9,
            "the whole sheet shows: the height is the binding side"
        );
        tick(&mut reader, 200.0);
        assert!((reader.zoom.committed - 1000.0 / 792.0).abs() < 1e-9);
        assert!((reader.zoom.desired - 1000.0 / 792.0).abs() < 1e-9, "a fit owns the ceiling too");
    }

    #[test]
    fn closing_keeps_the_reader_s_own_zoom_and_drops_what_was_moving() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        reader.turn(1);
        let scale = reader.zoom.committed;
        // A gesture still in flight when the reader leaves: dropped, or it
        // would keep asking the engine for frames of a page nobody is reading.
        reader.update(Message::Zoom(Command::Step(-1)), Instant::now());
        assert!(reader.needs_tick());
        reader.close();
        assert!(!reader.needs_tick());
        assert_eq!(reader.viewer.page, 1);
        assert!(
            (reader.zoom.committed - scale).abs() < 1e-9,
            "the reader's own zoom survives the book"
        );
    }
}
