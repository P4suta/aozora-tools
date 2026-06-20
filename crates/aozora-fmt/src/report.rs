//! Output rendering: the aggregate [`Outcome`], the JSON `--json` report,
//! and coloured unified diffs.

use std::io::{self, Write};
use std::process::ExitCode;

use anstream::AutoStream;
use anstyle::{AnsiColor, Style};
use anyhow::Result;
use serde::Serialize;
use similar::{ChangeTag, DiffOp, TextDiff};

use crate::cli::ColorChoice;
use crate::discover::Resolved;
use crate::process;

/// The aggregate result of a run. Ordered so folding with `max` keeps the
/// most severe outcome: `Error` > `WouldReformat` > `Ok`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Outcome {
    /// Everything was already formatted (or written / listed) without error.
    Ok,
    /// `--check` found at least one input that would change.
    WouldReformat,
    /// An I/O error, missing path, parser panic, or guard failure occurred.
    Error,
}

impl Outcome {
    /// Map to the documented process exit code (0 / 1 / 2).
    pub(crate) fn exit_code(self) -> ExitCode {
        match self {
            Self::Ok => ExitCode::SUCCESS,
            Self::WouldReformat => ExitCode::from(1),
            Self::Error => ExitCode::from(2),
        }
    }
}

/// Build the stdout stream for diff output, honouring `--color`. `anstream`
/// strips ANSI when the choice (or TTY detection, for `auto`) says no colour.
pub(crate) fn auto_stdout(color: ColorChoice) -> AutoStream<io::Stdout> {
    match color {
        ColorChoice::Auto => AutoStream::auto(io::stdout()),
        ColorChoice::Always => AutoStream::always(io::stdout()),
        ColorChoice::Never => AutoStream::never(io::stdout()),
    }
}

/// Write a coloured unified diff of `old` → `new` under a `label` header.
pub(crate) fn write_diff(
    out: &mut impl Write,
    label: &str,
    old: &str,
    new: &str,
) -> io::Result<()> {
    let header = Style::new().bold();
    let meta = Style::new().fg_color(Some(AnsiColor::Cyan.into()));
    let del = Style::new().fg_color(Some(AnsiColor::Red.into()));
    let ins = Style::new().fg_color(Some(AnsiColor::Green.into()));

    writeln!(out, "{header}--- {label}{header:#}")?;
    writeln!(out, "{header}+++ {label}{header:#}")?;

    let diff = TextDiff::from_lines(old, new);
    for group in &diff.grouped_ops(3) {
        let (os, ol, ns, nl) = hunk_span(group);
        writeln!(out, "{meta}@@ -{os},{ol} +{ns},{nl} @@{meta:#}")?;
        for op in group {
            for change in diff.iter_changes(op) {
                let value = change.value();
                let line = value.strip_suffix('\n').unwrap_or(value);
                match change.tag() {
                    ChangeTag::Delete => writeln!(out, "{del}-{line}{del:#}")?,
                    ChangeTag::Insert => writeln!(out, "{ins}+{line}{ins:#}")?,
                    ChangeTag::Equal => writeln!(out, " {line}")?,
                }
            }
        }
    }
    Ok(())
}

/// 1-based `(old_start, old_len, new_start, new_len)` spanning a hunk group.
fn hunk_span(ops: &[DiffOp]) -> (usize, usize, usize, usize) {
    let (mut os, mut ns) = (usize::MAX, usize::MAX);
    let (mut oe, mut ne) = (0_usize, 0_usize);
    for op in ops {
        let (old, new) = (op.old_range(), op.new_range());
        os = os.min(old.start);
        oe = oe.max(old.end);
        ns = ns.min(new.start);
        ne = ne.max(new.end);
    }
    (os + 1, oe - os, ns + 1, ne - ns)
}

/// One entry in the `--json` report.
#[derive(Serialize)]
pub(crate) struct JsonFile {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl JsonFile {
    /// An already-formatted file.
    pub(crate) fn ok(path: String) -> Self {
        Self {
            path,
            status: "ok",
            message: None,
        }
    }

    /// A file that `--check` would reformat.
    pub(crate) fn would_reformat(path: String) -> Self {
        Self {
            path,
            status: "would_reformat",
            message: None,
        }
    }

    /// A file that could not be read or formatted.
    pub(crate) fn error(path: String, message: String) -> Self {
        Self {
            path,
            status: "error",
            message: Some(message),
        }
    }
}

#[derive(Serialize)]
struct JsonReport {
    version: u32,
    formatted: bool,
    files: Vec<JsonFile>,
}

/// Print the JSON report to stdout. `formatted` is true only when every
/// input was already canonical.
pub(crate) fn emit_json(outcome: Outcome, files: Vec<JsonFile>) -> io::Result<()> {
    let report = JsonReport {
        version: 1,
        formatted: outcome == Outcome::Ok,
        files,
    };
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, &report)?;
    out.write_all(b"\n")
}

/// `--check --json` over a resolved file set: collect every file's status
/// (including discovery errors) into one JSON object and return the outcome.
pub(crate) fn run_check_json(resolved: &Resolved) -> Result<Outcome> {
    let mut files = Vec::new();
    let mut outcome = Outcome::Ok;
    for err in &resolved.errors {
        files.push(JsonFile::error("<discovery>".to_owned(), err.clone()));
        outcome = Outcome::Error;
    }
    for path in &resolved.files {
        let label = path.display().to_string();
        match process::read_and_format(path) {
            Ok(fmt) if fmt.changed() => {
                files.push(JsonFile::would_reformat(label));
                outcome = outcome.max(Outcome::WouldReformat);
            }
            Ok(_) => files.push(JsonFile::ok(label)),
            Err(err) => {
                files.push(JsonFile::error(label, format!("{err:#}")));
                outcome = outcome.max(Outcome::Error);
            }
        }
    }
    emit_json(outcome, files)?;
    Ok(outcome)
}
