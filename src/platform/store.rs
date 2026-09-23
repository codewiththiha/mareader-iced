//! The library's own roof: the store where the books it owns live, and the
//! sweep that takes them back.
//!
//! A stored book is a copy under the app's data directory —
//! `<data>/Library/items/<id>/source.<ext>`, the layout `library_core::store`
//! names and the Tauri shell built, so the two installs of the reader speak
//! one roof. The copy is the library's byte: removing the book removes it,
//! and the containment check below is the whole safety story — a delete that
//! trusted its argument would be `rm` with a function call around it.
//!
//! This is the shell's `store_books` and `delete_stored`, in-process: the
//! same gate, the same stamp, the same measurement riding home with the copy
//! so the row lands wearing its own identity and the folder that reads the
//! source keeps the source's fingerprint free.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use library_core::book::Fingerprint;
use library_core::hash::mtime_ms;
use library_core::paths;
use library_core::store;
use library_core::wire::{BookFileRequest, ImportPhase, StoreResult};

use super::fs::{ensure_readable_document, read_head};
use super::progress::{Emitter, ProgressSink};

/// The store's root: `<app data>/Library` — the roof the Tauri shell gave
/// the library's own copies, kept so the wire's spelling of the layout stays
/// true on both stacks.
pub fn store_root() -> PathBuf {
    super::data_dir().join("Library")
}

/// Copy one batch of books into the store, one [`StoreResult`] per request
/// in the order asked. A failure is per-file rather than per-batch: a folder
/// with one locked file in it still imports the other ninety-nine, plus an
/// answer naming the one that did not copy. The beats flow through `sink`
/// as the copies land — the same channel the walk's beats rode.
pub fn store_books(
    task: &str,
    requests: &[BookFileRequest],
    sink: &ProgressSink,
) -> Vec<StoreResult> {
    let mut progress = Emitter::new(task, ImportPhase::Copy, requests.len() as u32, sink);
    let mut out = Vec::with_capacity(requests.len());
    for request in requests {
        let name = paths::file_name(&request.from);
        out.push(copy_one(request, &mut progress, &name));
    }
    if !requests.is_empty() {
        progress.flush("");
    }
    out
}

fn copy_one(request: &BookFileRequest, progress: &mut Emitter<'_>, name: &str) -> StoreResult {
    let fail = |error: String| StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: String::new(),
        error: Some(error),
        measured: None,
    };
    if ensure_readable_document(&request.from).is_err() {
        return fail(format!("not a document this app may copy: {}", request.from));
    }
    let root = store_root();
    let items = store::items_root(&root.to_string_lossy());
    let ext = paths::extension(&request.from);
    let target = PathBuf::from(store::source_path(&items, &request.id, &ext));
    // The id is minted by the app and sanitized by `store`, but the promise
    // is checked where the byte is written, not where the name is trusted.
    if contained_in(&root, &target).is_none() {
        return fail(format!("refusing to write outside the store: {}", request.id));
    }
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the store directory: {e}"));
    }
    if let Err(e) = fs::copy(&request.from, &target) {
        return fail(format!("could not copy {}: {e}", request.from));
    }
    own_stamp(&target);
    progress.tick(name);
    StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: target.to_string_lossy().into_owned(),
        error: None,
        // Measured by the same pass that stamped it: the row lands wearing
        // its copy's own identity, and the folder that reads the source
        // keeps the source's free.
        measured: Some(measure_copy(&target)),
    }
}

/// The copy's own (size, mtime, first bytes) — the same triple
/// `check_paths` reads — taken here so the answer rides home with the copy
/// instead of costing a second trip.
fn measure_copy(target: &Path) -> Fingerprint {
    let Ok(meta) = fs::metadata(target) else {
        return Fingerprint::of(0, 0, &[]);
    };
    Fingerprint::of(meta.len(), mtime_ms(meta.modified().ok()), &read_head(target))
}

/// A copy owes the ledger a measurement of its own: a stored row is known by
/// its copy's fingerprint, so the source file's stays free for the folder
/// that reads it. A fresh stamp is what makes the two fingerprints differ.
fn own_stamp(target: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(target) {
        let _ = file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()));
    }
}

/// Remove a stored book's byte — and its item folder, which is the unit a
/// removal sweeps whole. Refuses any path outside the store, canonicalised
/// so a `..` cannot walk out. A file already gone is a success: the row
/// leaving is the fact that matters.
pub fn delete_stored(path: &str) -> Result<(), String> {
    let root = store_root();
    let Some(target) = contained_in(&root, Path::new(path)) else {
        return Err(format!("refusing to delete a file outside the store: {path}"));
    };
    match fs::remove_file(&target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("could not delete {path}: {e}")),
    }
    sweep_the_item_folder(&root, &target);
    Ok(())
}

/// A book's own item folder goes whole, and any other directory goes only
/// when the file was the last thing in it. Silent: a directory the host will
/// not release is an empty folder, not a removal that failed.
fn sweep_the_item_folder(root: &Path, deleted: &Path) {
    let Some(dir) = deleted.parent() else {
        return;
    };
    let Ok(root) = root.canonicalize() else {
        return;
    };
    if dir == root {
        return;
    }
    // The items root is resolved the same way before the comparison, because
    // a symlinked app-data directory would otherwise fail a match the
    // containment just proved.
    let items = PathBuf::from(store::items_root(&root.to_string_lossy()));
    let is_item_dir = dir.parent().is_some_and(|grandparent| {
        items
            .canonicalize()
            .map_or(*grandparent == items, |real| *grandparent == real)
    });
    if is_item_dir {
        let _ = fs::remove_dir_all(dir);
    } else if fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
        let _ = fs::remove_dir(dir);
    }
}

/// The nearest existing ancestor canonicalised, the rest resolved lexically
/// on top of it: `canonicalize` refuses a path that is not there, and a
/// target that always answered "outside" would refuse every copy the store
/// was asked to make.
fn contained_in(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let mut existing = path;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let base = loop {
        if let Ok(real) = existing.canonicalize() {
            break real;
        }
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return None;
        };
        tail.push(name);
        existing = parent;
    };
    let mut full = base;
    for name in tail.iter().rev() {
        if *name == std::ffi::OsStr::new("..") {
            full = full.parent()?.to_path_buf();
        } else if *name != std::ffi::OsStr::new(".") {
            full.push(name);
        }
    }
    full.starts_with(&root).then_some(full)
}
