//! aozora lexer diagnostic → LSP `Diagnostic` mapping.
//!
//! Every variant of [`aozora::Diagnostic`] carries a byte-range
//! [`aozora::Span`] that points into the original source buffer
//! (the lexer's Phase 0 sanitization does not shift byte offsets, so
//! these indices line up with the source the editor is holding).
//!
//! ## Message style
//!
//! Each diagnostic message is written for the *typesetter*, not the
//! parser author. Three things every variant should answer:
//!
//! 1. **何が起きた** — plain summary in the first sentence
//! 2. **何が問題** — why this matters in plain Japanese
//! 3. **どう直す** — at least one concrete example of the corrected
//!    form, written in actual aozora notation
//!
//! `tags` is set when the lint is "unnecessary" (an editor can grey
//! out unnecessary code). `data` carries enough context for the
//! `code_action` handler to construct a quick-fix without re-parsing.

use aozora::{Diagnostic as AozoraDiagnostic, Document, PairKind, Span};
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Range};

use crate::line_index::LineIndex;

/// Serialised payload attached to LSP `Diagnostic.data`. Lets the
/// `code_action` handler build a quick-fix without re-parsing or
/// re-classifying the offending span.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DiagnosticPayload {
    /// `UnclosedBracket` — the open delimiter is here; the missing
    /// close is one of the chars in `expected_close`.
    UnclosedBracket {
        pair_kind: SerializablePairKind,
        expected_close: String,
    },
    /// `UnmatchedClose` — the close delimiter is here without a
    /// matching open.
    UnmatchedClose { pair_kind: SerializablePairKind },
    /// `SourceContainsPua` — a private-use codepoint clashes with
    /// the lexer's sentinel reservations.
    SourceContainsPua { codepoint: u32 },
    /// `ResidualAnnotationMarker` — `［＃...］` pair survived
    /// classification (likely a typo or unsupported keyword).
    ResidualAnnotationMarker,
}

/// Stringified [`PairKind`] for `serde_json` round-tripping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SerializablePairKind {
    Bracket,
    Ruby,
    DoubleRuby,
    Tortoise,
    Quote,
}

impl From<PairKind> for SerializablePairKind {
    fn from(k: PairKind) -> Self {
        // `PairKind` is `#[non_exhaustive]`, so we have to handle
        // future-added variants. Merging `Bracket` with the wildcard
        // (`PairKind::Bracket | _`) makes the fallback explicit
        // without giving clippy two arms with identical bodies — the
        // pre-merge shape (`PairKind::Bracket => Self::Bracket` as a
        // distinct arm plus a separate `_ => Self::Bracket`) tripped
        // `clippy::match_same_arms` on the duplicate body.
        match k {
            PairKind::Ruby => Self::Ruby,
            PairKind::DoubleRuby => Self::DoubleRuby,
            PairKind::Tortoise => Self::Tortoise,
            PairKind::Quote => Self::Quote,
            PairKind::Bracket | _ => Self::Bracket,
        }
    }
}

impl SerializablePairKind {
    /// Human-readable open delimiter literal (`［`, `《`, `《《`,
    /// `〔`, `「`).
    #[must_use]
    pub fn open_str(self) -> &'static str {
        match self {
            Self::Bracket => "［",
            Self::Ruby => "《",
            Self::DoubleRuby => "《《",
            Self::Tortoise => "〔",
            Self::Quote => "「",
        }
    }

    /// Human-readable close delimiter literal.
    #[must_use]
    pub fn close_str(self) -> &'static str {
        match self {
            Self::Bracket => "］",
            Self::Ruby => "》",
            Self::DoubleRuby => "》》",
            Self::Tortoise => "〕",
            Self::Quote => "」",
        }
    }
}

/// Parse `source` and return its diagnostics in LSP shape.
#[must_use]
pub fn compute_diagnostics(source: &str) -> Vec<Diagnostic> {
    let document = Document::new(source);
    let tree = document.parse();
    compute_diagnostics_from_iter(source, tree.diagnostics())
}

