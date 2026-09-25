//! The reader's half of the Elm loop: the messages it answers, the effects it
//! hands back, and the element it draws.

use std::time::Instant;

use iced::keyboard::Key;
use iced::{Element, Size};

use reader_core::view::{Axis, ViewMode};
use reader_core::zoom_math::FitMode;

use crate::formats::pdf::Event;
use crate::theme::Tokens;
use crate::ui::toast::Tone;
use super::keys::{self, Nav};
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
    /// Step the page: `-1` back, `1` forward. The bar's own buttons and a
    /// keypress in a paginated mode both speak it.
    Turn(i32),
    /// A navigation key, as the reader's own keymap reads it: the same arrows
    /// turn pages in the paginated modes and nudge the strip in the continuous
    /// ones, so which one a key means is the reader's question and not the
    /// window's.
    Key { key: Key, shift: bool },
    /// The strip's own report: where the surface it draws came to rest along the
    /// axis the mode scrolls.
    Scrolled(f64),
    /// A wheel notch the surface did not take, in lines with down positive —
    /// the convention every other scrolling surface answers to.
    Wheel(f32),
    /// The view mode was chosen — one page, a spread, the continuous column, the
    /// horizontal strip.
    Mode(ViewMode),
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
    /// long as something is moving ([`Reader::needs_tick`]).
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
    /// Put the continuous strip where the reader asked for it. The surface is a
    /// widget the reader owns the name of, and only the app can post a command
    /// to a widget, so the position travels out as one of these.
    Scroll {
        /// The strip's own axis: the one it moves along.
        axis: Axis,
        offset: f64,
    },
}

impl Reader {
    /// The engine's answers, already wearing the surface's own message type —
    /// the app's subscription is one `.map` away from its route.
    pub fn subscription(&self) -> iced::Subscription<Message> {
        self.engine.subscription().map(Message::Engine)
    }

