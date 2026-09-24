//! The operating system's touch-points: where the app's data lives, how a
//! byte reaches the disk safely, the document gate, the folder walks and the
//! path checks behind them, the store the library's own copies live in, the
//! progress channel that carries the runs' beats to the Elm loop, the file
//! and folder pickers, and the PDF engine's shared library.
//!
//! The web app kept this layer in Tauri's `commands` crate and crossed an
//! IPC boundary to reach it; natively it is this module, called in-process
//! on the executor's threads through `Task::perform`. Same work, same
//! safety rules — no boundary to smuggle anything across, which is why the
//! document gate stays anyway: it is the reader's promise about which files
//! it touches, not an IPC artefact.

pub mod dialogs;
pub mod fs;
pub mod pdfium;
pub mod progress;
pub mod store;

/// The desktops this app ships on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

/// The desktop this process runs on. Anything unrecognised is treated as
/// Linux: frameless with the app's own caption cluster is the safe default.
///
/// Asked by the chrome (which cluster to draw) and by this layer's own file
/// handling (which reveal command exists), which is why it lives here rather
/// than with the chrome that was its first caller.
pub fn os() -> Os {
    match std::env::consts::OS {
        "macos" => Os::Mac,
        "windows" => Os::Windows,
        _ => Os::Linux,
    }
}

use std::io;
use std::path::{Path, PathBuf};

/// The app's data directory, following each OS's convention:
/// `~/.local/share/com.codewiththiha.mareader` on Linux,
/// `~/Library/Application Support/com.codewiththiha.mareader` on macOS,
/// `%APPDATA%\com\codewiththiha\mareader\data` on Windows — the identifier
/// the Tauri shell used, so the two installs of the reader never share a
/// store they would each write differently.
///
/// A platform with no discoverable data directory (which the three this
/// ships on do not have) falls back to a named folder in the temp
/// directory: reading still works, and nothing pretends the write is
/// durable that is not.
pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "codewiththiha", "mareader")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("com.codewiththiha.mareader"))
}

/// Milliseconds since the epoch — the stamp the library's ids, rows and
/// tombstones are minted with. A wall clock on purpose: these stamps are
/// persisted and read back on another run, so a monotonic counter would not
/// do, and a clock that steps backwards is clamped by every consumer's own
/// `saturating_sub`.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The persisted settings blob.
pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// The persisted library blob.
pub fn library_path() -> PathBuf {
    data_dir().join("library.json")
}

/// Write bytes so a crash mid-write cannot cost the old copy: a temporary
/// file beside the target, fully written, then one `rename` over it — the
/// atomic swap every POSIX and NTFS filesystem in the field provides.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        "{}.tmp",
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
