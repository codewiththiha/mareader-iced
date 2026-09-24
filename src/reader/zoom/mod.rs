//! The zoom pipeline: one resolver, three scales, one tween. A port of the web
//! app's `src/zoom/*` into the shape an iced app can hold — there is no actuator
//! and no frame loop, so the arithmetic stays whole and the reader asking for a
//! scale is the only door.
mod advance;
mod ask;
mod begin;
mod fit;
mod tween;

pub use ask::*;
pub use tween::*;

#[cfg(test)]
mod tests;

use fit::sane;

/// The zoom pipeline's whole state: the three scales, the transition in
/// flight, and the held follow's quiet window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    /// The zoom the reader asked for, whether or not it currently fits.
    pub desired: f64,
    /// The live scale — what the reader is looking at this frame.
    pub display: f64,
    /// The scale the mounted raster is crisp at.
    pub committed: f64,
    /// The transition in flight, if any. `None` means idle.
    pub transition: Option<Transition>,
    /// Milliseconds left before a held follow commits.
    settle_ms: f64,
}

impl Default for Zoom {
    fn default() -> Self {
        Self {
            desired: 1.0,
            display: 1.0,
            committed: 1.0,
            transition: None,
            settle_ms: 0.0,
        }
    }
}

impl Zoom {
    /// Seed every scale for a freshly opened document: no transition, and the
    /// scales already in agreement, so the first frame is crisp.
    pub fn initialize(&mut self, scale: f64) {
        let scale = sane(scale);
        self.desired = scale;
        self.display = scale;
        self.committed = scale;
        self.transition = None;
        self.settle_ms = 0.0;
    }

    /// Write a resolved target's intent and answer whether the fit mode has to
    /// be dropped with it — the one part of the intent that lives on the
    /// viewer rather than here.
    ///
    /// A manual zoom clears the fit: the reader's own scale has taken over. A
    /// fit and a window constraint leave it alone.
    pub fn note(&mut self, target: Target) -> bool {
        match target.intent {
            Intent::Manual => {
                self.desired = target.scale;
                true
            }
            Intent::Fit => {
                self.desired = target.scale;
                false
            }
            Intent::Ceiling => false,
        }
    }

    /// The scale the in-flight transition is heading to, if any. Manual steps
    /// chain from this, so a fast `+ +` advances two rungs rather than
    /// resolving the same rung twice.
    pub fn in_flight_target(&self) -> Option<f64> {
        self.transition.map(|t| t.to)
    }

    /// Whether an animation frame is still owed: true for the whole life of a
    /// transaction, a held follow included — its commit is the thing waiting.
    pub fn ticking(&self) -> bool {
        self.transition.is_some()
    }

    /// Abandon whatever is in flight: the reader is leaving the document, and
    /// nothing is waiting on a scale that no longer has a page to draw. The
    /// scales stay where they are — the next open re-seeds them from its own
    /// fit — and the reader's own choice survives it, the way the mode, the fit
    /// and the margin do.
    pub fn cancel(&mut self) {
        self.transition = None;
        self.settle_ms = 0.0;
    }
}
