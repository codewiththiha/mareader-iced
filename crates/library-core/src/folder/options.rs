//! The options an import was made with: which formats count, how big a file has to
//! be, and whether the folder is watched.

use std::collections::BTreeSet;

use reader_core::format::Format;
use serde::{Deserialize, Serialize};

use crate::scan::{admits, selectable_formats};
use super::FolderMode;

/// The size threshold the import sheet opens on: a PDF smaller than this is a
/// stub, a placeholder or a corrupt download. A default, not a rule.
pub(super) const DEFAULT_MIN_SIZE: u64 = 30 * 1024;

/// The −/+ step, counted in steps rather than bytes so no caller can invent a
/// value the control could not have produced.
const MIN_SIZE_STEP: u64 = 10 * 1024;

/// Bounds for the −/+ buttons; [`sanitize`] clamps a loaded blob back inside them.
pub const MIN_SIZE_FLOOR: u64 = 0;

pub const MIN_SIZE_CEIL: u64 = 500 * 1024;

/// How one folder is scanned. Every field is a choice the import sheet offers
/// and every one is honoured on every later rescan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpts {
    /// Always a subset of [`selectable_formats`].
    #[serde(default = "default_formats")]
    pub formats: BTreeSet<Format>,
    /// `true` = only these formats; `false` = everything but these. One set
    /// and a flip rather than two lists that can contradict each other.
    #[serde(default = "default_true")]
    pub include_selected: bool,
    /// Strict lower bound in bytes: a file of exactly this size is refused.
    #[serde(default = "default_min_size")]
    pub min_size: u64,
    /// `true` links each book to the address it was found at; `false` copies
    /// it into the app's store. Defaults to `true` so an upgrading reader keeps
    /// the library they had.
    #[serde(default = "default_true")]
    pub in_place: bool,
    /// Legacy mirror of the root rung's tracking answer (see
     /// [`WatchedFolder::set_tracking`]); the sheet only offers it alongside
     /// [`FolderOpts::in_place`].
    #[serde(default)]
    pub watch: bool,
    /// Cut a shelf per subfolder (`true`) or keep the whole tree on one shelf.
    #[serde(default = "default_true")]
    pub groups: bool,
}

pub(super) fn default_formats() -> BTreeSet<Format> {
    selectable_formats().into_iter().collect()
}

fn default_true() -> bool {
    true
}

fn default_min_size() -> u64 {
    DEFAULT_MIN_SIZE
}

impl Default for FolderOpts {
    fn default() -> Self {
        Self {
            formats: default_formats(),
            include_selected: true,
            min_size: DEFAULT_MIN_SIZE,
            in_place: true,
            watch: false,
            groups: true,
        }
    }
}

impl FolderOpts {
    /// Step the threshold by one press of the sheet's −/+, clamped to its bounds.
    pub fn step_min_size(&mut self, delta: i32) {
        let steps = delta as i64;
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next
            .clamp(MIN_SIZE_FLOOR as i64, MIN_SIZE_CEIL as i64)
            as u64;
    }

    pub fn min_size_label(&self) -> String {
        if self.min_size.is_multiple_of(1024) {
            format!("{} KB", self.min_size / 1024)
        } else {
            format!("{:.1} KB", self.min_size as f64 / 1024.0)
        }
    }

    pub fn admits_file(&self, ext: &str, size: u64) -> bool {
        admits(self, ext, size)
    }

    /// The mode the two persisted switches add up to; callers ask this rather
    /// than testing the pair apart.
    pub fn mode(&self) -> FolderMode {
        FolderMode::from_opts(self)
    }
}
