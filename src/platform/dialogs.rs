//! The file and folder pickers.
//!
//! `rfd`'s async dialogs run the platform's own sheet — AppKit's panel
//! dispatched to the main thread, Win32's COM dialog on the calling
//! thread, the XDG portal (with a zenity fallback) on a thread of its
//! own — so the future this hands to `Task::perform` never needs an async
//! runtime to host it and never blocks the UI thread. The document
//! picker's filter is the gate's own extension list: what the dialog
//! offers and what the app refuses cannot drift apart.

use std::path::PathBuf;

use iced::Task;

use super::fs::DOCUMENT_EXTENSIONS;

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
    Task::perform(pick_files_async(), picked)
}

async fn pick_folder_async() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|handle| handle.path().to_path_buf())
}

async fn pick_files_async() -> Option<Vec<PathBuf>> {
    let extensions: Vec<&str> =
        DOCUMENT_EXTENSIONS.iter().map(|ext| &ext[1..]).collect();
    rfd::AsyncFileDialog::new()
        .add_filter("Documents", &extensions)
        .pick_files()
        .await
        .map(|handles| handles.iter().map(|h| h.path().to_path_buf()).collect())
}
