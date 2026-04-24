//! afm lexer diagnostic → LSP `Diagnostic` mapping.
//!
//! Every variant of [`afm_parser::Diagnostic`] carries a byte-range
//! [`afm_syntax::Span`] that points into the original source buffer
//! (the lexer's Phase 0 sanitization does not shift byte offsets, so
//! these indices line up with the source the editor is holding).
//! [`compute_diagnostics`] runs the full afm parse pipeline — it's
//! `parse(&arena, source, &opts)` under the hood — and maps every
//! diagnostic it collects into an LSP diagnostic with line/UTF-16
//! coordinates.
//!
//! Severity is chosen per-variant: lexer structural self-checks (V1–V3)
//! are `Error`, source-side hygiene observations (PUA collision,
//! residual markers) are `Warning`.

use afm_parser::{ComrakArena, Diagnostic as AfmDiagnostic, Options, parse};
use afm_syntax::Span;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

use crate::position::byte_offset_to_position;

/// Parse `source` and return its diagnostics in LSP shape.
#[must_use]
pub fn compute_diagnostics(source: &str) -> Vec<Diagnostic> {
    let arena = ComrakArena::new();
    let opts = Options::afm_default();
    let result = parse(&arena, source, &opts);
    result
        .diagnostics
        .iter()
        .map(|d| to_lsp(source, d))
        .collect()
}

fn to_lsp(source: &str, d: &AfmDiagnostic) -> Diagnostic {
    let (span, message, code, severity) = describe(d);
    let start = byte_offset_to_position(source, span.start as usize);
    let end = byte_offset_to_position(source, span.end as usize);
    Diagnostic {
        range: Range::new(start, end),
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_owned())),
        source: Some("aozora-lsp".to_owned()),
        message,
        ..Default::default()
    }
}

fn describe(d: &AfmDiagnostic) -> (Span, String, &'static str, DiagnosticSeverity) {
    match d {
        AfmDiagnostic::SourceContainsPua {
            span, codepoint, ..
        } => (
            *span,
            format!(
                "ソースに PUA 字 U+{:04X} が含まれています (lexer の sentinel と衝突する恐れ)",
                *codepoint as u32,
            ),
            "aozora::source-contains-pua",
            DiagnosticSeverity::WARNING,
        ),
        AfmDiagnostic::UnclosedBracket { span, kind, .. } => (
            *span,
            format!("{kind:?} の開き括弧に対応する閉じ括弧がありません"),
            "aozora::unclosed-bracket",
            DiagnosticSeverity::ERROR,
        ),
        AfmDiagnostic::UnmatchedClose { span, kind, .. } => (
            *span,
            format!("対応する開き括弧のない {kind:?} の閉じ括弧"),
            "aozora::unmatched-close",
            DiagnosticSeverity::ERROR,
        ),
        AfmDiagnostic::ResidualAnnotationMarker { span, .. } => (
            *span,
            "`［＃…］` が分類されずに残っています (未対応の注記種別の可能性)".to_owned(),
            "aozora::residual-annotation-marker",
            DiagnosticSeverity::WARNING,
        ),
        AfmDiagnostic::UnregisteredSentinel {
            span, codepoint, ..
        } => (
            *span,
            format!(
                "レジストリに未登録の PUA sentinel U+{:04X} が検出されました (lexer 内部矛盾)",
                *codepoint as u32,
            ),
            "aozora::unregistered-sentinel",
            DiagnosticSeverity::ERROR,
        ),
        AfmDiagnostic::RegistryOutOfOrder { span, .. } => (
            *span,
            "プレースホルダーレジストリが昇順に並んでいません (lexer 内部矛盾)".to_owned(),
            "aozora::registry-out-of-order",
            DiagnosticSeverity::ERROR,
        ),
        AfmDiagnostic::RegistryPositionMismatch {
            span, expected, ..
        } => (
            *span,
            format!(
                "レジストリは U+{:04X} を期待していますが別の字が置かれています (lexer 内部矛盾)",
                *expected as u32,
            ),
            "aozora::registry-position-mismatch",
            DiagnosticSeverity::ERROR,
        ),
        // afm_parser::Diagnostic は `#[non_exhaustive]` なので、
        // 将来の追加 variant は generic な warning として一旦通し、
        // LSP クライアントには原文メッセージを届ける。
        other => (
            Span::new(0, 0),
            format!("未知の afm 診断: {other:?}"),
            "aozora::unknown-diagnostic",
            DiagnosticSeverity::WARNING,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_has_no_diagnostics() {
        assert!(compute_diagnostics("hello world").is_empty());
    }

    #[test]
    fn canonical_ruby_has_no_diagnostics() {
        assert!(compute_diagnostics("｜日本《にほん》").is_empty());
    }

    #[test]
    fn source_contains_pua_surfaces_as_warning() {
        // ソース側に PUA sentinel (U+E001) を撒くと、Phase 0 の
        // `SourceContainsPua` (Warning) に加えて Phase 6 の
        // `UnregisteredSentinel` (Error) も連鎖して発火する。
        // ここでは Warning 側だけが期待通り出ていることを確認する。
        let src = "abc\u{E001}def";
        let diags = compute_diagnostics(src);
        let warnings: Vec<&Diagnostic> = diags
            .iter()
            .filter(|d| {
                matches!(
                    &d.code,
                    Some(NumberOrString::String(s)) if s == "aozora::source-contains-pua"
                )
            })
            .collect();
        assert_eq!(
            warnings.len(),
            1,
            "expected one source-contains-pua warning, got {diags:?}",
        );
        assert_eq!(warnings[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn diagnostic_carries_aozora_lsp_source_tag() {
        let src = "abc\u{E001}def";
        let diags = compute_diagnostics(src);
        assert!(
            diags
                .iter()
                .all(|d| d.source.as_deref() == Some("aozora-lsp")),
            "every diagnostic must be tagged aozora-lsp: {diags:?}",
        );
    }
}
