//! The zoom pipeline: one resolver, three scales, one tween.
//!
//! A port of the web app's `src/zoom/*` into the shape an iced app can hold.
//! The DOM half has no counterpart here — there is no actuator rescaling
//! strips and no rAF loop — and that is exactly why the arithmetic is kept
//! whole: the reading surface paints the sheet it has at the display scale and
//! re-asks the engine for a raster at the committed one, so "the layout moved"
//! is simply "the next frame draws the page at a new size".
//!
//! ```text
//! resolve the command against the window, the mode and the sheet
//!     ↓
//! open a transition from the scale on screen to that target
//!     ↓
//! tween the DISPLAY scale — the page grows under the reader's eyes
//!     ↓
//! bring the committed scale onto the target, once, and re-rasterise
//! ```
//!
//! The three scales are the web app's three, deliberately kept apart:
//!
//! - `desired` — what the reader asked for, whether or not it currently fits.
//!   It is the ceiling a hand-picked zoom resolves to, so a page zoomed in on
//!   overflows with a scroll affordance instead of snapping back to fit width.
//! - `display` — the scale on screen this frame: what the painter reads.
//! - `committed` — the scale the mounted raster is crisp at, and the only
//!   scale a frame is ever asked for. It moves once per transaction, when the
//!   transition lands.
//!
//! Two cadences, both the web app's: a manual change tweens — one gesture,
//! 120 ms on an out-cubic curve — while a container [`Command::Follow`] lands
//! in the frame it was asked for (a tween chasing a moving window reads as
//! lag), its crisp commit held until the burst goes quiet ([`SETTLE_MS`]), so
//! a window drag costs one raster pass rather than one per frame.
//!
//! Nothing here touches iced: the reader drives it with `begin`/`advance`, so
//! the ladder, the fit resolution and both cadences are unit-testable on the
//! host.

use reader_core::zoom_math::{FitMode, MIN_SCALE, clamp_scale, nearest_zoom};

use crate::formats::pdf::PageBox;

use super::viewer::{FitDims, Viewer};

/// The zoom tween's length, in milliseconds. Short enough that a press reads
/// as immediate, long enough that the eye sees the page move.
const TWEEN_MS: f64 = 120.0;

/// How long the space around the page must be quiet before a held follow
/// commits its crisp render. The layout follows every frame of a drag; the
/// rasters wait for the end of it.
const SETTLE_MS: f64 = 180.0;

/// Scales closer than this are the same scale. One margin for the whole
/// pipeline: the resolver calls a step that cannot move a no-op, and a
/// transition that would not go anywhere is never opened.
const SETTLED_EPSILON: f64 = 0.0005;

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

    /// Open a transition to `scale`, or land it in this frame.
    ///
    /// `animate` asks for the tween; a follow never tweens whatever it asks,
    /// because a scale that eases towards a moving window chases it instead of
    /// sitting in it. An untweened landing commits in the same frame, except a
    /// follow's: it lands — the layout has to move with the window — and holds
    /// its commit for the settle window.
    pub fn begin(&mut self, scale: f64, animate: bool, following: bool) -> Advanced {
        let scale = sane(scale);
        // Move the settle deadline before deciding this frame is a no-op:
        // while a burst runs the scale may be pinned (a page at the minimum, a
        // hand-picked zoom the window has caught up with), and a commit landing
        // on a frame the window is still moving rasterises at a size the reader
        // is already past.
        if following {
            self.settle_ms = SETTLE_MS;
        }
        // Already there, or already bound there: nothing to move — and the
        // transaction in flight is left exactly as it is.
        let settled = self.in_flight_target().unwrap_or(self.display);
        if (scale - settled).abs() < SETTLED_EPSILON {
            return Advanced::NONE;
        }
        let animate = animate && !following;
        self.transition = Some(Transition {
            from: self.display,
            to: scale,
            elapsed_ms: 0.0,
            animate,
            following,
        });
        if following {
            // A follow lands in the frame it was asked for, not on the next
            // animation frame: the new size has to be in the same frame as the
            // new width, or the page sits a few pixels wider than its box for
            // a whole frame — a flicker along the whole drag.
            self.display = scale;
            return Advanced::MOVED;
        }
        if !animate {
            // A refit tracking a live resize lands where it was asked to; only
            // a gesture is worth 120 ms.
            self.transition = None;
            self.commit(scale);
            return Advanced::LANDED;
        }
        Advanced::MOVED
    }

    /// One animation frame, `delta_ms` after the last.
    pub fn advance(&mut self, delta_ms: f64) -> Advanced {
        let Some(mut t) = self.transition else {
            return Advanced::NONE;
        };
        if t.following {
            // A held follow has already landed; what is left is its deadline,
            // re-armed by every post, so a burst re-arms one window rather than
            // queueing a commit per frame — the commit happens once, at the
            // size the window stopped at.
            self.settle_ms -= delta_ms.max(0.0);
            if self.settle_ms <= 0.0 {
                self.transition = None;
                self.settle_ms = 0.0;
                self.commit(t.to);
                return Advanced::LANDED;
            }
            return Advanced::NONE;
        }
        t.elapsed_ms += delta_ms.max(0.0);
        // The transition's own guard, the web app's tween loop kept the same
        // one: a transaction opened without a tween has no midpoint to
        // interpolate through — its first frame is its last.
        let progress = if t.animate {
            (t.elapsed_ms / TWEEN_MS).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.display = t.from + (t.to - t.from) * ease_out_cubic(progress);
        if progress >= 1.0 {
            let to = t.to;
            self.transition = None;
            self.commit(to);
            return Advanced::LANDED;
        }
        self.transition = Some(t);
        Advanced::MOVED
    }

    /// Every transaction ends here: the committed scale moves onto the target,
    /// and the display scale with it — a tween's last frame interpolated to
    /// exactly this number, and an untweened landing wrote it outright.
    fn commit(&mut self, to: f64) {
        self.display = to;
        self.committed = to;
    }
}