    /// One message, and what the app owes the world because of it.
    pub fn update(&mut self, message: Message, now: Instant) -> Vec<Effect> {
        let mut effects = match message {
            Message::Engine(event) => self.on_event(event),
            Message::Open(open) => self.begin_open(open),
            Message::Turn(step) => self.turn(step),
            Message::Key { key, shift } => self.nav(key, shift),
            Message::Scrolled(along) => self.scrolled(along),
            Message::Wheel(lines) => self.wheel(lines),
            Message::Mode(mode) => {
                self.set_mode(mode);
                Vec::new()
            }
            Message::Resized(size) => {
                self.resize(size);
                Vec::new()
            }
            Message::Scale(factor) => {
                self.viewer.dpr = factor.max(0.1);
                // Every page's device box moves with the display, so the rasters
                // are re-asked on the new grid rather than scaled by the
                // compositor: a 1.5× laptop panel would otherwise read text
                // drawn for a 1× screen.
                self.pump_frames();
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
            Message::Tick => self.on_tick(now),
            Message::Close => self.close(),
        };
        // The strip is reconciled after EVERY message rather than from each arm:
        // a mode, a fit, a scale and a window all move it, and the geometry
        // compares its own inputs, so a message that moved none of them costs
        // one comparison.
        effects.extend(self.reflow());
        effects
    }

    /// One animation frame: the moves that are running, in the order they depend
    /// on each other.
    fn on_tick(&mut self, now: Instant) -> Vec<Effect> {
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f64() * 1000.0;
        self.last_tick = now;
        // A cover that waited out its grace lifts here whatever the rasters did:
        // a render that never arrives must not strand the reader on paper.
        self.cover_expired(now);
        let mut effects = self.glide_effect(delta);
        let advanced = self.zoom.advance(delta);
        if advanced.moved {
            // The transaction moved the scale the pages are drawn at, so the
            // strip follows it on the same frame — the web app's actuator, which
            // was the only owner of layout rescaling and ran on every frame of a
            // zoom.
            effects.extend(self.reflow());
        }
        // The aim is posted again while it has posts left: a fresh surface has
        // no widget to receive the first of them, and a rescaled one clamps the
        // post against the bounds it had a frame earlier.
        effects.extend(self.anchor_effect());
        if advanced.committed {
            // The transaction ended: the scale the rasters are drawn for has
            // moved, and a turn that landed inside the transaction takes its
            // jump now.
            self.pump_frames();
            if self.held.take() {
                effects.extend(self.jump_to(self.viewer.page));
            }
        }
        // A scroll-driven page change is reported once the scroll has gone
        // quiet: the app writes the whole library on a report, and a fling
        // crosses pages far faster than it should be writing one.
        if self.progress.owes(self.viewer.page) && self.progress.settled() {
            effects.extend(self.report_progress());
        }
        effects
    }

    /// One navigation key, resolved against the mode it landed in.
    fn nav(&mut self, key: Key, shift: bool) -> Vec<Effect> {
        match keys::resolve(&key, shift, self.viewer.mode) {
            Some(Nav::PagePrev) => self.turn(-1),
            Some(Nav::PageNext) => self.turn(1),
            Some(Nav::Line(dir)) => {
                let step = keys::line_step(self.strip_viewport());
                self.scroll_by(f64::from(dir) * step)
            }
            Some(Nav::PageStep(dir)) => {
                let step = keys::page_step(self.strip_viewport());
                self.scroll_by(f64::from(dir) * step)
            }
            None => Vec::new(),
        }
    }

    /// Whether the reader owes an animation frame. The app subscribes to the
    /// frames it needs and nothing else: an idle reader costs no redraws.
    pub fn needs_tick(&self) -> bool {
        self.zoom.ticking()
            || self.glide.is_some()
            || self.anchor.as_ref().is_some_and(|anchor| !anchor.spent())
            || self.progress.owes(self.viewer.page)
            || self.cover_up()
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
    use crate::reader::kit::{reader, seeded, tick};
    use iced::Size;
    use std::time::Instant;

    #[test]
    fn the_resume_page_is_clamped_to_the_book_that_opened() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.viewer.mode = ViewMode::Single;
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
        reader.viewer.mode = ViewMode::Single;
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
        reader.viewer.mode = ViewMode::Single;
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
            reader.page_box_px().0 > reader.box_of(reader.viewer.page, reader.zoom.committed).0,
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
        // fit width: a page zoomed in on keeps its scale and overflows into the
        // scroll affordance, rather than snapping back to fit.
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.viewer.mode = ViewMode::Single;
        seeded(&mut reader, 3, 1);
        reader.update(Message::Zoom(Command::Step(1)), Instant::now());
        tick(&mut reader, 200.0);
        assert!((reader.zoom.desired - 1.5).abs() < 1e-9);
        assert_eq!(reader.viewer.fit, FitMode::None, "the gesture dropped the fit");
        // A window the fit would now answer differently: the reader's own zoom
        // is what the page keeps.
        reader.update(Message::Resized(Size::new(1400.0, 1000.0)), Instant::now());
        assert!((reader.zoom.display - 1.5).abs() < 1e-9);
        assert!((reader.zoom.committed - 1.5).abs() < 1e-9, "nothing moved, so nothing renders");
    }

    #[test]
    fn choosing_a_fit_answers_in_the_frame_it_lands() {
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.viewer.mode = ViewMode::Single;
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
        reader.viewer.mode = ViewMode::Single;
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

    #[test]
    fn a_key_is_read_by_the_mode_it_lands_in() {
        // The window forwards navigation keys; what each one means is the
        // reader's own question, and the answer depends on the mode the key
        // lands in — the web app's keymap, kept whole.
        use iced::keyboard::{Key, key};
        let mut reader = reader();
        reader.viewer.container = Size::new(800.0, 1000.0);
        reader.viewer.mode = ViewMode::Single;
        seeded(&mut reader, 3, 1);
        let down = Key::Named(key::Named::ArrowDown);
        reader.update(
            Message::Key {
                key: down.clone(),
                shift: false,
            },
            Instant::now(),
        );
        assert_eq!(reader.viewer.page, 2, "an arrow turns a page in a paginated mode");
        // In the column the same key nudges the strip instead, and the page
        // only follows the scroll once it has stopped.
        reader.update(Message::Mode(ViewMode::ScrollVertical), Instant::now());
        let effects = reader.update(
            Message::Key {
                key: down,
                shift: false,
            },
            Instant::now(),
        );
        assert_eq!(reader.viewer.page, 2, "a nudge is not a page turn");
        assert!(matches!(effects.as_slice(), [Effect::Scroll { .. }]));
    }
}
