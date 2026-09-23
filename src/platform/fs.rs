//! The library's OS touch-points: walking a folder and measuring what it
//! holds, and the document gate every read passes through first.
//!
//! Raw IO and nothing else. Every decision — which files a folder admits,
//! what a rescan does about a file it has seen, where a book lands on a
//! shelf — is `library_core`'s, running here in the app where the library
//! state lives. This is the Tauri shell's `commands::library` walk, with
//! the IPC event emitter replaced by a [`ProgressSink`] the app turns into
//! a subscription: the throttle that kept a 2 000-file folder from
//! repainting a progress ring every syscall is the same throttle.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::hash::{HEAD_BYTES, mtime_ms};
use reader_core::format::{format_of, Format};
use library_core::paths;
use library_core::scan::FoundFile;
use library_core::wire::{ImportPhase, ImportProgress};

use super::progress::ProgressSink;

/// A tree deeper than this is either a loop this walk did not catch or a
/// directory nobody meant to import; either way the answer is to stop.
const MAX_DEPTH: usize = 12;

/// Past this the answer is larger than the state it would update, and the
/// honest response is to ask for a narrower folder.
const MAX_FOUND: usize = 20_000;

/// A 2 000-file folder would otherwise push 2 000 beats through the
/// channel in under a second, and the app would spend the import
/// repainting a ring.
const EMIT_EVERY: u32 = 8;
const EMIT_INTERVAL_MS: u128 = 60;

/// The suffixes this app reads, copies and opens — the format registry's
/// reach, spelled the way the Tauri shell's document gate spelled it. One
/// list serves the gate and the picker's filter, so they cannot drift.
pub const DOCUMENT_EXTENSIONS: &[&str] =
    &[".pdf", ".txt", ".text", ".md", ".markdown", ".mdown"];

/// True for a path this app may try to open: absolute, with a document
/// suffix. Also strips Windows shell quoting, which `std::env::args()`
/// leaves around a path Explorer handed over.
pub fn path_looks_absolute(path: &str) -> bool {
    path.starts_with('/')                  // POSIX
        || path.starts_with("\\\\")        // Windows UNC share
        || path.as_bytes().get(1) == Some(&b':') // Windows drive letter
}

/// The gate every read passes: an absolute path with a known document
/// suffix, or refused. The webview needed this because a general
/// file-read primitive behind IPC is a hole; the native app keeps the
/// promise it made — the reader touches documents, not the filesystem.
pub fn ensure_readable_document(path: &str) -> Result<(), String> {
    if !path_looks_absolute(path) {
        return Err(format!("refusing to read a non-absolute path: {path}"));
    }
    let lower = path.to_lowercase();
    if !DOCUMENT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err(format!("refusing to read a non-document file: {path}"));
    }
    Ok(())
}

/// Measure one document into the identity an import mints rows from: the
/// fingerprint the ledger keys on (size, stamp, head hash — the walk's own
/// measurement) and the format the address names.
pub fn measure_document(path: &str) -> Result<(Fingerprint, Format), String> {
    let file = Path::new(path);
    let meta = fs::metadata(file)
        .map_err(|e| format!("could not measure “{path}”: {e}"))?;
    if !meta.is_file() {
        return Err(format!("not a file: {path}"));
    }
    let fp = Fingerprint::of(
        meta.len(),
        mtime_ms(meta.modified().ok()),
        &read_head(file),
    );
    Ok((fp, format_of(path)))
}

struct Progress<'a> {
    task: String,
    phase: ImportPhase,
    done: u32,
    total: u32,
    since_emit: u32,
    last: Instant,
    sink: &'a ProgressSink,
}

impl<'a> Progress<'a> {
    fn new(task: &str, phase: ImportPhase, total: u32, sink: &'a ProgressSink) -> Self {
        Self {
            task: task.to_string(),
            phase,
            done: 0,
            total,
            since_emit: 0,
            last: Instant::now(),
            sink,
        }
    }

    /// A dropped beat is not an error: the next one carries the same
    /// totals and the final one is always flushed. The first file emits
    /// too — a three-file import that showed nothing until its final flush
    /// would read as a hang — and after that the throttle holds.
    fn tick(&mut self, name: &str) {
        self.done = self.done.saturating_add(1);
        self.since_emit = self.since_emit.saturating_add(1);
        if self.done > 1
            && self.since_emit < EMIT_EVERY
            && self.last.elapsed().as_millis() < EMIT_INTERVAL_MS
        {
            return;
        }
        self.flush(name);
    }

    fn flush(&mut self, name: &str) {
        self.since_emit = 0;
        self.last = Instant::now();
        (self.sink)(&ImportProgress {
            task: self.task.clone(),
            phase: self.phase,
            done: self.done,
            total: self.total,
            name: name.to_string(),
        });
    }
}

struct Scan<'a> {
    root: &'a Path,
    opts: &'a FolderOpts,
    progress: Progress<'a>,
    found: Vec<FoundFile>,
    truncated: bool,
}

