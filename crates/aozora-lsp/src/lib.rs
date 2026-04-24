//! `aozora-lsp` — Language Server for aozora-flavored-markdown.
//!
//! The server is built on top of the stable `afm` library surface
//! defined by ADR-0009 in the sibling `afm` repository. It exposes
//! three LSP capabilities today:
//!
//! - `textDocument/publishDiagnostics` — every `afm_lexer::Diagnostic`
//!   variant is mapped to an LSP `Diagnostic` with a byte-range span
//!   converted into line/UTF-16-column coordinates.
//! - `textDocument/formatting` — runs `parse ∘ serialize` (via
//!   `aozora_fmt::format_source`) and returns a single document-replace
//!   `TextEdit`.
//! - `textDocument/hover` — when the cursor sits inside a
//!   `※［＃…］` gaiji reference, resolves via `afm_encoding::gaiji`
//!   and returns a Markdown explanation.
//!
//! The public surface is the [`Backend`] type plus the pure helper
//! functions the hover / diagnostics / formatting handlers are
//! built from — they are exported so they can be unit-tested
//! without booting a full LSP session.

#![forbid(unsafe_code)]

mod backend;
mod diagnostics;
mod formatting;
mod hover;
mod position;

pub use backend::Backend;
pub use diagnostics::compute_diagnostics;
pub use formatting::format_edits;
pub use hover::hover_at;
pub use position::{byte_offset_to_position, position_to_byte_offset};
