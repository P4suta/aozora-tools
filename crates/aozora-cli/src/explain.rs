//! `aozora explain <code>` — long-form explanation of a diagnostic code.
//!
//! Accepts the bare (`unclosed-bracket`) or namespaced
//! (`aozora::unclosed-bracket`) form, a unique prefix, or — on a miss —
//! suggests the nearest code. With no argument it lists every code.

use std::io::{self, Write};
use std::mem;
use std::process::ExitCode;

use anstyle::{AnsiColor, Style};

use aozora_diagnostics::{CATALOGUE, CatalogueEntry, lookup};
use aozora_fmt::auto_stdout;

use crate::cli::ExplainArgs;

/// Run `aozora explain` and return the process exit code.
#[must_use]
pub(crate) fn run(args: &ExplainArgs) -> ExitCode {
    let mut out = auto_stdout(args.color);
    let result = match &args.code {
        None => list_all(&mut out),
        Some(code) => match find(code) {
            Found::One(entry) => print_entry(&mut out, entry),
            Found::Ambiguous(matches) => return ambiguous(code, &matches),
            Found::None(suggestion) => return unknown(code, suggestion),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("aozora explain: {err}");
            ExitCode::from(2)
        }
    }
}

/// The result of resolving a user-supplied code.
enum Found {
    One(&'static CatalogueEntry),
    Ambiguous(Vec<&'static str>),
    None(Option<&'static str>),
}

/// Resolve `input` to a catalogue entry: exact match, then unique prefix, then
/// a nearest-neighbour suggestion.
fn find(input: &str) -> Found {
    let normalized = normalize(input);
    if let Some(entry) = lookup(&normalized) {
        return Found::One(entry);
    }
    let needle = bare(&normalized);
    let prefix_matches: Vec<&'static CatalogueEntry> = CATALOGUE
        .iter()
        .filter(|entry| bare(entry.code).starts_with(needle))
        .collect();
    match prefix_matches.as_slice() {
        [one] => Found::One(one),
        [] => Found::None(nearest(needle)),
        many => Found::Ambiguous(many.iter().map(|entry| entry.code).collect()),
    }
}

/// Prefix an unqualified code with `aozora::`.
fn normalize(input: &str) -> String {
    if input.contains("::") {
        input.to_owned()
    } else {
        format!("aozora::{input}")
    }
}

/// Strip the `aozora::` namespace from a code.
fn bare(code: &str) -> &str {
    code.strip_prefix("aozora::").unwrap_or(code)
}

/// The nearest code by edit distance, if within a small threshold.
fn nearest(needle: &str) -> Option<&'static str> {
    CATALOGUE
        .iter()
        .map(|entry| (levenshtein(needle, bare(entry.code)), entry.code))
        .min_by_key(|(distance, _)| *distance)
        .filter(|(distance, _)| *distance <= 3)
        .map(|(_, code)| code)
}

/// Classic Levenshtein edit distance over Unicode scalar values.
fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn print_entry(out: &mut impl Write, entry: &CatalogueEntry) -> io::Result<()> {
    let code_style = Style::new().fg_color(Some(AnsiColor::Cyan.into()));
    let bold = Style::new().bold();
    writeln!(out, "{code_style}{}{code_style:#}", entry.code)?;
    writeln!(out)?;
    writeln!(out, "{bold}{}{bold:#}", entry.title)?;
    writeln!(out)?;
    writeln!(out, "{}", entry.explain)?;
    writeln!(out)?;
    writeln!(out, "  問題のある例:")?;
    writeln!(out, "    {}", entry.repro)?;
    writeln!(out)?;
    writeln!(out, "  直した例:")?;
    writeln!(out, "    {}", entry.fixed)
}

fn list_all(out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "診断コード一覧 (詳細は `aozora explain <code>`):")?;
    writeln!(out)?;
    for entry in CATALOGUE {
        writeln!(out, "  {:<38} {}", entry.code, entry.title)?;
    }
    Ok(())
}

fn unknown(input: &str, suggestion: Option<&'static str>) -> ExitCode {
    if let Some(code) = suggestion {
        eprintln!("aozora explain: unknown diagnostic code `{input}`; did you mean `{code}`?");
    } else {
        eprintln!("aozora explain: unknown diagnostic code `{input}`");
        eprintln!("run `aozora explain` to list every code");
    }
    ExitCode::from(2)
}

fn ambiguous(input: &str, matches: &[&'static str]) -> ExitCode {
    eprintln!("aozora explain: `{input}` is ambiguous; matches:");
    for code in matches {
        eprintln!("  {code}");
    }
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    fn one(input: &str) -> &'static CatalogueEntry {
        match find(input) {
            Found::One(entry) => entry,
            _ => panic!("expected a unique match for {input}"),
        }
    }

    #[test]
    fn bare_and_namespaced_forms_both_resolve() {
        assert_eq!(one("unclosed-bracket").code, "aozora::unclosed-bracket");
        assert_eq!(
            one("aozora::unclosed-bracket").code,
            "aozora::unclosed-bracket"
        );
    }

    #[test]
    fn unique_prefix_resolves() {
        assert_eq!(one("unclosed").code, "aozora::unclosed-bracket");
    }

    #[test]
    fn near_typo_suggests_nearest() {
        match find("unclosed-bracket-x") {
            Found::None(Some(code)) => assert_eq!(code, "aozora::unclosed-bracket"),
            other => panic!("expected a suggestion, got {:?}", DebugFound(&other)),
        }
    }

    #[test]
    fn far_garbage_has_no_suggestion() {
        match find("zzzzzzzzzzzzzzzz") {
            Found::None(None) => {}
            other => panic!("expected no suggestion, got {:?}", DebugFound(&other)),
        }
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn print_entry_includes_title_repro_and_fixed() {
        let entry = lookup("aozora::unclosed-bracket").expect("entry");
        let mut buf = Vec::new();
        print_entry(&mut buf, entry).expect("print");
        let text = String::from_utf8(buf).expect("utf8");
        assert!(text.contains("aozora::unclosed-bracket"), "code: {text}");
        assert!(text.contains(entry.title), "title: {text}");
        assert!(text.contains("問題のある例"), "repro heading: {text}");
        assert!(text.contains(entry.fixed), "fixed form: {text}");
    }

    #[test]
    fn list_all_lists_every_code() {
        let mut buf = Vec::new();
        list_all(&mut buf).expect("list");
        let text = String::from_utf8(buf).expect("utf8");
        for entry in CATALOGUE {
            assert!(text.contains(entry.code), "missing {}: {text}", entry.code);
        }
    }

    /// Tiny debug shim so the `panic!`s above can print which arm we hit.
    struct DebugFound<'a>(&'a Found);
    impl fmt::Debug for DebugFound<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Found::One(e) => write!(f, "One({})", e.code),
                Found::Ambiguous(m) => write!(f, "Ambiguous({m:?})"),
                Found::None(s) => write!(f, "None({s:?})"),
            }
        }
    }
}