impl Scan<'_> {
    fn walk(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH || self.truncated {
            return;
        }
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<fs::DirEntry> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if self.truncated {
                return;
            }
            // `file_type` does not follow the link, which is the point: a
            // symlinked directory is a loop this walk has no business
            // entering.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                self.walk(&path, depth + 1);
            } else if kind.is_file() {
                self.admit(entry, &path);
            }
        }
    }

    fn admit(&mut self, entry: fs::DirEntry, path: &Path) {
        let Ok(meta) = entry.metadata() else {
            return;
        };
        let size = meta.len();
        let ext = extension_of(path);
        if !self.opts.admits_file(&ext, size) {
            return;
        }
        let fp = Fingerprint::of(size, mtime_ms(meta.modified().ok()), &read_head(path));
        let root = self.root;
        let found = FoundFile {
            rel: relative_to(root, path),
            path: path_to_string(path),
            ext,
            size,
            fp,
        };
        self.found.push(found);
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.progress.tick(&label);
        if self.found.len() >= MAX_FOUND {
            self.truncated = true;
        }
    }
}

/// Walk `root` and answer with every document it holds, deepest `MAX_DEPTH`,
/// capped at `MAX_FOUND` — a folder past the cap answers with the advice to
/// import a narrower one. Runs on the executor's threads, so a large walk
/// never holds the UI; the beats flow through `sink` as they land.
pub fn scan(
    task: &str,
    root: &str,
    opts: &FolderOpts,
    sink: &ProgressSink,
) -> Result<Vec<FoundFile>, String> {
    let root_path = PathBuf::from(root);
    ensure_walkable(&root_path)?;
    let mut state = Scan {
        root: &root_path,
        opts,
        progress: Progress::new(task, ImportPhase::Scan, 0, sink),
        found: Vec::new(),
        truncated: false,
    };
    state.walk(&root_path, 0);
    if state.truncated {
        return Err(format!(
            "This folder has more than {MAX_FOUND} documents — try importing a smaller folder."
        ));
    }
    let total = state.found.len() as u32;
    state.progress.total = total;
    state.progress.done = total;
    state.progress.flush("");
    Ok(state.found)
}

/// Empty for a name Rust reads as having none (`Makefile`, `.gitignore`) —
/// which the format registry also refuses, so an extension-less file is
/// never admitted, measured or copied. The spelling is
/// [`library_core::paths::extension`]'s, so a `Path` here and a string in
/// the ledger answer the same.
fn extension_of(path: &Path) -> String {
    paths::extension(&path.to_string_lossy())
}

/// The subfolder half of this string is what a grouped import cuts its
/// shelves from, so it is normalised here rather than at three call sites.
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        .replace('\\', "/")
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// An unreadable head is not a failed scan: the size and the stamp still
/// identify the file, and a book that opens is worth more than a hash that
/// is exact.
fn read_head(path: &Path) -> Vec<u8> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(HEAD_BYTES as u64).read_to_end(&mut buf);
    buf
}

fn ensure_walkable(root: &Path) -> Result<(), String> {
    let text = root.to_string_lossy();
    if !path_looks_absolute(&text) {
        return Err(format!("refusing to walk a relative path: {text}"));
    }
    match fs::metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("not a directory: {text}")),
        Err(e) => Err(format!("cannot read {text}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_readable_document, extension_of, relative_to};
    use std::path::Path;

    #[test]
    fn an_extension_is_lower_case_and_has_no_dot() {
        assert_eq!(extension_of(Path::new("/books/Dune.PDF")), "pdf");
        assert_eq!(extension_of(Path::new("/books/notes.markdown")), "markdown");
        assert_eq!(extension_of(Path::new("/books/Makefile")), "");
        assert_eq!(extension_of(Path::new("/books/.gitignore")), "");
    }

    #[test]
    fn a_relative_path_always_uses_forward_slashes() {
        let root = Path::new("/books");
        assert_eq!(relative_to(root, Path::new("/books/a.pdf")), "a.pdf");
        assert_eq!(
            relative_to(root, Path::new("/books/scifi/deep/a.pdf")),
            "scifi/deep/a.pdf"
        );
        assert_eq!(relative_to(root, Path::new("/other/a.pdf")), "/other/a.pdf");
    }

    #[test]
    fn the_gate_reads_documents_and_refuses_everything_else() {
        ensure_readable_document("/Users/thiha/Documents/paper.pdf").unwrap();
        ensure_readable_document("C:\\Users\\thiha\\Desktop\\report.PDF").unwrap();
        ensure_readable_document("\\\\NAS\\books\\scan.pdf").unwrap();
        ensure_readable_document("/Users/thiha/notes.txt").unwrap();
        ensure_readable_document("/Users/thiha/notes.TEXT").unwrap();
        ensure_readable_document("/Users/thiha/README.md").unwrap();

        assert!(ensure_readable_document("books/dune.pdf").is_err(), "relative");
        assert!(ensure_readable_document("/Users/thiha/.ssh/id_rsa").is_err(), "not a document");
        assert!(ensure_readable_document("/Users/thiha/tool.exe").is_err(), "not a document");
        assert!(ensure_readable_document("/Users/thiha/Makefile").is_err(), "no extension");
        assert!(ensure_readable_document("").is_err(), "nothing at all");
    }
}
