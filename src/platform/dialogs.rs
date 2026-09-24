//! The file and folder pickers.
//!
//! `rfd`'s async dialogs run the platform's own sheet — AppKit's panel
//! dispatched to the main thread, Win32's COM dialog on the calling
//! thread, the XDG portal (with a zenity fallback) on a thread of its
//! own — so the future this hands to `Task::perform` never needs an async
//! runtime to host it and never blocks the UI thread. The document
//! picker's filter is the gate's own extension list: what the dialog
//! offers and what the app refuses cannot drift apart.
//!
//! A picker is also the one focus event the app causes itself, and the
//! focus-rescan that event would trigger walks the import behind the
//! picker is about to run anyway. The grace below is the web app's
//! `picker_focus`, with the `Cooldown` rule the crate already tested.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use iced::Task;
use library_core::id::Cooldown;

use super::fs::DOCUMENT_EXTENSIONS;
use super::now_ms;

/// How long after a picker closes its focus still counts as the app's own.
const PICKER_GRACE_MS: u64 = 1_000;

static PICKER_OPEN: AtomicBool = AtomicBool::new(false);

fn picker_closed() -> &'static Mutex<Cooldown> {
    static CELL: OnceLock<Mutex<Cooldown>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(Cooldown::new(PICKER_GRACE_MS)))
}

/// Whether the focus the window just regained is the app's own picker
/// closing rather than a reader coming back. The measure pass runs either
/// way — a book whose file died while a dialog was up is a book the library
/// should know about; it is the folder walks that wait.
pub fn picker_focus() -> bool {
    if PICKER_OPEN.load(Ordering::Relaxed) {
        return true;
    }
    picker_closed()
        .lock()
        .is_ok_and(|guard| guard.within(now_ms()))
}

/// Mark the grace around one dialog: open before the sheet goes up, closed
/// and armed the moment it comes down.
struct PickerGuard;

impl PickerGuard {
    fn new() -> Self {
        PICKER_OPEN.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for PickerGuard {
    fn drop(&mut self) {
        PICKER_OPEN.store(false, Ordering::Relaxed);
        if let Ok(mut guard) = picker_closed().lock() {
            guard.arm(now_ms());
        }
    }
}

/// Ask for one folder. The answer is the picked path, or `None` when the
/// sheet was dismissed.
pub fn pick_folder<M>(picked: fn(Option<PathBuf>) -> M) -> Task<M>
where
    M: Send + 'static,
{
    Task::perform(pick_folder_async(), picked)
}

/// Ask for several documents at once — the shelf's "Choose files…" door.
/// The answer is the picked paths, or `None` when the sheet was dismissed.
pub fn pick_files<M>(picked: fn(Option<Vec<PathBuf>>) -> M) -> Task<M>
where
    M: Send + 'static,
{
    Task::perform(pick_files_async(None), picked)
}

/// Ask for several documents, opening the sheet inside `root` — the
/// "choose files from this folder" door of a watched shelf, where the
/// level the reader stands on is the place to browse to.
pub fn pick_files_in<M>(root: String, picked: fn(Option<Vec<PathBuf>>) -> M) -> Task<M>
where
    M: Send + 'static,
{
    Task::perform(pick_files_async(Some(root)), picked)
}

async fn pick_folder_async() -> Option<PathBuf> {
    pick_folder_in_async(String::new()).await
}

async fn pick_folder_in_async(root: String) -> Option<PathBuf> {
    let _guard = PickerGuard::new();
    let mut dialog = rfd::AsyncFileDialog::new();
    if !root.is_empty() {
        dialog = dialog.set_directory(&root);
    }
    dialog.pick_folder().await.map(|handle| handle.path().to_path_buf())
}

async fn pick_files_async(root: Option<String>) -> Option<Vec<PathBuf>> {
    let _guard = PickerGuard::new();
    let extensions: Vec<&str> =
        DOCUMENT_EXTENSIONS.iter().map(|ext| &ext[1..]).collect();
    let mut dialog = rfd::AsyncFileDialog::new().add_filter("Documents", &extensions);
    if let Some(root) = root
        && !root.is_empty()
    {
        dialog = dialog.set_directory(&root);
    }
    dialog
        .pick_files()
        .await
        .map(|handles| handles.iter().map(|h| h.path().to_path_buf()).collect())
}
