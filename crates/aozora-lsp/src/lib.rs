//! `aozora-lsp` — Language Server for aozora-flavored-markdown.
//!
//! The server is built on top of the stable `aozora` library surface
//! defined by ADR-0009 in the sibling `aozora` repository. It exposes
//! three LSP capabilities today:
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
//! The public surface is the [`Backend`] type plus the pure helper
//! functions the hover / diagnostics / formatting handlers are
//! built from — they are exported so they can be unit-tested
//! without booting a full LSP session.

#![forbid(unsafe_code)]

mod backend;
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
pub mod metrics;
mod position;
pub mod segment_cache;
mod segmented_doc;
mod semantic_tokens;
mod state;
mod text_edit;

pub use backend::Backend;
pub use code_actions::wrap_selection_actions;
pub use commands::{COMMAND_CANONICALIZE_SLUG, canonicalize_slug_edit};
pub use completion::completion_at;
pub use diagnostics::{
    compute_diagnostics, compute_diagnostics_from_iter, compute_diagnostics_from_parsed,
};
pub use document_symbol::document_symbols;
pub use folding_range::folding_ranges;
pub use formatting::format_edits;
pub use gaiji_spans::{GaijiSpan, extract_gaiji_spans as extract_gaiji_spans_for_bench};
pub use half_width_emmet::emmet_completions;
pub use hover::hover_at;
pub use incremental::{IncrementalDoc, input_edit};
pub use inlay_hints::inlay_hints;
pub use line_index::LineIndex;
pub use linked_editing::linked_editing_at;
pub use position::{byte_offset_to_position, position_to_byte_offset};
pub use segmented_doc::{Segment, SegmentedDoc};
pub use semantic_tokens::{legend as semantic_token_legend, semantic_tokens_full};
pub use state::{BufferState, DocState, Snapshot};
pub use text_edit::{EditError, LocalTextEdit, apply_edits};
