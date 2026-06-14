#![no_main]
//! Fuzz the pure edit + coordinate machinery behind the LSP's
//! `did_change` path. This is aozora-tools' *own* byte / UTF-16 code —
//! exactly the kind of logic `forbid(unsafe_code)` does not protect from
//! off-by-one and char-boundary bugs.
//!
//! For arbitrary input, asserts:
//! * [`apply_edits`] never panics on arbitrary (out-of-bounds, inverted,
//!   mid-codepoint) edits — it validates and returns `Err`.
//! * [`byte_offset_to_position`] / [`position_to_byte_offset`] never
//!   panic and round-trip to the identity on every char boundary.
//!
//! The derive + assert logic is mirrored by
//! `crates/aozora-lsp/tests/fuzz_regressions.rs` so promoted crashes
//! replay on stable; keep the two in sync.

use libfuzzer_sys::fuzz_target;

use aozora_lsp::{LocalTextEdit, apply_edits, byte_offset_to_position, position_to_byte_offset};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();

    // A single edit derived from the leading bytes — deliberately
    // allowed to be out-of-bounds / inverted / mid-codepoint so the
    // validation path is exercised. apply_edits must never panic.
    if data.len() >= 4 {
        let a = u16::from_le_bytes([data[0], data[1]]) as usize;
        let b = u16::from_le_bytes([data[2], data[3]]) as usize;
        let repl = String::from_utf8_lossy(&data[4..data.len().min(16)]).into_owned();
        let edits = [LocalTextEdit::new(a..b, repl)];
        let _ = apply_edits(&text, &edits);
    }

    // Position <-> byte conversion: never panic, identity on boundaries.
    if text.len() <= 8192 {
        for byte in 0..=text.len() {
            if !text.is_char_boundary(byte) {
                continue;
            }
            let pos = byte_offset_to_position(&text, byte);
            if let Some(back) = position_to_byte_offset(&text, pos) {
                assert_eq!(back, byte, "position round-trip broke at byte {byte}");
            }
        }
    }
});
