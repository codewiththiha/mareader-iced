//! The persisted state: the same schemas the web app kept in localStorage,
//! now JSON files in the app's data directory, written atomically.
//!
//! Two blobs, two files. `settings.json` carries the reader's
//! [`Settings`] — loaded through [`reader_core::settings::sanitize`], so a
//! blob written by any older build lands with the invariants the current
//! one expects. `library.json` carries the [`LibraryBlob`] — the books,
//! shelves, watched folders and the persisted view, in
//! `library_core`'s schema.
//!
//! The blob migrations (`library_core::blob::migrate`) stay ported and
//! tested in the crate that owns them, but the native loader does not run
//! them: the v1 and v2 shapes lived in the webview's localStorage, which
//! no native install has ever written. This app was born on the current
//! schema, and an unreadable `library.json` falls back to an empty shelf
//! rather than guessing.

use library_core::blob::LibraryBlob;
use reader_core::settings::{sanitize, Settings};
use std::path::Path;

use crate::platform;

/// The settings on disk, sanitized; a missing or unreadable file is a
/// fresh install's defaults.
pub fn load_settings() -> Settings {
    let mut settings = read(platform::settings_path())
        .and_then(|raw| serde_json::from_str::<Settings>(&raw).ok())
        .unwrap_or_default();
    sanitize(&mut settings);
    settings
}

/// The settings to disk, atomically. A failure is the caller's to surface:
/// the in-memory copy stays authoritative either way.
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json(&platform::settings_path(), settings)
}

/// The library to disk, atomically — the same change-time contract the
/// settings keep: every flow that moves the blob ends with this.
pub fn save_library(library: &LibraryBlob) -> Result<(), String> {
    write_json(&platform::library_path(), library)
}

/// The library on disk; a missing or unreadable file is an empty shelf.
pub fn load_library() -> LibraryBlob {
    read(platform::library_path())
        .and_then(|raw| serde_json::from_str::<LibraryBlob>(&raw).ok())
        .unwrap_or_default()
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the file".to_owned());
    let raw = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("could not serialise {name}: {e}"))?;
    platform::write_atomic(path, &raw)
        .map_err(|e| format!("could not write {name}: {e}"))
}