/// Map a set of pre-computed [`AozoraDiagnostic`]s to LSP diagnostics.
#[must_use]
pub fn compute_diagnostics_from_iter(
    source: &str,
    diagnostics: &[AozoraDiagnostic],
) -> Vec<Diagnostic> {
    let line_index = LineIndex::new(source);
    diagnostics
        .iter()
        .map(|d| to_lsp(source, &line_index, d))
        .collect()
}

/// Backwards-compat alias used by the LSP backend's
/// `publishDiagnostics` path.
#[must_use]
pub fn compute_diagnostics_from_parsed(
    source: &str,
    diagnostics: &[AozoraDiagnostic],
) -> Vec<Diagnostic> {
    compute_diagnostics_from_iter(source, diagnostics)
}

fn to_lsp(source: &str, line_index: &LineIndex, d: &AozoraDiagnostic) -> Diagnostic {
    let described = describe(d);
    let start = line_index.position(source, described.span.start as usize);
    let end = line_index.position(source, described.span.end as usize);
    Diagnostic {
        range: Range::new(start, end),
        severity: Some(described.severity),
        code: Some(NumberOrString::String(described.code.to_owned())),
        source: Some("aozora-lsp".to_owned()),
        message: described.message,
        tags: described.tags,
        data: described
            .payload
            .map(|p| serde_json::to_value(p).unwrap_or(serde_json::Value::Null)),
        ..Default::default()
    }
}

struct Described {
    span: Span,
    message: String,
    code: &'static str,
    severity: DiagnosticSeverity,
    tags: Option<Vec<DiagnosticTag>>,
    payload: Option<DiagnosticPayload>,
}

/// Top-level dispatcher. Unpacks the diagnostic variant and delegates
/// to a per-variant helper below — splitting them out keeps this
/// function short enough to drop the previous
/// `#[allow(clippy::too_many_lines)]` and makes each catalogue entry
/// independently navigable.
fn describe(d: &AozoraDiagnostic) -> Described {
    match d {
        AozoraDiagnostic::SourceContainsPua {
            span, codepoint, ..
        } => describe_source_contains_pua(*span, *codepoint),
        AozoraDiagnostic::UnclosedBracket { span, kind, .. } => {
            describe_unclosed_bracket(*span, *kind)
        }
        AozoraDiagnostic::UnmatchedClose { span, kind, .. } => {
            describe_unmatched_close(*span, *kind)
        }
        AozoraDiagnostic::ResidualAnnotationMarker { span, .. } => {
            describe_residual_annotation_marker(*span)
        }
        AozoraDiagnostic::UnregisteredSentinel {
            span, codepoint, ..
        } => describe_unregistered_sentinel(*span, *codepoint),
        AozoraDiagnostic::RegistryOutOfOrder { span, .. } => describe_registry_out_of_order(*span),
        AozoraDiagnostic::RegistryPositionMismatch { span, expected, .. } => {
            describe_registry_position_mismatch(*span, *expected)
        }
        // aozora::Diagnostic は `#[non_exhaustive]` なので、将来の追加 variant
        // は generic な warning として一旦通し、LSP クライアントには原文
        // メッセージを届ける。
        other => describe_unknown(other),
    }
}

fn describe_source_contains_pua(span: Span, codepoint: char) -> Described {
    Described {
        span,
        message: format!(
            "私用領域文字 `U+{cp:04X}` がソースに紛れ込んでいます。\n\n\
             この文字 (`{ch}`) は青空文庫の通常テキストには現れない予約コードポイントで、aozora-lex の内部マーカー (U+E001..U+E004) と衝突します。\n\
             通常はテキストエディタの非表示文字設定や、コピペ時の不可視サニタイズで混入します。\n\n\
             直し方: 該当の 1 文字を削除してください。",
            cp = codepoint as u32,
            ch = codepoint,
        ),
        code: "aozora::source-contains-pua",
        severity: DiagnosticSeverity::WARNING,
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        payload: Some(DiagnosticPayload::SourceContainsPua {
            codepoint: codepoint as u32,
        }),
    }
}

