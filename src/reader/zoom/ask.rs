//! What a zoom is asked for, and what the ask resolves to against the state it
//! lands in: the scale the reader sees now, the one they are heading for, and the
//! one the fit would give.

use reader_core::zoom_math::nearest_zoom;

use crate::formats::pdf::PageBox;
use super::super::viewer::Viewer;
use super::{ceiling, fitted, sane, SETTLED_EPSILON, Target, Zoom};

/// One zoom intent, posted by whichever surface wants the zoom to change —
/// the reading bar's control, the keyboard's ladder steps, a window resize.
///
/// Deliberately not a scale: a command is resolved against the state it lands
/// in, so the same `Step(1)` means "one rung up from wherever the eye is".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// One step along the preset ladder: `+1` zooms in, `-1` zooms out.
    Step(i32),
    /// Re-resolve the active fit mode against the window and the sheet under
    /// the reader's eyes. Stands down when no fit mode is active.
    Refit,
    /// Re-resolve a hand-picked zoom against the window. The reader's own
    /// `desired` is authoritative up to the clamp — a manual zoom is never
    /// shrunk back to fit width.
    Constrain,
    /// The space around the page moved: a window resize, a docked rail taking
    /// its column. Resolves to whichever of the two above owns the scale.
    Follow,
}

/// What resolution decided the command should write, kept apart from the
/// scales themselves so the rule is one function rather than three call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// A hand-picked scale: it becomes `desired` and drops the fit mode, since
    /// a fit ceiling must never fight a gesture.
    Manual,
    /// A fit mode's answer: it becomes `desired` too, because a fit mode *is*
    /// a deliberate choice — without this, leaving the fit would resurrect an
    /// earlier gesture's number and the page would jump to it.
    Fit,
    /// A window constraint: `desired` is left exactly as it was, which is what
    /// makes it stable across a whole drag.
    Ceiling,
}

/// Resolve a command to the scale it wants, or `None` when it must stand down
/// — a step with nowhere to go, a fit that is not active, a window constraint
/// while a fit mode owns the scale.
///
/// Manual steps chain from the in-flight target when a transition is running,
/// so a fast `+ +` advances a rung per press instead of resolving the rung the
/// tween is already heading for.
pub fn resolve(viewer: &Viewer, zoom: &Zoom, box_: PageBox, cmd: Command) -> Option<Target> {
    match cmd {
        Command::Step(dir) => {
            let base = zoom.in_flight_target().unwrap_or(zoom.display);
            let scale = sane(nearest_zoom(base, dir));
            // At the end of the ladder `nearest_zoom` answers with the rung the
            // reader is already on, so there is nowhere to go. Bailing here
            // (before any intent is recorded) keeps leaning on the zoom button
            // from dropping the reader out of Fit Width.
            if (scale - base).abs() < SETTLED_EPSILON {
                return None;
            }
            Some(Target {
                scale,
                intent: Intent::Manual,
            })
        }
        Command::Refit => fitted(viewer, zoom, box_),
        Command::Constrain => ceiling(viewer, zoom),
        // The space around the page moved. Both cases are the same question —
        // what does the width deserve now? — and exactly one owns the answer:
        // an active fit mode, else the reader's own zoom.
        Command::Follow => fitted(viewer, zoom, box_).or_else(|| ceiling(viewer, zoom)),
    }
}
