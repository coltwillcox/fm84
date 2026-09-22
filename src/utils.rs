use crate::constants::*;
use ratatui::style::Color;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width in terminal columns, which is what the layout needs. Not the byte
/// length, and not the character count either: CJK characters take two columns.
pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

// Converts bytes to human-readable format with binary prefixes (KiB, MiB, etc.)
pub fn format_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.0} {}", size, UNITS[unit_index])
}

/// Width of a line as the viewer draws it: tabs expanded, columns not bytes.
pub fn line_display_width(line: &str) -> usize {
    line.chars()
        .map(|character| {
            if character == '\t' {
                TAB_SPACES.len()
            } else {
                UnicodeWidthChar::width(character).unwrap_or(0)
            }
        })
        .sum()
}

pub fn color_for_extension(ext: &str) -> Color {
    if ext.is_empty() {
        return COLOR_FILE;
    }
    // Simple hash of extension bytes.
    let hash: u32 = ext.bytes().fold(5381u32, |h, b| h.wrapping_mul(33).wrapping_add(b as u32));
    // Derive hue 0..360, keep saturation and lightness high for synthwave look.
    let hue: f64 = (hash % 360) as f64;
    let saturation: f64 = 0.7;
    let lightness: f64 = 0.65;
    // HSL to RGB.
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = lightness - c / 2.0;
    let (r1, g1, b1) = match hue as u32 {
        0..60 => (c, x, 0.0),
        60..120 => (x, c, 0.0),
        120..180 => (0.0, c, x),
        180..240 => (0.0, x, c),
        240..300 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::Rgb(
        ((r1 + m) * 255.0) as u8,
        ((g1 + m) * 255.0) as u8,
        ((b1 + m) * 255.0) as u8,
    )
}

/// Shorten a path to the last `n` columns, marking the cut with "...". Walks
/// back a character at a time: slicing to a byte offset splits multi-byte
/// characters and panics.
pub fn limit_path_string(path: &Path, n: usize) -> String {
    let path_string = path.display().to_string();
    if display_width(&path_string) <= n {
        return path_string;
    }

    let mut width = 0;
    let mut start = path_string.len();
    for (index, character) in path_string.char_indices().rev() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > n {
            break;
        }
        width += character_width;
        start = index;
    }

    format!("...{}", &path_string[start..])
}