fn describe_unclosed_bracket(span: Span, kind: PairKind) -> Described {
    let pk: SerializablePairKind = kind.into();
    let open = pk.open_str();
    let close = pk.close_str();
    let example = example_for(pk);
    Described {
        span,
        message: format!(
            "閉じられていない `{open}` があります。\n\n\
             どこかに対応する `{close}` を必ず置いてください。aozora 記法では一行内で閉じるのが基本です。\n\n\
             例: `{example}`",
        ),
        code: "aozora::unclosed-bracket",
        severity: DiagnosticSeverity::ERROR,
        tags: None,
        payload: Some(DiagnosticPayload::UnclosedBracket {
            pair_kind: pk,
            expected_close: close.to_owned(),
        }),
    }
}

fn describe_unmatched_close(span: Span, kind: PairKind) -> Described {
    let pk: SerializablePairKind = kind.into();
    let open = pk.open_str();
    let close = pk.close_str();
    Described {
        span,
        message: format!(
            "対応する `{open}` のない `{close}` です。\n\n\
             考えられる原因:\n\
             1. 余分な `{close}` を打ってしまった → 削除する\n\
             2. 前にあるはずの `{open}` が欠けている → 適切な位置に追加する\n\
             3. その間に別の `{close}` があり、ペアが一段ずれた → 該当箇所のペアを見直す\n\n\
             右クリックの Quick Fix から「`{close}` を削除する」を選べます。",
        ),
        code: "aozora::unmatched-close",
        severity: DiagnosticSeverity::ERROR,
        tags: None,
        payload: Some(DiagnosticPayload::UnmatchedClose { pair_kind: pk }),
    }
}

fn describe_residual_annotation_marker(span: Span) -> Described {
    Described {
        span,
        message: "未分類の `［＃...］` 注記です。\n\n\
                 注記辞典 (`gaiji_chuki.pdf`) のキーワードに合致しなかったか、誤字の可能性があります。\n\n\
                 確認手順:\n\
                 1. ［＃ の中身が `改ページ` / `中央揃え` などの登録済みキーワードと一致するか確認\n\
                 2. `第3水準1-...` のような JIS X 0213 mencode を付け忘れていないか確認\n\
                 3. それでも不明な場合は description-only 形式 (`※［＃「説明」］`) でひとまず通せます"
            .to_owned(),
        code: "aozora::residual-annotation-marker",
        severity: DiagnosticSeverity::WARNING,
        tags: None,
        payload: Some(DiagnosticPayload::ResidualAnnotationMarker),
    }
}

fn describe_unregistered_sentinel(span: Span, codepoint: char) -> Described {
    Described {
        span,
        message: format!(
            "未登録の私用領域 sentinel `U+{cp:04X}` が検出されました (lexer 内部の整合性エラー)。\n\n\
             これは aozora-lex のバグの可能性が高いです。再現手順を添えて issue で報告してください。",
            cp = codepoint as u32,
        ),
        code: "aozora::unregistered-sentinel",
        severity: DiagnosticSeverity::ERROR,
        tags: None,
        payload: None,
    }
}

fn describe_registry_out_of_order(span: Span) -> Described {
    Described {
        span,
        message: "プレースホルダーレジストリの順序が崩れています (lexer 内部の整合性エラー)。\n\n\
             aozora-lex のバグの可能性があります。"
            .to_owned(),
        code: "aozora::registry-out-of-order",
        severity: DiagnosticSeverity::ERROR,
        tags: None,
        payload: None,
    }
}

fn describe_registry_position_mismatch(span: Span, expected: char) -> Described {
    Described {
        span,
        message: format!(
            "プレースホルダーレジストリは `U+{cp:04X}` を期待していたのに別の字が置かれていました (lexer 内部の整合性エラー)。\n\n\
             aozora-lex のバグの可能性があります。",
            cp = expected as u32,
        ),
        code: "aozora::registry-position-mismatch",
        severity: DiagnosticSeverity::ERROR,
        tags: None,
        payload: None,
    }
}

