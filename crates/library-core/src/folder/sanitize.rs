//! What a folder has to satisfy to stay in the library: a root and an id, one
//! folder per root, and every field inside the bounds it is read with.

use std::collections::HashSet;

use crate::tracking::Track;
use super::{default_formats, MIN_SIZE_CEIL, MIN_SIZE_FLOOR, WatchedFolder};

/// Drop rows with no id or no root, dedupe by root (first wins), clamp the
/// size threshold, and refill a format set that would admit nothing.
/// Idempotent.
pub fn sanitize(folders: &mut Vec<WatchedFolder>) {
    let mut seen = HashSet::new();
    folders.retain(|f| {
        !f.id.trim().is_empty() && !f.root.trim().is_empty() && seen.insert(f.root.clone())
    });
    for f in folders.iter_mut() {
        f.opts.min_size = f
            .opts
            .min_size
            .clamp(MIN_SIZE_FLOOR, MIN_SIZE_CEIL);
        if f.opts.formats.is_empty() {
            f.opts.formats = default_formats();
        }
        // The legacy flag is carried into the rung tree the first time this
        // build sees the folder, and kept equal to the tree's root so the two
        // cannot drift. A watch on a copying folder is left alone: the source
        // may still gain a file worth copying.
        if f.tracking.is_empty() && f.opts.watch {
            f.tracking.set("", Track::On);
        }
        f.opts.watch = f.tracking.tracked();
        f.shelf_map.retain(|k, v| !v.trim().is_empty() && !k.contains('\\'));
        // One tombstone per fingerprint: a book removed twice must not offer
        // the same file back from two rows.
        let mut stones = HashSet::new();
        f.ignored.retain(|t| !t.last_path.trim().is_empty() && stones.insert(t.fp));
        let mut seen = HashSet::new();
        f.last_seen.retain(|(fp, path)| !path.trim().is_empty() && seen.insert(*fp));
    }
}
