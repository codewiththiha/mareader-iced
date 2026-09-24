//! Where the PDF engine's shared library comes from.
//!
//! pdfium-render binds at run time, so the app never links Pdfium and never
//! fails to build without it — the question "is there an engine?" is answered
//! when the engine thread starts, and a "no" is a state the app lives in
//! rather than a crash. The web app's engine was a vendor of pdf.js inside the
//! webview: either the bundle shipped it or the reader had no engine at all.
//! Natively the library is a file on disk, and this module is the one place
//! that decides which file.
//!
//! **The bind happens once per process.** pdfium-render keeps its bindings in
//! a process-global cell: the first `bind_*` call fills it, `Pdfium::new`
//! initialises Pdfium's own library from it, and every later bind is refused
//! with `PdfiumLibraryBindingsAlreadyInitialized`. That is why the engine
//! thread owns the binding (see `crate::formats::pdf::engine`) and why this
//! module hands back the bound [`Pdfium`] rather than a path: there is exactly
//! one of these in a run, and nothing else may call it.
//!
//! Order of search, most deliberate first:
//!
//! 1. `MAREAEDER_PDFIUM` — a full path to the library file itself.
//! 2. `MAREAEDER_PDFIUM_DIR` — the directory holding it, which is what CI
//!    sets when it unpacks a build. Deliberately *not* pdfium-render's own
//!    `PDFIUM_DYNAMIC_LIB_PATH`: that variable is read by the crate's build
//!    script, which passes it to the linker under the `static`/`link`
//!    features. For a bind that happens at run time it would be a false
//!    friend — set, believed, and read by nobody.
//! 3. Beside the executable: its own directory, then `lib/`, then `bin/` —
//!    the three shapes the release archives use (`lib/libpdfium.so`,
//!    `lib/libpdfium.dylib`, `bin/pdfium.dll`). The release bundle's sidecar
//!    lands here.
//! 4. The working directory, same three.
//! 5. The system library, by name.
//!
//! A library that exists but refuses to load does not end the search: a stale
//! `libpdfium.so` beside an executable must not shadow the system's working
//! one. Every candidate is reported in the failure sentence, because "no
//! engine" without the list of places that were looked is a sentence nobody
//! can act on.

use std::path::PathBuf;

use pdfium_render::prelude::Pdfium;

/// The variable naming the library FILE, for a dev tree or a package that
/// keeps Pdfium somewhere of its own.
pub const FILE_VAR: &str = "MAREAEDER_PDFIUM";

/// The variable naming the library's DIRECTORY: one export on a development
/// machine or a CI runner, instead of an absolute path per machine.
pub const DIR_VAR: &str = "MAREAEDER_PDFIUM_DIR";

/// The directories to search, in order: the caller supplies the two ambient
/// facts (the executable's directory, the working directory) and the two
/// environment answers, so this list is a pure function the tests can walk.
pub fn candidate_dirs(
    env_dir: Option<PathBuf>,
    exe_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = env_dir {
        dirs.push(dir);
    }
    for base in [exe_dir, cwd].into_iter().flatten() {
        dirs.push(base.clone());
        dirs.push(base.join("lib"));
        dirs.push(base.join("bin"));
    }
    dirs
}

/// Bind the process to a Pdfium library, or explain why it could not.
///
/// Called once, by the engine thread, before it answers anything.
pub fn bind() -> Result<Pdfium, String> {
    let mut tried: Vec<PathBuf> = Vec::new();

    // 1. The explicit file. Its name is the reader's to get right, so a wrong
    // one is reported as the path it was rather than searched past.
    if let Some(file) = std::env::var_os(FILE_VAR) {
        let path = PathBuf::from(file);
        match Pdfium::bind_to_library(&path) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(_) => tried.push(path),
        }
    }

    // 2-4. Directories, by the name the platform's loader expects.
    let dirs = candidate_dirs(
        std::env::var_os(DIR_VAR).map(PathBuf::from),
        std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)),
        std::env::current_dir().ok(),
    );
    for dir in dirs {
        let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
        // Absent is the common case and says nothing; present-but-unloadable
        // is worth the same sentence as a failed bind, and the path list
        // carries it either way.
        if !path.exists() {
            tried.push(path);
            continue;
        }
        match Pdfium::bind_to_library(&path) {
            Ok(bindings) => return Ok(Pdfium::new(bindings)),
            Err(_) => tried.push(path),
        }
    }

    // 5. Whatever the system's loader can find by name.
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => Ok(Pdfium::new(bindings)),
        Err(error) => Err(unavailable(&tried, &error.to_string())),
    }
}

/// The sentence the reader is shown, and the one the log keeps.
fn unavailable(tried: &[PathBuf], last: &str) -> String {
    let places = tried
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if places.is_empty() {
        return format!(
            "The PDF engine is not available on this machine, so PDFs cannot be \
             opened ({last}). Reading settings, the shelf and the text formats all \
             still work."
        );
    }
    format!(
        "The PDF engine is not available on this machine, so PDFs cannot be opened \
         ({last}). Looked for the Pdfium library at: {places}. Reading settings, the \
         shelf and the text formats all still work."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_order_is_env_then_beside_then_underfoot() {
        let dirs = candidate_dirs(
            Some(PathBuf::from("/opt/pdfium")),
            Some(PathBuf::from("/apps/mareader")),
            Some(PathBuf::from("/work")),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/opt/pdfium"),
                PathBuf::from("/apps/mareader"),
                PathBuf::from("/apps/mareader/lib"),
                PathBuf::from("/apps/mareader/bin"),
                PathBuf::from("/work"),
                PathBuf::from("/work/lib"),
                PathBuf::from("/work/bin"),
            ]
        );
    }

    #[test]
    fn a_missing_ambient_directory_drops_its_whole_group() {
        // No executable directory (an odd platform) must not turn into three
        // empty joins that each get tried as a path.
        let dirs = candidate_dirs(None, None, Some(PathBuf::from("/work")));
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/work"),
                PathBuf::from("/work/lib"),
                PathBuf::from("/work/bin"),
            ]
        );
    }

    #[test]
    fn the_platform_names_the_library_the_loader_would() {
        // The three shapes the release archives use; a bundle that lays the
        // library down under its own name is found by this, and a bundle that
        // renames it is found by MAREAEDER_PDFIUM.
        let name = Pdfium::pdfium_platform_library_name();
        let name = name.to_string_lossy();
        #[cfg(target_os = "windows")]
        assert_eq!(name, "pdfium.dll");
        #[cfg(target_os = "macos")]
        assert_eq!(name, "libpdfium.dylib");
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(name, "libpdfium.so");
        let _ = name;
    }

    #[test]
    fn a_failure_sentence_names_every_place_that_was_looked() {
        let tried = vec![PathBuf::from("/apps/mareader/libpdfium.so")];
        let sentence = unavailable(&tried, "cannot open shared object file");
        assert!(sentence.contains("/apps/mareader/libpdfium.so"));
        assert!(sentence.contains("still work"));
    }
}
