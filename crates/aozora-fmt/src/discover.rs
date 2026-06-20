//! Resolve the positional path arguments into a concrete, ordered,
//! de-duplicated list of files — or stdin.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::cli::is_stdin;

/// The resolved input source.
pub(crate) enum Input {
    /// Read a single document from stdin.
    Stdin,
    /// A set of files discovered from the path arguments.
    Files(Resolved),
}

/// Files to process plus any non-fatal discovery errors (a `walkdir`
/// traversal error, say) accumulated rather than aborting the whole run.
#[derive(Default)]
pub(crate) struct Resolved {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) errors: Vec<String>,
}

/// Filename suffixes recognised as aozora sources during directory
/// recursion (matched case-insensitively).
const EXTENSIONS: &[&str] = &[".afm", ".aozora", ".aozora.txt"];

/// Classify the path arguments. Returns [`Input::Stdin`] when no paths are
/// given or the sole argument is `-`; mixing `-` with real paths is an error.
pub(crate) fn resolve(paths: &[PathBuf]) -> Result<Input> {
    if paths.is_empty() {
        return Ok(Input::Stdin);
    }
    if paths.iter().any(|p| is_stdin(p)) {
        if paths.len() > 1 {
            bail!("cannot mix stdin (`-`) with file paths");
        }
        return Ok(Input::Stdin);
    }

    let mut resolved = Resolved::default();
    for path in paths {
        collect(path, &mut resolved);
    }
    resolved.files.sort();
    resolved.files.dedup();
    Ok(Input::Files(resolved))
}

/// Add one argument's files to `resolved`: recurse directories, take other
/// paths verbatim. A missing path is pushed as-is so it surfaces as a read
/// error during processing — keeping all per-file error accounting in one
/// place rather than splitting it across discovery and processing.
#[allow(
    clippy::filetype_is_file,
    reason = "recursion intentionally processes regular files only; symlinks are skipped to match WalkDir's follow_links(false)"
)]
fn collect(path: &Path, resolved: &mut Resolved) {
    if !path.is_dir() {
        resolved.files.push(path.to_path_buf());
        return;
    }
    let walk = WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| !is_ignored(e));
    for entry in walk {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_aozora_source(entry.path()) => {
                resolved.files.push(entry.into_path());
            }
            Ok(_) => {}
            Err(err) => resolved.errors.push(err.to_string()),
        }
    }
}

/// Skip `target` and dotted entries (e.g. `.git`) below the explicitly
/// passed root; the root itself (depth 0) is never skipped.
fn is_ignored(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    name == "target" || name.starts_with('.')
}

/// True for filenames ending in a recognised aozora source extension,
/// e.g. `chapter.afm`, `本文.aozora`, `note.aozora.txt`.
fn is_aozora_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}
