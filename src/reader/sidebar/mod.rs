//! The reader's rail: the panel it shows, the book it belongs to, and the switch
//! at its foot — the web app's sidebar family with its two DOM-shaped pieces
//! replaced. The open and the close are a motion the reader's own frame clock
//! steps (`slide`), and a reveal is arithmetic over uniform rows rather than a
//! query of the document with a retry chain (`outline`).
//!
//! One distinction survives from the web app's mode-plus-`collapsing` pair: a
//! close keeps its panel painted until the motion is over, so a column of
//! chapters does not pop out of a rail that is still on screen.

mod outline;
mod rail;
mod slide;
#[cfg(test)]
mod tests;

pub(crate) use outline::OUTLINE_ID;
pub(super) use rail::{docked, edge, floating, toggle};

use reader_core::outline::active_entry;
use reader_core::settings::Settings;

use slide::{HOVER_GRACE_MS, Slide};

use super::{Effect, Reader};

/// The rail's full width — the web app's `w-72` — and the hairline down its
/// right edge. The reading area gives up the first; the rail's rows are laid out
/// in what is left of the second.
pub(super) const RAIL_W: f32 = 288.0;
pub(super) const BORDER_W: f32 = 1.0;
pub(super) const INNER_W: f32 = RAIL_W - BORDER_W;

/// Which panel the rail shows. The thumbnails arrive with their own increment;
/// the machine already speaks a panel rather than a flag, so they are an arm here
/// and a tab in the rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// The document's chapter tree.
    Outline,
}

/// Everything the rail can be told.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// The bar's toggle, and a floating rail's edge: close the rail, or open the
    /// panel the last close left behind.
    Toggle,
    /// Show `panel`.
    Open(Panel),
    /// Put the rail away. Its panel stays painted until the motion is over.
    Close,
    /// The pointer entered or left the rail's own box — a floating rail closes a
    /// short grace after it is left.
    Hold(bool),
    /// The switcher's tab for the panel already showing: take me to where I am.
    Reveal,
    /// The panel reported how tall it is, in px.
    Measured(f32),
    /// The panel reported where its list sits, in px from its top.
    Scrolled(f32),
    /// A chapter was chosen: an index into the document's outline.
    Chapter(usize),
}

/// A scroll the panel is owed, and whether it has had a frame to be built in.
/// The first frame after an aim is set only marks it: a panel being built this
/// frame has no scroller to command yet.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Aim {
    offset: f32,
    built: bool,
}

impl Aim {
    fn new(offset: f32) -> Self {
        Self {
            offset,
            built: false,
        }
    }
}

/// The rail's whole state.
pub(super) struct Sidebar {
    /// The panel on screen, or `None` while the rail is closed.
    mode: Option<Panel>,
    /// The panel the last close left behind: what a reopen restores.
    last: Panel,
    /// The window the rail slides through: 0 closed, 1 open.
    slide: Slide,
    /// Where the panel's list sits and how tall the panel is — both reported by
    /// the panel, the only thing that knows.
    offset: f32,
    view: f32,
    /// The scroll the panel owes, if one is armed.
    aim: Option<Aim>,
    /// The page a reveal last answered, so a reader who has not moved on is not
    /// scrolled at twice.
    seen: Option<u32>,
    /// What is left of a floating rail's grace, in ms, while the pointer is off
    /// it.
    grace: Option<f64>,
    /// Whether the rail lies over the page — the settings' layout choice.
    overlay: bool,
    /// Whether the open and the close animate — the settings' motion switch.
    animates: bool,
}

