//! The fit arithmetic: the scale a page's fit resolves to, the ceiling a hand-
//! picked zoom may not pass, and the clamp every scale goes through.

use reader_core::zoom_math::{clamp_scale, FitMode, MIN_SCALE};

use crate::formats::pdf::PageBox;
use super::super::viewer::{FitDims, Viewer};
use super::{Intent, Target, Zoom};

/// A scale the pipeline will act on: the ladder's range, and a non-finite
/// input — NaN, infinity, a corrupt measurement upstream — collapsed to the
/// minimum rather than poisoning every derived box.
pub(super) fn sane(scale: f64) -> f64 {
    if scale.is_finite() {
        clamp_scale(scale)
    } else {
        MIN_SCALE
    }
}

/// The scale the active fit mode wants, recorded as the reader's own choice.
/// `None` with no fit mode: a refit of a hand-picked zoom would resolve to the
/// current scale *and* clobber `desired`, resurrecting an old number as the
/// ceiling.
pub(super) fn fitted(viewer: &Viewer, zoom: &Zoom, box_: PageBox) -> Option<Target> {
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
pub(super) fn ceiling(viewer: &Viewer, zoom: &Zoom) -> Option<Target> {
    if viewer.fit != FitMode::None {
        return None; // a fit mode owns the scale while it is active
    }
    Some(Target {
        scale: sane(zoom.desired),
        intent: Intent::Ceiling,
    })
}
