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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use crate::folder::WatchedFolder;
    use crate::folder::kit::folder;
    use crate::folder::sanitize::sanitize;
    use crate::scan::selectable_formats;

    #[test]
    fn a_shape_answered_for_one_rung_cuts_the_rungs_under_it_alone() {
        let mut f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            shelf_map: BTreeMap::from([
                (String::new(), "root".to_string()),
                ("Fiction".to_string(), "fic".to_string()),
            ]),
            ..folder("/books")
        };
        assert_eq!(
            f.rung_for("Fiction/SciFi"),
            "",
            "one shelf files every address on its root rung"
        );
        assert_eq!(f.rungs_for("/books/Reference/x.pdf").0, Some("root"));
        // The nested folder the re-import answered for cuts its own rungs;
        // the tree above keeps the books never under it.
        f.set_shape("Fiction", true);
        assert_eq!(f.rung_for("Fiction"), "Fiction");
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        assert_eq!(f.rung_for("Reference"), "", "the rest of the tree is where it was");
        assert_eq!(
            f.rungs_for("/books/Fiction/SciFi/dune.pdf").0,
            None,
            "no shelf stands for the rung the answer cut yet"
        );
        assert_eq!(f.rungs_for("/books/Fiction/other.pdf").0, Some("fic"));
    }

    #[test]
    fn the_roots_answer_stands_for_the_whole_tree() {
        let mut f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            ..folder("/books")
        };
        f.set_shape("Fiction", true);
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        f.set_shape("", true);
        assert!(f.opts.groups, "the root's answer IS the folder's own shape");
        assert!(f.shapes.is_empty(), "and it takes every deeper answer with it");
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        f.set_shape("", false);
        assert_eq!(
            f.rung_for("Fiction/SciFi"),
            "",
            "one shelf puts every directory on the root rung"
        );
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_rung_for_every_file() {
        let f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            shelf_map: BTreeMap::from([
                ("".to_string(), "root".to_string()),
                ("Fiction".to_string(), "ignored".to_string()),
            ]),
            ..folder("/books")
        };
        assert_eq!(f.rungs_for("/books/Fiction/SciFi/dune.pdf"), (Some("root"), Some("root")));
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("root"), Some("root")));
    }

    #[test]
    fn the_defaults_are_the_ones_the_sheet_opens_on() {
        let o = FolderOpts::default();
        assert_eq!(o.min_size, DEFAULT_MIN_SIZE);
        assert_eq!(o.min_size_label(), "30 KB");
        assert!(o.include_selected);
        assert!(o.in_place, "read in place is the mode the app always had");
        assert!(!o.watch, "watching is opt-in");
        assert!(o.groups);
        assert_eq!(o.formats.len(), selectable_formats().len());
    }

    #[test]
    fn a_blob_from_before_the_folder_options_existed_loads_them() {
        let f: WatchedFolder =
            serde_json::from_str(r#"{"id":"f1","root":"/books"}"#).unwrap();
        assert_eq!(f.opts, FolderOpts::default());
        assert!(f.placed.is_empty() && f.ignored.is_empty());
        assert!(f.shelf_map.is_empty());
    }

    #[test]
    fn the_size_dial_steps_in_kb_and_stops_at_its_bounds() {
        let mut o = FolderOpts::default();
        o.step_min_size(1);
        assert_eq!(o.min_size, 40 * 1024);
        o.step_min_size(-2);
        assert_eq!(o.min_size, 20 * 1024);
        for _ in 0..100 {
            o.step_min_size(-1);
        }
        assert_eq!(o.min_size, MIN_SIZE_FLOOR);
        assert_eq!(o.min_size_label(), "0 KB");
        for _ in 0..200 {
            o.step_min_size(1);
        }
        assert_eq!(o.min_size, MIN_SIZE_CEIL);
        assert_eq!(o.min_size_label(), "500 KB");
    }

    #[test]
    fn a_sub_thousand_byte_threshold_still_prints_honestly() {
        let o = FolderOpts { min_size: 512, ..Default::default() };
        assert_eq!(o.min_size_label(), "0.5 KB");
    }

    #[test]
    fn sanitize_dedupes_roots_and_clamps_the_dial() {
        let mut folders = vec![
            WatchedFolder {
                opts: FolderOpts {
                    min_size: 10_000_000,
                    ..FolderOpts::default()
                },
                ..folder("/books")
            },
            folder("/books"),
            folder(""),
            WatchedFolder {
                id: " ".into(),
                ..folder("/other")
            },
        ];
        sanitize(&mut folders);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].opts.min_size, MIN_SIZE_CEIL);
    }

    #[test]
    fn an_empty_format_set_is_not_a_folder_that_admits_nothing() {
        // A hand-edited blob or a format removed from the registry must not
        // silently turn a watched folder into a dead one.
        let mut folders = vec![WatchedFolder {
            opts: FolderOpts {
                formats: BTreeSet::new(),
                ..FolderOpts::default()
            },
            ..folder("/books")
        }];
        sanitize(&mut folders);
        assert_eq!(folders[0].opts.formats.len(), selectable_formats().len());
    }
}
