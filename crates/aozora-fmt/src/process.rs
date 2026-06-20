//! Per-file work: read, format (panic-guarded), and the in-place write with
//! the idempotency guard.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, set_hook, take_hook};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use crate::format_source;

/// A formatted file: the original bytes and the canonical form.
pub(crate) struct Formatted {
    pub(crate) old: String,
    pub(crate) new: String,
}

impl Formatted {
    /// True when canonicalisation changed the source.
    pub(crate) fn changed(&self) -> bool {
        self.old != self.new
    }
}

/// Read `path` and canonicalise it (panic-guarded).
pub(crate) fn read_and_format(path: &Path) -> Result<Formatted> {
    let old = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let new = format_guarded(&old)?;
    Ok(Formatted { old, new })
}

/// Rewrite `path` with its canonical form if it changed, upholding the
/// formatter's idempotency contract: refuse to write when a second pass
/// differs rather than corrupt the file.
pub(crate) fn write_back(path: &Path, fmt: &Formatted) -> Result<()> {
    if !fmt.changed() {
        return Ok(());
    }
    let reformatted = format_guarded(&fmt.new)?;
    if reformatted != fmt.new {
        bail!(
            "refusing to overwrite {}: formatting is not idempotent for this \
             input (a second pass changes the output). This is a bug — please \
             report it. The file was left unchanged.",
            path.display()
        );
    }
    fs::write(path, &fmt.new).with_context(|| format!("writing {}", path.display()))
}

/// Format `source`, converting an upstream parser panic into a clean error
/// instead of a process abort (via `panic = "unwind"` and `catch_unwind`).
/// In `--write` mode this guarantees no file is touched after a panic.
pub(crate) fn format_guarded(source: &str) -> Result<String> {
    // Silence the default hook so a caught panic doesn't also print
    // "thread 'main' panicked …"; we report it ourselves below.
    let prev_hook = take_hook();
    set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| format_source(source)));
    set_hook(prev_hook);
    result.map_err(|_| {
        anyhow!(
            "the formatter panicked while processing this input; no files were \
             modified. This is a bug — please report it at \
             https://github.com/P4suta/aozora-tools/issues"
        )
    })
}
