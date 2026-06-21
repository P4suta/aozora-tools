//! Run summaries for `--stats`.
//!
//! The counts are accumulated during a run; rendering takes the elapsed
//! [`Duration`] as a parameter so tests pass a fixed value and assert the exact
//! string (no wall clock).

use std::time::Duration;

use serde::Serialize;

/// Accumulated counts for a `lint` run.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct LintStats {
    /// Inputs processed (post-discovery).
    pub files_scanned: usize,
    /// Inputs with no diagnostics.
    pub clean: usize,
    /// Inputs with at least one diagnostic.
    pub with_diagnostics: usize,
    /// Inputs that could not be read.
    pub errored: usize,
    /// ERROR-severity diagnostics across all inputs.
    pub errors: usize,
    /// WARNING-severity diagnostics across all inputs.
    pub warnings: usize,
}

/// The `stats` object embedded in `lint --json` output.
#[derive(Debug, Serialize)]
pub(crate) struct LintStatsJson {
    files_scanned: usize,
    clean: usize,
    with_diagnostics: usize,
    errored: usize,
    errors: usize,
    warnings: usize,
    elapsed_ms: u128,
}

impl LintStats {
    /// The one-line human summary printed to stderr.
    #[must_use]
    pub(crate) fn summary(&self, elapsed: Duration) -> String {
        format!(
            "aozora: scanned {} file{} in {} — {} clean, {} with diagnostics, \
             {} errored ({} error{}, {} warning{})",
            self.files_scanned,
            plural(self.files_scanned),
            humanise(elapsed),
            self.clean,
            self.with_diagnostics,
            self.errored,
            self.errors,
            plural(self.errors),
            self.warnings,
            plural(self.warnings),
        )
    }

    /// The machine-readable form for `--json`.
    #[must_use]
    pub(crate) fn to_json(self, elapsed: Duration) -> LintStatsJson {
        LintStatsJson {
            files_scanned: self.files_scanned,
            clean: self.clean,
            with_diagnostics: self.with_diagnostics,
            errored: self.errored,
            errors: self.errors,
            warnings: self.warnings,
            elapsed_ms: elapsed.as_millis(),
        }
    }
}

/// `""` for one, `"s"` otherwise — for naive English pluralisation.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Format a duration as `38ms` under a second, `1.4s` above.
fn humanise(elapsed: Duration) -> String {
    let ms = elapsed.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", elapsed.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_uses_fixed_elapsed_and_pluralises() {
        let stats = LintStats {
            files_scanned: 12,
            clean: 9,
            with_diagnostics: 3,
            errored: 0,
            errors: 5,
            warnings: 1,
        };
        let line = stats.summary(Duration::from_millis(38));
        assert_eq!(
            line,
            "aozora: scanned 12 files in 38ms — 9 clean, 3 with diagnostics, \
             0 errored (5 errors, 1 warning)"
        );
    }

    #[test]
    fn one_file_is_singular_and_seconds_format_past_1s() {
        let stats = LintStats {
            files_scanned: 1,
            clean: 1,
            ..LintStats::default()
        };
        let line = stats.summary(Duration::from_millis(1500));
        assert!(line.contains("scanned 1 file in 1.5s"), "{line}");
        assert!(line.contains("0 errors, 0 warnings"), "{line}");
    }

    #[test]
    fn json_carries_elapsed_ms() {
        let stats = LintStats {
            files_scanned: 2,
            ..LintStats::default()
        };
        let json = serde_json::to_value(stats.to_json(Duration::from_millis(7))).unwrap();
        assert_eq!(json["files_scanned"], 2);
        assert_eq!(json["elapsed_ms"], 7);
    }
}