impl Sidebar {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            // Nothing is open until the reader asks; the panel a first open lands
            // on is the one the foot offers.
            mode: None,
            last: Panel::Outline,
            slide: Slide::closed(),
            offset: 0.0,
            view: 0.0,
            aim: None,
            seen: None,
            grace: None,
            overlay: settings.layout.sidebar_overlay,
            animates: settings.animations.enabled && settings.animations.sidebar_slide,
        }
    }

    /// How much of the rail is on screen: 0 closed, 1 fully open.
    pub(super) fn factor(&self) -> f32 {
        self.slide.factor()
    }

    /// Whether the rail is painted at all: open, or a close still running.
    pub(super) fn present(&self) -> bool {
        self.mode.is_some() || self.slide.moving()
    }

    /// Whether the rail takes the page's width. A floating rail lies over the
    /// page and takes none.
    pub(super) fn docks(&self) -> bool {
        !self.overlay
    }

    /// The width a docked rail takes from the reading area.
    pub(super) fn slot(&self) -> f32 {
        if self.docks() {
            RAIL_W * self.factor()
        } else {
            0.0
        }
    }

    /// The panel to paint this frame, if any.
    pub(super) fn panel(&self) -> Option<Panel> {
        match self.mode {
            Some(panel) => Some(panel),
            // The close: the panel the rail is leaving stays up for the whole
            // motion, so the rail reads as one thing moving rather than a column
            // of rows vanishing out of a box.
            None if self.slide.moving() => Some(self.last),
            None => None,
        }
    }

    /// Whether `panel` is the one on screen.
    pub(super) fn shows(&self, panel: Panel) -> bool {
        self.panel() == Some(panel)
    }

    /// The bar's toggle, and a floating rail's edge: close, or restore.
    pub(super) fn toggle(&mut self) {
        match self.mode {
            Some(_) => self.close(),
            None => self.open(self.last),
        }
    }

    /// Open on `panel`, and remember it as the one a reopen restores.
    pub(super) fn open(&mut self, panel: Panel) {
        self.last = panel;
        self.mode = Some(panel);
        self.forget();
        self.grace = None;
        self.slide.begin(1.0, !self.animates);
    }

    /// Put the rail away.
    pub(super) fn close(&mut self) {
        if self.mode.take().is_some() {
            self.slide.begin(0.0, !self.animates);
        }
    }

    /// The pointer entered or left the rail. A docked rail is opened and closed
    /// by hand only; a floating one is the pointer's while it is over it.
    pub(super) fn hold(&mut self, held: bool) {
        if !self.overlay || self.mode.is_none() {
            return;
        }
        self.grace = (!held).then_some(HOVER_GRACE_MS);
    }

    /// The panel's own size, as it reported it.
    pub(super) fn measured(&mut self, height: f32) {
        self.view = height.max(0.0);
    }

    /// Where the panel's list has come to rest.
    pub(super) fn scrolled(&mut self, offset: f32) {
        self.offset = offset.max(0.0);
    }

    /// Ask the panel to centre the chapter the reader is in — the deliberate
    /// gesture, which moves whether or not the row is already on screen.
    pub(super) fn centre(&mut self, index: Option<usize>) {
        // A panel nobody has measured has no window to centre in.
        if let Some(index) = index.filter(|_| self.view > 0.0) {
            self.aim = Some(Aim::new(outline::centre(index, self.view)));
        }
    }

    /// Look again at where the reader is. A page that has moved under the panel
    /// owes a scroll when the chapter it landed in is off the panel's window.
    pub(super) fn watch(&mut self, index: Option<usize>, page: u32) {
        // A panel that has not been laid out yet cannot be told where to scroll:
        // its window is a zero, and the answer would be a row's height short.
        // Nothing is remembered then, so the next frame asks again.
        if self.view <= 0.0 || !self.shows(Panel::Outline) {
            return;
        }
        if self.seen == Some(page) {
            return;
        }
        self.seen = Some(page);
        let Some(index) = index else {
            return;
        };
        self.aim = outline::reveal(index, self.view, self.offset).map(Aim::new);
    }

    /// The scroll the panel is owed, if the surface is ready for it.
    pub(super) fn aim_effect(&mut self) -> Option<f32> {
        let aim = self.aim.as_mut()?;
        if !aim.built {
            aim.built = true;
            return None;
        }
        let offset = aim.offset;
        self.aim = None;
        Some(offset)
    }

    /// Forget where the panel left the reader: the next look reveals again.
    pub(super) fn forget(&mut self) {
        self.seen = None;
    }

    /// One frame of the rail's own clock.
    pub(super) fn step(&mut self, delta_ms: f64) {
        self.slide.step(delta_ms);
        if let Some(left) = self.grace.as_mut() {
            *left -= delta_ms.max(0.0);
        }
        if self.grace.is_some_and(|left| left <= 0.0) {
            self.grace = None;
            self.close();
        }
    }

    /// Whether the rail owes animation frames — a motion running, a grace
    /// counting down, or a reveal waiting for its frame.
    pub(super) fn ticking(&self) -> bool {
        self.slide.moving() || self.grace.is_some() || self.aim.is_some()
    }
}

impl Reader {
    /// One rail action.
    pub(super) fn rail(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Toggle => self.sidebar.toggle(),
            Action::Open(panel) => self.sidebar.open(panel),
            Action::Close => self.sidebar.close(),
            Action::Hold(held) => self.sidebar.hold(held),
            Action::Reveal => {
                let index = self.chapter_index();
                self.sidebar.centre(index);
            }
            Action::Measured(height) => self.sidebar.measured(height),
            Action::Scrolled(offset) => self.sidebar.scrolled(offset),
            Action::Chapter(index) => return self.chapter(index),
        }
        Vec::new()
    }

    /// The chapter the reader is inside, as an index into the outline: what the
    /// panel marks and what a reveal is aimed at.
    fn chapter_index(&self) -> Option<usize> {
        active_entry(&self.document.outline, self.viewer.page)
    }

    /// The rail's own surface commands, resolved after every message: a page the
    /// reader has left behind the panel arms a reveal, and an armed one is posted
    /// once its panel has been built.
    pub(super) fn rail_effects(&mut self) -> Vec<Effect> {
        if self.sidebar.shows(Panel::Outline) {
            let index = self.chapter_index();
            self.sidebar.watch(index, self.viewer.page);
        }
        self.sidebar
            .aim_effect()
            .map(Effect::Outline)
            .into_iter()
            .collect()
    }
}
