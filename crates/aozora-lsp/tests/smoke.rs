//! Smoke tests for the pure helper surface of `aozora-lsp`. The tower-lsp
//! backend itself is not wired up here — in-process RPC testing needs
//! a tokio runtime with stdin/stdout plumbing and is cheaper to cover
//! by spawning the binary from an editor smoketest on demand.

use aozora_lsp::{compute_diagnostics, format_edits, hover_at};
use tower_lsp::lsp_types::{DiagnosticSeverity, HoverContents, Position};

#[test]
fn plain_text_yields_no_diagnostics_and_no_edits() {
    let src = "hello world";
    assert!(compute_diagnostics(src).is_empty());
    assert!(format_edits(src).is_empty());
}

#[test]
fn pua_collision_produces_warning_diagnostic() {
    // PUA collision triggers SourceContainsPua plus an internal
    // sanity-check; at least one warning-severity diagnostic must
    // surface.
    let src = "oops\u{E001}here";
    let diags = compute_diagnostics(src);
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::WARNING)),
        "expected at least one warning diagnostic, got {diags:?}",
    );
}

#[test]
fn implicit_ruby_reformats_via_format_edits() {
    let src = "日本《にほん》";
    let edits = format_edits(src);
    assert_eq!(edits.len(), 1, "non-canonical ruby should produce one edit");
    assert!(edits[0].new_text.starts_with('｜'));
}

#[test]
fn canonical_ruby_reformats_to_itself() {
    let src = "｜日本《にほん》";
    assert!(format_edits(src).is_empty());
}

#[test]
fn hover_on_known_gaiji_mentions_resolved_character() {
    let src = "語※［＃「木＋吶のつくり」、第3水準1-85-54］で";
    // cursor inside the gaiji token
    let pos = Position::new(0, 3);
    let hover = hover_at(src, pos).expect("hover must fire");
    let md = match hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("expected Markdown hover"),
    };
    // JIS X 0213:2004 plane 1 row 85 cell 54 = 枘 (U+6798).
    assert!(md.contains("枘") || md.contains("6798"));
}

#[test]
fn hover_outside_any_gaiji_returns_none() {
    assert!(hover_at("ただの文", Position::new(0, 1)).is_none());
}
