//! The reader's motion: the one easing curve its moves share, the glide a page
//! turn rides, and which of the reader's own switches allow either.

use reader_core::settings::Settings;
use reader_core::zoom_math::ease_out_cubic;

/// How long a gliding page turn takes, in milliseconds. Long enough to read as
/// travel, short enough that the turn still answers the key that asked for it.
pub(super) const GLIDE_MS: f64 = 240.0;

/// How far a glide may travel, in viewports. Past this a jump lands at once:
/// a glide across half a book is a wait, not a motion.
pub(super) const GLIDE_VIEWPORTS: f64 = 2.0;

/// The reader's motion switches, projected from the settings once so no caller
/// asks the schema twice. The master switch speaks for every one of them: off,
/// a move still happens, it just lands in the frame it was asked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Motion {
    /// A zoom eases to its target instead of appearing there.
    pub zoom: bool,
    /// Jumping to a page glides the strip there.
    pub glide: bool,
}

impl Motion {
    pub(super) fn from_settings(settings: &Settings) -> Self {
        let animations = &settings.animations;
        Self {
            zoom: animations.enabled && animations.zoom,
            glide: animations.enabled && animations.scroll_jumps,
        }
    }

    /// Whether a jump of `distance` across a `viewport` may glide at all.
    pub(super) fn glides(&self, distance: f64, viewport: f64) -> bool {
        self.glide && distance.abs() <= GLIDE_VIEWPORTS * viewport.max(1.0)
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            zoom: true,
            glide: true,
        }
    }
}

/// A gliding move along the strip: where it started, where it is going, and how
/// far into the clock it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Glide {
    from: f64,
    to: f64,
    elapsed_ms: f64,
}

impl Glide {
    pub(super) fn begin(from: f64, to: f64) -> Self {
        Self {
            from,
            to,
            elapsed_ms: 0.0,
        }
    }

    /// The offset to command this frame, and whether the glide is over. The last
    /// frame answers the target itself, so a glide cannot land a pixel short of
    /// the page it was aimed at.
    pub(super) fn step(&mut self, delta_ms: f64) -> (f64, bool) {
        self.elapsed_ms += delta_ms.max(0.0);
        let progress = (self.elapsed_ms / GLIDE_MS).clamp(0.0, 1.0);
        if progress >= 1.0 {
            return (self.to, true);
        }
        (self.from + (self.to - self.from) * ease_out_cubic(progress), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_starts_where_the_strip_is_and_lands_on_the_page() {
        let mut glide = Glide::begin(100.0, 900.0);
        assert_eq!(glide.step(0.0), (100.0, false), "the first frame is where it was");
        let (mid, landed) = glide.step(GLIDE_MS / 2.0);
        assert!(mid > 100.0 && mid < 900.0, "half the clock is part of the way");
        assert!(!landed);
        assert_eq!(glide.step(GLIDE_MS), (900.0, true), "the clock runs out on the page");
    }

    #[test]
    fn a_long_jump_lands_at_once_and_a_frozen_reader_lands_it_too() {
        let motion = Motion::default();
        assert!(motion.glides(1200.0, 800.0), "a viewport and a half is worth watching");
        assert!(!motion.glides(20_000.0, 800.0), "across a book is not");
        let frozen = Motion {
            glide: false,
            ..Motion::default()
        };
        assert!(!frozen.glides(10.0, 800.0), "the switch speaks for every jump");
    }

    #[test]
    fn each_switch_speaks_for_its_own_move_and_the_master_for_both() {
        let mut settings = Settings::default();
        let both = Motion::from_settings(&settings);
        assert!(both.zoom && both.glide, "the defaults allow every move");
        settings.animations.scroll_jumps = false;
        let no_glide = Motion::from_settings(&settings);
        assert!(!no_glide.glide && no_glide.zoom, "one switch, one move");
        settings.animations.scroll_jumps = true;
        settings.animations.enabled = false;
        let frozen = Motion::from_settings(&settings);
        assert!(!frozen.glide && !frozen.zoom, "the master speaks for both");
    }
}
