//! Terminal rendering of a [`Described`] diagnostic.
//!
//! Uses `annotate-snippets` (rustc's renderer) for the full caret-underlined
//! form, with a terse `--quiet` one-liner alongside. Colour is delegated to the
//! `anstream` stream the caller passes (built from `--color`/`NO_COLOR`): the
//! renderer always emits styled output, and a non-colour stream strips it.

use std::io::{self, Write};
use std::ops::Range;

use annotate_snippets::{Level, Renderer, Snippet};
use aozora_diagnostics::{Described, Severity};

/// A diagnostic plus the source it points into, ready to render.
pub(crate) struct TermDiag<'a> {
    /// Display label for the source (`本文.afm` or `<stdin>`).
    pub label: &'a str,
    /// The full source text the span indexes into.
    pub source: &'a str,
    /// The diagnostic to render.
    pub diag: &'a Described,
}

/// Render `d` as a rustc-style annotated snippet, followed by a blank line.
///
/// `out` should be an `anstream` stream so `--color`/`NO_COLOR` controls ANSI;
/// the renderer itself always emits styled output.
pub(crate) fn render(out: &mut impl Write, d: &TermDiag<'_>) -> io::Result<()> {
    let level = level_of(d.diag.severity);
    let (title, body) = split_message(&d.diag.message);
    let explain_hint = format!("run `aozora explain {}`", d.diag.code);
    let span = clamp_span(
        d.diag.span.start as usize,
        d.diag.span.end as usize,
        d.source.len(),
    );

    let mut message = level.title(title).snippet(
        Snippet::source(d.source)
            .line_start(1)
            .origin(d.label)
            .fold(true)
            .annotation(level.span(span)),
    );
    if !body.is_empty() {
        message = message.footer(Level::Note.title(&body));
    }
    message = message
        .footer(Level::Note.title(d.diag.code))
        .footer(Level::Help.title(&explain_hint));

    // `{}\n` + writeln's own newline ⇒ one blank line between diagnostics.
    writeln!(out, "{}\n", Renderer::styled().render(message))
}

/// Render `d` as one terse line: `label:line:col: severity[code]: <summary>`.
/// This is the `--quiet` / grep-friendly / CI-annotation form.
pub(crate) fn render_quiet(out: &mut impl Write, d: &TermDiag<'_>) -> io::Result<()> {
    let (line, col) = line_col(d.source, d.diag.span.start as usize);
    let summary = d.diag.message.split('\n').next().unwrap_or("");
    writeln!(
        out,
        "{}:{line}:{col}: {}[{}]: {summary}",
        d.label,
        severity_word(d.diag.severity),
        d.diag.code,
    )
}

fn level_of(severity: Severity) -> Level {
    match severity {
        Severity::Error => Level::Error,
        Severity::Warning => Level::Warning,
    }
}

/// The lowercase severity word used in the `--quiet` line.
pub(crate) fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// Split a diagnostic message into its headline (first line, used as the
/// snippet title) and body (the remaining 何が問題 / どう直す prose, emitted as
/// a footer note).
fn split_message(message: &str) -> (&str, String) {
    match message.split_once('\n') {
        Some((first, rest)) => (first, rest.trim().to_owned()),
        None => (message, String::new()),
    }
}

/// Clamp a byte span to `0..=len`, defensively (the unknown-diagnostic catalogue
/// entry uses `0..0`, and a span must lie within the rendered source).
fn clamp_span(start: usize, end: usize, len: usize) -> Range<usize> {
    let start = start.min(len);
    let end = end.clamp(start, len);
    start..end
}

/// 1-based `(line, column)` of `byte` within `source`, where the column counts
/// Unicode scalar values (not display width or UTF-16 units) — the measure
/// editors use to jump to a position.
pub(crate) fn line_col(source: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (offset, ch) in source.char_indices() {
        if offset >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aozora_diagnostics::describe_source;

    /// Render `d` through a colour-stripping stream so the assertions see plain
    /// text regardless of the styled ANSI the renderer emits. `quiet` selects
    /// the one-line form.
    fn render_to_string(d: &TermDiag<'_>, quiet: bool) -> String {
        let mut stream = anstream::AutoStream::never(Vec::new());
        if quiet {
            render_quiet(&mut stream, d).expect("render");
        } else {
            render(&mut stream, d).expect("render");
        }
        String::from_utf8(stream.into_inner()).expect("utf8")
    }

    #[test]
    fn unclosed_bracket_renders_header_caret_code_and_help() {
        let source = "本文［＃改ページ";
        let diags = describe_source(source);
        let diag = diags.first().expect("a diagnostic");
        let text = render_to_string(
            &TermDiag {
                label: "本文.afm",
                source,
                diag,
            },
            false,
        );
        assert!(text.contains("error"), "severity word: {text}");
        assert!(text.contains("本文.afm:1:3"), "location header: {text}");
        assert!(text.contains('^'), "caret underline: {text}");
        assert!(
            text.contains("aozora::unclosed-bracket"),
            "code note: {text}"
        );
        assert!(
            text.contains("aozora explain aozora::unclosed-bracket"),
            "explain hint: {text}"
        );
    }

    #[test]
    fn quiet_form_is_one_line_with_location_and_code() {
        let source = "本文［＃改ページ";
        let diags = describe_source(source);
        let diag = diags.first().expect("a diagnostic");
        let text = render_to_string(
            &TermDiag {
                label: "本文.afm",
                source,
                diag,
            },
            true,
        );
        assert_eq!(text.lines().count(), 1, "quiet is one line: {text}");
        assert!(
            text.starts_with("本文.afm:1:3: error[aozora::unclosed-bracket]: "),
            "quiet line shape: {text}",
        );
    }

    #[test]
    fn line_col_counts_scalar_values_across_newlines() {
        let source = "あ\nいう";
        // byte 0 = 'あ' → line 1 col 1
        assert_eq!(line_col(source, 0), (1, 1));
        // 'あ' is 3 bytes; byte 4 = start of 'い' (after the newline) → line 2 col 1
        assert_eq!(line_col(source, 4), (2, 1));
        // byte 7 = 'う' → line 2 col 2
        assert_eq!(line_col(source, 7), (2, 2));
    }
}
