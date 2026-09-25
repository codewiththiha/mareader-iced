//! The tween: where a zoom is going, how long it takes, how it eases, and the
//! three answers a stepping zoom gives its caller.

use super::Intent;

/// The zoom tween's length, in milliseconds. Short enough that a press reads
/// as immediate, long enough that the eye sees the page move.
pub(super) const TWEEN_MS: f64 = 120.0;

/// How long the space around the page must be quiet before a held follow
/// commits its crisp render. The layout follows every frame of a drag; the
/// rasters wait for the end of it.
pub(super) const SETTLE_MS: f64 = 180.0;

/// Scales closer than this are the same scale. One margin for the whole
/// pipeline: the resolver calls a step that cannot move a no-op, and a
/// transition that would not go anywhere is never opened.
pub(super) const SETTLED_EPSILON: f64 = 0.0005;

/// A resolved command: the scale to move to, and what writing it means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub scale: f64,
    pub intent: Intent,
}

/// A live zoom transaction: what is animating, from where, to where, and how
/// far along it is. It exists for exactly the duration of the transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// The scale the tween started from, so a retarget continues from wherever
    /// the eye is instead of teleporting.
    pub from: f64,
    pub to: f64,
    /// How far into the tween, in milliseconds.
    pub elapsed_ms: f64,
    /// Whether this transition tweens. A landing that does not animate still
    /// opens one when it is held (a follow): the transaction is what the
    /// commit waits on.
    pub animate: bool,
    /// True while this is a container follow: its commit is held until the
    /// burst goes quiet, and only a follow may be retargeted by a later
    /// command — never a gesture's own tween.
    pub following: bool,
}

/// What one command or one frame did, so the reader knows whether to repaint
/// and whether its rasters have gone stale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Advanced {
    /// The scale on screen moved: the page is drawn at a new size next frame.
    pub moved: bool,
    /// The committed scale moved: the rasters must be asked for again. Only
    /// ever once per transaction.
    pub committed: bool,
}

impl Advanced {
    /// Nothing happened.
    pub const NONE: Self = Self {
        moved: false,
        committed: false,
    };

    /// The eye moved; the rasters are still crisp at their own scale.
    pub const MOVED: Self = Self {
        moved: true,
        committed: false,
    };

    /// The transaction ended: repaint, and re-ask the engine.
    pub const LANDED: Self = Self {
        moved: true,
        committed: true,
    };
}