fn describe_unknown(other: &AozoraDiagnostic) -> Described {
    Described {
        span: Span::new(0, 0),
        message: format!(
            "未対応の aozora 診断です: {other:?}\n\n\
             aozora-lsp と aozora-lex のバージョンが揃っていない可能性があります。"
        ),
        code: "aozora::unknown-diagnostic",
        severity: DiagnosticSeverity::WARNING,
        tags: None,
        payload: None,
    }
}

/// Per-kind canonical example used in the unclosed-bracket message.
const fn example_for(kind: SerializablePairKind) -> &'static str {
    match kind {
        SerializablePairKind::Bracket => "［＃改ページ］",
        SerializablePairKind::Ruby => "｜青空《あおぞら》",
        SerializablePairKind::DoubleRuby => "《《重要》》",
        SerializablePairKind::Tortoise => "〔Crevez chiens〕",
        SerializablePairKind::Quote => "［＃「青空」に傍点］",
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
    fn source_contains_pua_message_explains_what_to_do() {
        let src = "abc\u{E001}def";
        let diags = compute_diagnostics(src);
        let pua = diags
            .iter()
            .find(|d| {
                matches!(
                    &d.code,
                    Some(NumberOrString::String(s)) if s == "aozora::source-contains-pua"
                )
            })
            .expect("PUA warning expected");
        assert!(pua.message.contains("削除"), "msg: {}", pua.message);
        assert_eq!(pua.severity, Some(DiagnosticSeverity::WARNING));
        assert!(
            pua.tags
                .as_ref()
                .is_some_and(|t| t.contains(&DiagnosticTag::UNNECESSARY))
        );
        assert!(pua.data.is_some(), "data payload should be attached");
    }

    #[test]
    fn unclosed_bracket_message_carries_example_and_close_char() {
        // `［＃改ページ` (no closing ］) — must surface as UnclosedBracket.
        let src = "本文［＃改ページ";
        let diags = compute_diagnostics(src);
        let unclosed = diags
            .iter()
            .find(|d| {
                matches!(
                    &d.code,
                    Some(NumberOrString::String(s)) if s == "aozora::unclosed-bracket"
                )
            })
            .expect("UnclosedBracket expected on missing ］");
        assert!(unclosed.message.contains("］"), "{}", unclosed.message);
        assert!(
            unclosed.message.contains("例:"),
            "message must include a concrete example: {}",
            unclosed.message,
        );
        assert!(
            unclosed.data.is_some(),
            "data payload required for quick-fix"
        );
    }

    #[test]
    fn unmatched_close_message_lists_three_causes() {
        // `］` without a leading `［` — surfaces as UnmatchedClose.
        let src = "本文 ］";
        let diags = compute_diagnostics(src);
        let unmatched = diags
            .iter()
            .find(|d| {
                matches!(
                    &d.code,
                    Some(NumberOrString::String(s)) if s == "aozora::unmatched-close"
                )
            })
            .expect("UnmatchedClose expected on stray ］");
        assert!(unmatched.message.contains("削除"), "{}", unmatched.message);
        assert!(
            unmatched.message.contains("欠けている"),
            "{}",
            unmatched.message
        );
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

    #[test]
    fn payload_round_trips_through_json() {
        let payload = DiagnosticPayload::UnclosedBracket {
            pair_kind: SerializablePairKind::Bracket,
            expected_close: "］".to_owned(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        let back: DiagnosticPayload = serde_json::from_value(json).unwrap();
        match back {
            DiagnosticPayload::UnclosedBracket {
                pair_kind,
                expected_close,
            } => {
                assert_eq!(pair_kind, SerializablePairKind::Bracket);
                assert_eq!(expected_close, "］");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
