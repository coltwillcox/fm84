//! Bytes as `hexdump -C` writes them.

use super::*;

/// Rows a hexdump of these bytes occupies; at least one, so an empty file still
/// has a line to render.
pub fn hex_line_count(bytes: &[u8]) -> usize {
    bytes.len().div_ceil(HEX_BYTES_PER_LINE).max(1)
}

/// One `hexdump -C` row: offset, sixteen bytes split into two groups, then the
/// printable characters. Short rows are padded so the gutter stays aligned.
pub fn hex_line(offset: usize, bytes: &[u8]) -> String {
    hex_row(offset, bytes, HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS)
}

/// The same row with `per_line` bytes on it, for a pane too narrow for sixteen.
/// `digits` of 0 leaves the offset off altogether, which is what the narrowest
/// layouts come down to - by then it is a third of the row.
///
/// The gap splitting the bytes in half goes with the offset. Both are there to
/// count your way along a row by, and a row with no offset to count from has
/// little use for the other.
pub fn hex_row(offset: usize, bytes: &[u8], per_line: usize, digits: usize) -> String {
    let mut text = String::with_capacity(hex_row_width(per_line, digits));
    if digits > 0 {
        text.push_str(&format!("{offset:0digits$x}  "));
    }

    for index in 0..per_line {
        if digits > 0 && index == per_line / 2 {
            text.push(' ');
        }
        match bytes.get(index) {
            Some(byte) => text.push_str(&format!("{byte:02x} ")),
            None => text.push_str("   "),
        }
    }

    text.push_str(" |");
    for byte in bytes {
        text.push(if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' });
    }
    text.push('|');
    text
}

/// Columns such a row takes: the offset and the two spaces after it, three
/// columns a byte, the gap splitting them in half, and the gutter in its bars
/// behind one more space.
pub const fn hex_row_width(per_line: usize, digits: usize) -> usize {
    let offset = if digits > 0 { digits + 2 + 1 } else { 0 };
    offset + per_line * 3 + 2 + per_line + 1
}
