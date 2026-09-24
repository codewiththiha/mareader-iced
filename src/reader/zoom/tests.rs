//! The module's tests: the subjects beside this hold the code, and this
//! holds the cases they have to satisfy.

use super::*;

use reader_core::zoom_math::{FitMode, MIN_SCALE};

use crate::formats::pdf::PageBox;

use super::super::viewer::Viewer;

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
