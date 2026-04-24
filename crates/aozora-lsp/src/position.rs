//! Byte-offset ↔ LSP `Position` conversion.
//!
//! LSP 3.17 `Position` is `{ line: u32, character: u32 }` where
//! `character` is measured in **UTF-16 code units** relative to the
//! start of the line. afm's lexer works in byte offsets. These
//! helpers bridge the two coordinate systems.

use tower_lsp::lsp_types::Position;

/// Convert a byte offset in `source` into an LSP `Position`.
///
/// Clamps to `source.len()` if `byte_offset` overshoots — LSP clients
/// treat an out-of-range position as end-of-buffer anyway, and
/// failing loudly here would just translate afm's "probably a bug"
/// into "definitely a panic".
#[must_use]
pub fn byte_offset_to_position(source: &str, byte_offset: usize) -> Position {
    let byte_offset = byte_offset.min(source.len());
    let mut line: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i >= byte_offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = source[line_start..byte_offset]
        .encode_utf16()
        .count()
        .min(u32::MAX as usize) as u32;
    Position::new(line, col)
}

/// Convert an LSP `Position` back into a byte offset in `source`.
///
/// Returns `None` if the position names a line past the end of the
/// buffer; UTF-16 characters past the end of their line clamp to the
/// line end (matching most LSP clients' own behaviour).
#[must_use]
pub fn position_to_byte_offset(source: &str, position: Position) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut current_line: u32 = 0;
    let mut line_start: usize = 0;
    for (i, b) in bytes.iter().enumerate() {
        if current_line == position.line {
            break;
        }
        if *b == b'\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    if current_line != position.line {
        return None;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |p| line_start + p);
    let line_slice = &source[line_start..line_end];
    let mut utf16_cursor: u32 = 0;
    for (byte_i, ch) in line_slice.char_indices() {
        if utf16_cursor >= position.character {
            return Some(line_start + byte_i);
        }
        utf16_cursor = utf16_cursor.saturating_add(ch.len_utf16() as u32);
    }
    Some(line_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_byte_maps_to_origin() {
        assert_eq!(byte_offset_to_position("hello", 0), Position::new(0, 0));
    }

    #[test]
    fn last_byte_maps_to_end_of_line() {
        assert_eq!(byte_offset_to_position("hello", 5), Position::new(0, 5));
    }

    #[test]
    fn overshoot_clamps_to_eof() {
        assert_eq!(byte_offset_to_position("hi", 99), Position::new(0, 2));
    }

    #[test]
    fn newline_advances_line() {
        let src = "one\ntwo";
        assert_eq!(byte_offset_to_position(src, 4), Position::new(1, 0));
        assert_eq!(byte_offset_to_position(src, 7), Position::new(1, 3));
    }

    #[test]
    fn utf8_multibyte_uses_utf16_column() {
        // 「あ」は UTF-8 で 3 バイト、UTF-16 で 1 code unit。
        let src = "あいう";
        assert_eq!(byte_offset_to_position(src, 3), Position::new(0, 1));
        assert_eq!(byte_offset_to_position(src, 6), Position::new(0, 2));
        assert_eq!(byte_offset_to_position(src, 9), Position::new(0, 3));
    }

    #[test]
    fn surrogate_pair_counts_two_utf16_units() {
        // U+1F600 (😀) は UTF-16 でサロゲートペア (2 code unit)。
        let src = "a😀b";
        // 1 (a) + 2 (😀 surrogate pair) + 1 (b) = 4 UTF-16 code units
        assert_eq!(byte_offset_to_position(src, 5), Position::new(0, 3));
        assert_eq!(byte_offset_to_position(src, 6), Position::new(0, 4));
    }

    #[test]
    fn round_trip_origin() {
        let src = "hello";
        assert_eq!(position_to_byte_offset(src, Position::new(0, 0)), Some(0));
    }

    #[test]
    fn round_trip_newline_line_2() {
        let src = "one\ntwo\nthree";
        assert_eq!(position_to_byte_offset(src, Position::new(2, 0)), Some(8));
        assert_eq!(position_to_byte_offset(src, Position::new(2, 5)), Some(13));
    }

    #[test]
    fn position_past_end_of_buffer_returns_none() {
        let src = "one";
        assert_eq!(position_to_byte_offset(src, Position::new(5, 0)), None);
    }

    #[test]
    fn utf16_column_past_line_end_clamps_to_line_end() {
        let src = "abc\ndef";
        // asking for column 99 on line 0 returns the line-end byte (3)
        assert_eq!(position_to_byte_offset(src, Position::new(0, 99)), Some(3));
    }

    #[test]
    fn byte_to_position_and_back_is_identity() {
        let src = "abc\nあいう\ndef";
        for byte in 0..=src.len() {
            if !src.is_char_boundary(byte) {
                continue;
            }
            let pos = byte_offset_to_position(src, byte);
            let round = position_to_byte_offset(src, pos).expect("round-trip");
            assert_eq!(round, byte, "byte {byte} round-tripped to {round} via {pos:?}");
        }
    }
}