/// A scale the pipeline will act on: the ladder's range, and a non-finite
/// input — NaN, infinity, a corrupt measurement upstream — collapsed to the
/// minimum rather than poisoning every derived box.
fn sane(scale: f64) -> f64 {
    if scale.is_finite() {
        clamp_scale(scale)
    } else {
        MIN_SCALE
    }
}

/// The tween's progress curve: covers ground early, decelerates onto the
/// target instead of stopping dead on it — the web app's `ease_out_cubic`.
fn ease_out_cubic(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
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

/// The scale the active fit mode wants, recorded as the reader's own choice.
/// `None` with no fit mode: a refit of a hand-picked zoom would resolve to the
/// current scale *and* clobber `desired`, resurrecting an old number as the
/// ceiling.
fn fitted(viewer: &Viewer, zoom: &Zoom, box_: PageBox) -> Option<Target> {
    if viewer.fit == FitMode::None {
        return None;
    }
    let dims = FitDims::from_geometry(viewer.mode, viewer.container, viewer.margin, box_)?;
    Some(Target {
        scale: dims.fit(viewer.fit, zoom.committed),
        intent: Intent::Fit,
    })
}

/// The ceiling a hand-picked zoom resolves to: the reader's own `desired`,
/// clamped.
///
/// Deliberately not `min(desired, fit width)`, which is the ceiling that used
/// to lock a manual zoom at fit width, so a reader could never look at a page
/// up close: a page too wide for the window overflows and scrolls instead of
/// snapping back. Computing from `desired` — never from the live scale times a
/// container ratio — is also why a drag cannot accumulate rounding and land
/// somewhere the reader never asked for.
fn ceiling(viewer: &Viewer, zoom: &Zoom) -> Option<Target> {
    if viewer.fit != FitMode::None {
        return None; // a fit mode owns the scale while it is active
    }
    Some(Target {
        scale: sane(zoom.desired),
        intent: Intent::Ceiling,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_core::zoom_math::MAX_SCALE;

    use iced::Size;

    fn page(w: f64, h: f64) -> PageBox {
        PageBox {
            width: w,
            height: h,
        }
    }

    /// A window and a sheet whose fit width is exactly 2.0 (1000 / 500).
    fn viewer(fit: FitMode) -> Viewer {
        Viewer {
            fit,
            container: Size::new(1000.0, 800.0),
            ..Viewer::default()
        }
    }

    fn sheet() -> PageBox {
        page(500.0, 700.0)
    }

    fn zoom_at(scale: f64) -> Zoom {
        let mut zoom = Zoom::default();
        zoom.initialize(scale);
        zoom
    }

    #[test]
    fn a_step_advances_one_rung_and_records_the_readers_own_scale() {
        let mut zoom = zoom_at(1.0);
        let target = resolve(&viewer(FitMode::Width), &zoom, sheet(), Command::Step(1))
            .expect("the ladder has room");
        assert_eq!(target.intent, Intent::Manual);
        assert!((target.scale - 1.25).abs() < 1e-9);
        assert!(zoom.note(target), "a manual step drops the fit mode");
        assert!((zoom.desired - 1.25).abs() < 1e-9);
        // Untweened here, so the rasters are already stale: one frame, one
        // commit, no animation owed.
        assert_eq!(zoom.begin(target.scale, false, false), Advanced::LANDED);
        assert!((zoom.committed - 1.25).abs() < 1e-9);
        assert!(!zoom.ticking());
    }

    #[test]
    fn a_step_at_the_end_of_the_ladder_stands_down_untouched() {
        // Leaning on the zoom button with nothing left to do must not take the
        // reader out of Fit Width, nor write a `desired` they did not ask for.
        for (scale, dir) in [(MAX_SCALE, 1), (MIN_SCALE, -1)] {
            let zoom = zoom_at(scale);
            assert!(
                resolve(&viewer(FitMode::Width), &zoom, sheet(), Command::Step(dir)).is_none(),
                "{scale} dir {dir}"
            );
        }
    }

    #[test]
    fn a_step_chains_from_an_in_flight_target() {
        // `+ +` in quick succession advances two rungs: resolution reads the
        // target the tween is heading for, not the half-way scale on screen.
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(1.25, true, false), Advanced::MOVED);
        let target = resolve(
            &viewer(FitMode::None),
            &zoom,
            sheet(),
            Command::Step(1),
        )
        .expect("one rung further up");
        assert!((target.scale - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_refit_answers_with_the_fit_and_owns_the_ceiling() {
        let mut zoom = zoom_at(1.0);
        let target = resolve(&viewer(FitMode::Width), &zoom, sheet(), Command::Refit)
            .expect("a fit mode is active");
        assert_eq!(target.intent, Intent::Fit);
        assert!((target.scale - 2.0).abs() < 1e-9, "1000 / 500 of usable width");
        assert!(!zoom.note(target), "a fit leaves the reader's fit mode alone");
        assert!(
            (zoom.desired - 2.0).abs() < 1e-9,
            "the fit is a deliberate choice, so it owns the ceiling too"
        );
    }

    #[test]
    fn a_refit_stands_down_when_no_fit_mode_is_active() {
        let zoom = zoom_at(1.9);
        assert!(resolve(&viewer(FitMode::None), &zoom, sheet(), Command::Refit).is_none());
    }

    #[test]
    fn a_constraint_leaves_the_readers_own_zoom_alone() {
        // The ceiling is `desired`, deliberately uncapped by the fit width: a
        // page zoomed in on overflows and scrolls. A window constraint must not
        // rewrite it either, or a drag would slowly move the reader's zoom.
        let mut zoom = zoom_at(1.0);
        zoom.desired = 2.5;
        let target = resolve(&viewer(FitMode::None), &zoom, sheet(), Command::Constrain)
            .expect("a hand-picked zoom owns the scale");
        assert_eq!(target.intent, Intent::Ceiling);
        assert!((target.scale - 2.5).abs() < 1e-9);
        assert!(!zoom.note(target));
        assert!((zoom.desired - 2.5).abs() < 1e-9, "untouched");
    }

    #[test]
    fn a_constraint_stands_down_while_a_fit_mode_is_active() {
        let zoom = zoom_at(2.5);
        assert!(resolve(&viewer(FitMode::Page), &zoom, sheet(), Command::Constrain).is_none());
    }

    #[test]
    fn a_follow_answers_with_whichever_owns_the_scale() {
        let zoom = zoom_at(1.0);
        let fit = resolve(&viewer(FitMode::Width), &zoom, sheet(), Command::Follow)
            .expect("the fit owns it");
        assert_eq!(fit.intent, Intent::Fit);
        assert!((fit.scale - 2.0).abs() < 1e-9);

        let mut own = zoom_at(1.0);
        own.desired = 1.2;
        let kept = resolve(&viewer(FitMode::None), &own, sheet(), Command::Follow)
            .expect("the reader's own zoom owns it");
        assert_eq!(kept.intent, Intent::Ceiling);
        assert!((kept.scale - 1.2).abs() < 1e-9);
    }

    #[test]
    fn a_landing_with_nothing_to_do_opens_no_transition() {
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(1.0, true, false), Advanced::NONE);
        assert!(!zoom.ticking(), "nothing was opened");
        assert!((zoom.display - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_tween_lands_on_its_target_and_commits_once() {
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(2.0, true, false), Advanced::MOVED);
        assert!(zoom.ticking());
        // Halfway through, the eye is well up the way — out-cubic covers ground
        // early — while the raster scale has not moved at all: that gap is what
        // "the page is still crisp" means.
        assert_eq!(zoom.advance(TWEEN_MS / 2.0), Advanced::MOVED);
        assert!(zoom.display > 1.5, "out-cubic covers ground early: {}", zoom.display);
        assert!((zoom.committed - 1.0).abs() < 1e-9, "the raster has not moved");
        assert_eq!(zoom.advance(TWEEN_MS), Advanced::LANDED);
        assert!((zoom.display - 2.0).abs() < 1e-12);
        assert!((zoom.committed - 2.0).abs() < 1e-12);
        assert!(!zoom.ticking());
        // And an idle frame does nothing at all.
        assert_eq!(zoom.advance(16.0), Advanced::NONE);
    }

    #[test]
    fn a_retarget_continues_from_where_the_eye_is() {
        let mut zoom = zoom_at(1.0);
        zoom.begin(2.0, true, false);
        zoom.advance(TWEEN_MS / 2.0);
        let eye = zoom.display;
        // A second press mid-flight retargets on a restarted clock, from the
        // scale on screen — never from the target it was heading for, which
        // would teleport the page.
        assert_eq!(zoom.begin(2.5, true, false), Advanced::MOVED);
        let t = zoom.transition.expect("the retarget opened a transition");
        assert!((t.from - eye).abs() < 1e-12);
        assert_eq!(t.elapsed_ms, 0.0);
        assert!((t.to - 2.5).abs() < 1e-12);
        assert!(t.animate);
        assert!(!t.following);
    }

    #[test]
    fn a_held_follow_lands_at_once_and_waits_out_its_quiet_window() {
        let mut zoom = zoom_at(1.0);
        // The layout moves in the frame it was asked for…
        assert_eq!(zoom.begin(1.4, false, true), Advanced::MOVED);
        assert!((zoom.display - 1.4).abs() < 1e-9);
        assert!((zoom.committed - 1.0).abs() < 1e-9, "but the raster waits");
        // …a later frame of the same burst pushes the deadline back rather than
        // queueing a commit of its own…
        assert_eq!(zoom.advance(SETTLE_MS - 10.0), Advanced::NONE);
        assert_eq!(zoom.begin(1.6, false, true), Advanced::MOVED);
        assert_eq!(zoom.advance(SETTLE_MS - 10.0), Advanced::NONE);
        // …and, once the window is quiet, one commit lands at the size it
        // stopped at.
        assert_eq!(zoom.advance(20.0), Advanced::LANDED);
        assert!((zoom.committed - 1.6).abs() < 1e-9);
        assert!(!zoom.ticking());
    }

    #[test]
    fn a_follow_that_moves_nothing_still_pushes_back_an_open_deadline() {
        // The window keeps resizing while the scale is pinned (a page already
        // at the minimum). Nothing moves, but every frame is still a frame of
        // the burst: the commit must wait for the end of it, and then land —
        // otherwise the transaction would be left open, holding the rasters.
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(1.4, false, true), Advanced::MOVED);
        assert_eq!(zoom.advance(SETTLE_MS - 10.0), Advanced::NONE);
        assert_eq!(zoom.begin(1.4, false, true), Advanced::NONE, "nothing to move");
        assert!(zoom.ticking(), "…and the deadline was pushed back");
        assert_eq!(
            zoom.advance(SETTLE_MS - 10.0),
            Advanced::NONE,
            "a re-armed window is a whole one, not the old remainder"
        );
        assert_eq!(zoom.advance(20.0), Advanced::LANDED);
        assert!((zoom.committed - 1.4).abs() < 1e-9);
    }

    #[test]
    fn an_untweened_command_lands_in_the_frame_it_was_posted() {
        // A refit tracking a live resize must not chase the window over 120 ms;
        // the reader asked for the frames to be dropped and the end frame to
        // land.
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(0.5, false, false), Advanced::LANDED);
        assert!((zoom.display - 0.5).abs() < 1e-9);
        assert!((zoom.committed - 0.5).abs() < 1e-9);
        assert!(!zoom.ticking());
    }

    #[test]
    fn the_seed_leaves_the_scales_in_agreement() {
        let mut zoom = Zoom::default();
        zoom.initialize(3.0);
        assert_eq!((zoom.desired, zoom.display, zoom.committed), (3.0, 3.0, 3.0));
        assert!(!zoom.ticking());
        // A corrupt measurement collapses to the ladder's end rather than
        // poisoning every derived box.
        zoom.initialize(f64::NAN);
        assert_eq!(zoom.display, MIN_SCALE);
    }

    #[test]
    fn a_scale_beyond_the_ladder_is_clamped_to_its_end() {
        let mut zoom = zoom_at(1.0);
        assert_eq!(zoom.begin(99.0, false, false), Advanced::LANDED);
        assert_eq!(zoom.committed, MAX_SCALE);
        let mut high = zoom_at(1.0);
        high.desired = f64::INFINITY;
        let target = resolve(&viewer(FitMode::None), &high, sheet(), Command::Constrain)
            .expect("a hand-picked zoom owns the scale");
        assert_eq!(target.scale, MIN_SCALE, "a corrupt `desired` is not a scale");
    }
}
