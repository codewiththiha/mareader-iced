//! Watching a shelf's ground: the tree's own row for it, the facts a menu reads,
//! and the promise the toggle keeps.

use crate::app::Mareader;

use super::Covered;
use crate::app::message::Message;
use crate::app::walk::{Asked, GroundWatch, RootPlan, ShelfWatch};
use crate::chrome::icons::IconName;
use crate::library::menus;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::folder::{self as folder_ops};
use library_core::governance::Governance;
use library_core::paths;
use library_core::shelf;
use std::path::PathBuf;

impl Mareader {
    /// Which tree governs a picked ground, and the rung the ground names
    /// in it: the covering tree's own seat when a rung shelf stands, else
    /// the family the directory's address belongs to — a rung whose shelf
    /// was deleted or departed as a copy still seeds from its family's
    /// answers. `None` for a ground no tree owns.
    pub(super) fn ground_tracking(&self, root: &str) -> Option<GroundWatch> {
        let governance = Governance::new(&self.library.folders, &self.library.shelves);
        let (tree_id, rung) = match governance.covering(root) {
            Some(covered) => (covered.folder_id, covered.rel),
            None => shelf::family_for(&self.library.folders, &self.library.shelves, root)?,
        };
        let row = folder_ops::find(&self.library.folders, &tree_id)?;
        let mut opts = row.opts.clone();
        // The structure answer is the rung's own, not the tree's root:
        // per-rung shapes are the whole of what a subfolder import asks.
        opts.groups = row.shape_at(&rung);
        let on = row.tracks_rung(&rung);
        Some(GroundWatch { tree_id, rung, on, opts })
    }

    /// The sheet's watch answer, written onto the rung it belongs to —
    /// never onto the tree's root — and persisted when it moved.
    pub(in crate::app) fn write_rung_tracking(&mut self, watch: &GroundWatch) -> Task<Message> {
        let moved = match folder_ops::find_mut(&mut self.library.folders, &watch.tree_id) {
            Some(folder) if folder.tracks_rung(&watch.rung) != watch.on => {
                folder.set_tracking(&watch.rung, watch.on);
                true
            }
            _ => false,
        };
        if moved {
            self.persist_library()
        } else {
            Task::none()
        }
    }

    /// The covered walk's seat: the rung shelf the ground names, its own
    /// name, and the tree's root — everything the landing needs to answer
    /// the pick back to the shelf it meant.
    pub(in crate::app) fn covered_shelf(&self, root: &str) -> Option<Covered> {
        let governance = Governance::new(&self.library.folders, &self.library.shelves);
        let covered = governance.covering(root)?;
        let tree_root =
            folder_ops::find(&self.library.folders, &covered.folder_id)?.root.clone();
        let shelf_name = shelf::find(&self.library.shelves, &covered.shelf_id)
            .map(|each| each.name.clone())
            .unwrap_or_else(|| paths::dir_label(&tree_root));
        Some(Covered {
            tree_root,
            shelf_id: covered.shelf_id,
            shelf_name,
        })
    }

    /// The watch seat a folder shelf answers for. `None` when the shelf is
    /// not a folder's seat — the toggle row the folder context menu shows
    /// only for one that is.
    fn shelf_watch(&self, shelf_id: &str) -> Option<ShelfWatch> {
        let seat =
            Governance::new(&self.library.folders, &self.library.shelves).seat_of(shelf_id)?;
        let folder = folder_ops::find(&self.library.folders, &seat.folder_id)?;
        let rung_label = (!seat.rung.is_empty())
            .then(|| paths::dir_label(&folder_ops::dir_of_rung(&folder.root, &seat.rung)));
        Some(ShelfWatch {
            on: folder.tracks_rung(&seat.rung),
            label: paths::dir_label(&folder.root),
            rung_label,
            folder_id: seat.folder_id,
            rung: seat.rung,
        })
    }

    /// The folder context menu's watch row, in the seat's own words. A
    /// deep seat answers for its rung alone; a root seat answers for the
    /// whole tree — and the glyph flips with the answer, because "stop
    /// watching" and "watch" are two different promises.
    pub(in crate::app) fn watch_facts(&self, shelf_id: &str) -> Option<menus::WatchFacts> {
        let watch = self.shelf_watch(shelf_id)?;
        let deep = watch.rung_label.is_some();
        let (glyph, label) = match (watch.on, deep) {
            (true, true) => (IconName::EyeOff, "Stop watching this subfolder"),
            (true, false) => (IconName::EyeOff, "Stop watching for new books"),
            (false, true) => (IconName::Eye, "Watch this subfolder for new books"),
            (false, false) => (IconName::Eye, "Watch for new books"),
        };
        let sublabel = match &watch.rung_label {
            Some(rung) => format!("Only “{rung}” and the folders inside it"),
            None => format!("The whole “{}” folder", watch.label),
        };
        Some(menus::WatchFacts {
            icon: glyph,
            label: label.to_string(),
            sublabel,
            message: Message::ToggleWatch(shelf_id.to_string()),
        })
    }

    /// The watch row's click: flip the seat's rung — a root seat flips the
    /// whole tree — persist, tell the reader what the answer now is, and,
    /// turning the watch on, walk the tree now rather than at the next
    /// focus: a promise made is a promise kept from the moment it is made.
    pub(in crate::app) fn toggle_watch(&mut self, shelf_id: &str) -> Task<Message> {
        let Some(watch) = self.shelf_watch(shelf_id) else {
            return Task::none();
        };
        let on = !watch.on;
        let Some((root, opts)) = folder_ops::find_mut(&mut self.library.folders, &watch.folder_id)
            .map(|folder| {
                if watch.rung.is_empty() {
                    folder.set_tracking_whole(on);
                } else {
                    folder.set_tracking(&watch.rung, on);
                }
                (folder.root.clone(), folder.opts.clone())
            })
        else {
            return Task::none();
        };
        let ground = match &watch.rung_label {
            Some(rung) => format!("“{rung}” in {}", watch.label),
            None => watch.label,
        };
        let persisted = self.persist_library();
        let line = if on {
            format!("Watching {ground} for new books.")
        } else {
            format!("{ground} is no longer watched for new books.")
        };
        self.toasts.show(Tone::Info, line, Instant::now());
        if on {
            Task::batch([
                persisted,
                self.begin_folder_walk(PathBuf::from(root), opts, Asked::OnFocus, RootPlan::default()),
            ])
        } else {
            persisted
        }
    }
}
