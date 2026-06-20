//! `aozora-lsp` — Language Server for aozora-flavored-markdown.
//!
//! The server is built on top of the `aozora` library surface from
//! the sibling `aozora` repository. Three primary LSP capabilities:
//!
//! - `textDocument/publishDiagnostics` — every `aozora::Diagnostic`
//!   variant is mapped to an LSP `Diagnostic` with a byte-range span
//!   converted into line/UTF-16-column coordinates.
//! - `textDocument/formatting` — runs `parse ∘ serialize` (via
//!   `aozora_fmt::format_source`) and returns a single document-replace
//!   `TextEdit`.
//! - `textDocument/hover` — when the cursor sits inside a
//!   `※［＃…］` gaiji reference, resolves via `aozora_encoding::gaiji`
//!   and returns a Markdown explanation.
//!
//! The stable public surface is intentionally tiny: [`Cli`] (so `xtask`
//! can generate the shell completions and man page) and the [`Backend`]
//! the daemon wires into tower-lsp. The internal building blocks the
//! handlers are made of are re-exported behind the `#[doc(hidden)]`
//! [`internals`] module — for the crate's own tests, benches, examples,
//! and fuzz targets only, with no semver guarantee.

#![forbid(unsafe_code)]

mod backend;
mod cli;
mod code_actions;
mod commands;
mod completion;
mod diagnostics;
mod document_symbol;
mod folding_range;
mod formatting;
mod gaiji_spans;
mod half_width_emmet;
mod hover;
mod incremental;
mod inlay_hints;
mod line_index;
mod linked_editing;
mod metrics;
mod on_type_formatting;
mod paragraph;
mod position;
mod segment_cache;
mod semantic_tokens;
mod state;
mod structured_snippets;
mod text_edit;

pub use backend::Backend;
pub use cli::Cli;

/// Internal API surface — re-exported here **only** for the crate's own
/// integration tests, benches, examples, and fuzz targets, which compile as
/// separate crates and so can reach `pub` items but not `pub(crate)` ones.
///
/// This module is `#[doc(hidden)]` and is **not** part of the public API: it
/// carries no semver guarantee and anything in it may change or vanish in any
/// release. Public-surface tools (`cargo public-api`, `cargo semver-checks`)
/// skip `#[doc(hidden)]` items, so the stable surface stays [`Cli`] + [`Backend`].
///
/// The targets that consume it are gated on the `internals` Cargo feature
/// (see `Cargo.toml`), so a plain `cargo test` skips them; CI runs
/// `cargo test --features internals`.
#[doc(hidden)]
pub mod internals {
    pub use crate::code_actions::wrap_selection_actions;
    pub use crate::commands::{COMMAND_CANONICALIZE_SLUG, canonicalize_slug_edit};
    pub use crate::completion::completion_at;
    pub use crate::diagnostics::{
        compute_diagnostics, compute_diagnostics_from_iter, compute_diagnostics_from_parsed,
    };
    pub use crate::document_symbol::document_symbols;
    pub use crate::folding_range::folding_ranges;
    pub use crate::formatting::format_edits;
    pub use crate::gaiji_spans::{GaijiSpan, extract_gaiji_spans_from_tree};
    pub use crate::half_width_emmet::emmet_completions;
    pub use crate::hover::hover_at;
    pub use crate::incremental::{IncrementalDoc, input_edit};
    pub use crate::inlay_hints::inlay_hints;
    pub use crate::line_index::LineIndex;
    pub use crate::linked_editing::linked_editing_at;
    pub use crate::on_type_formatting::{TRIGGERS as ON_TYPE_TRIGGERS, format_on_type};
    pub use crate::paragraph::{MAX_PARAGRAPH_BYTES, MutParagraph, ParagraphSnapshot};
    pub use crate::position::{byte_offset_to_position, position_to_byte_offset};
    pub use crate::segment_cache::{ReparseStats, SegmentCache};
    pub use crate::semantic_tokens::{legend as semantic_token_legend, semantic_tokens_full};
    pub use crate::state::{BufferState, DocState, Snapshot};
    pub use crate::structured_snippets::snippet_completions;
    pub use crate::text_edit::{EditError, LocalTextEdit, apply_edits};
}
