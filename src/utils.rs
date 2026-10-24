use crate::constants::*;
use ratatui::style::Color;
use std::ffi::{OsStr, OsString};
use std::io::{Write, stdout};
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Width in terminal columns, which is what the layout needs. Not the byte
/// length, and not the character count either: CJK characters take two columns.
pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// A byte count as the panels write it, rounded to a whole number of KiB,
/// MiB and so on - or of kB, MB, when F11 asks for powers of 1000.
pub fn format_size(bytes: u64) -> String {
    let (step, units) = if crate::display::decimal_sizes() { (1000.0, DECIMAL_UNITS) } else { (1024.0, UNITS) };
    let mut size = bytes as f64;
    let mut unit_index = 0;

    // Rounded before it is compared: 1023.6 KiB is written as a whole number,
    // and as one it is the next unit up - not "1024 KiB".
    while size.round() >= step && unit_index < units.len() - 1 {
        size /= step;
        unit_index += 1;
    }

    format!("{:.0} {}", size, units[unit_index])
}

/// A modification time as the Modified column writes it. `now` is passed in
/// so a relative date can be tested, and so one frame measures every row from
/// the same moment.
pub fn format_modified(time: std::time::SystemTime, format: crate::options::DateFormat, now: std::time::SystemTime) -> String {
    use crate::options::DateFormat;
    let local: chrono::DateTime<chrono::Local> = time.into();
    match format {
        DateFormat::Short => local.format("%d/%m/%y %H:%M").to_string(),
        DateFormat::Iso => local.format("%Y-%m-%d %H:%M").to_string(),
        DateFormat::Relative => {
            // A time ahead of the clock - a file from a machine set a little
            // fast - reads as just now rather than as a negative age.
            let seconds = now.duration_since(time).map_or(0, |age| age.as_secs());
            match seconds {
                0..60 => "just now".to_string(),
                60..3600 => format!("{} min ago", seconds / 60),
                3600..86_400 => format!("{} h ago", seconds / 3600),
                86_400..2_592_000 => format!("{} days ago", seconds / 86_400),
                2_592_000..31_536_000 => format!("{} mo ago", seconds / 2_592_000),
                _ => format!("{} yr ago", seconds / 31_536_000),
            }
        }
    }
}

/// A time to the second, for the detail lines under the panels, in the
/// order F11's date format puts day, month and year. Relative has no exact
/// form of its own, and the column already says how long ago; the detail
/// gives the time itself, as the short form does.
pub fn format_exact(time: std::time::SystemTime, format: crate::options::DateFormat) -> String {
    use crate::options::DateFormat;
    let local: chrono::DateTime<chrono::Local> = time.into();
    match format {
        DateFormat::Short | DateFormat::Relative => local.format("%d/%m/%y %H:%M:%S").to_string(),
        DateFormat::Iso => local.format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

/// A byte count with its thousands marked off, so a long figure can be read at
/// a glance rather than counted.
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(digit);
    }
    out
}

/// One word of a command from F11, with each `{}` in it standing for `value`.
/// Built as an OsString rather than with str::replace, which would need the
/// path as text: a name that is not valid UTF-8 would come out with a
/// replacement character in it, naming a file that is not there.
pub fn substitute(word: &str, value: &OsStr) -> OsString {
    let mut pieces = word.split("{}");
    let mut out = OsString::from(pieces.next().unwrap_or_default());
    for piece in pieces {
        out.push(value);
        out.push(piece);
    }
    out
}

/// Ask the terminal to put `text` on the system clipboard (OSC 52). Terminals
/// without support ignore it, and it travels over SSH, which is why this is
/// preferred to linking a platform clipboard library.
pub fn set_system_clipboard(text: &str) {
    if text.len() > OSC52_MAX_BYTES {
        return;
    }
    let mut out = stdout();
    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let _ = out.flush();
}

/// Minimal base64, needed only for the escape above.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let bytes = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let packed = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);

        encoded.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        encoded.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        encoded.push(if chunk.len() > 1 { ALPHABET[(packed >> 6) as usize & 63] as char } else { '=' });
        encoded.push(if chunk.len() > 2 { ALPHABET[packed as usize & 63] as char } else { '=' });
    }
    encoded
}

#[cfg(test)]
mod base64_tests {
    use super::base64;

    #[test]
    fn matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("héllo\n".as_bytes()), "aMOpbGxvCg==");
    }
}

/// Expand tabs and neutralise control characters. Files can contain escape
/// sequences - a lossy text view of a binary almost certainly does - and
/// passing those through to the terminal would execute them.
pub fn printable_line(line: &str) -> String {
    let mut text = String::with_capacity(line.len());
    for character in line.chars() {
        if character == '\t' {
            text.extend(std::iter::repeat_n(' ', crate::display::tab_width()));
        } else if character.is_control() {
            text.push('.');
        } else {
            text.push(character);
        }
    }
    text
}

/// Columns one character takes as printable_line draws it: a tab expanded, a
/// control character as the one '.' put in its place, and a wide one as two.
pub fn char_display_width(character: char) -> usize {
    if character == '\t' {
        crate::display::tab_width()
    } else if character.is_control() {
        1
    } else {
        UnicodeWidthChar::width(character).unwrap_or(0)
    }
}

/// Width of a line as the viewer draws it: tabs expanded, columns not bytes.
pub fn line_display_width(line: &str) -> usize {
    line.chars().map(char_display_width).sum()
}

/// The column the character at `char_col` is drawn in, from the start of the
/// line - what horizontal scrolling counts in. Not the character index: a
/// line of CJK text is drawn twice as wide as it is long.
pub fn display_column(line: &str, char_col: usize) -> usize {
    line.chars().take(char_col).map(char_display_width).sum()
}

pub fn color_for_extension(ext: &str) -> Color {
    if ext.is_empty() {
        return crate::display::palette().file;
    }
    extension_color(ext, crate::display::light_background())
}

/// The colour hashed from an extension, for a light background or a dark one.
pub fn extension_color(ext: &str, light: bool) -> Color {
    // Simple hash of extension bytes.
    let hash: u32 = ext.bytes().fold(5381u32, |h, b| h.wrapping_mul(33).wrapping_add(b as u32));
    // Derive hue 0..360, keeping saturation and lightness high for a bright
    // look on a dark background - or low enough to read on a light one.
    let hue: f64 = (hash % 360) as f64;
    let (saturation, lightness): (f64, f64) = if light { (0.75, 0.32) } else { (0.7, 0.7) };
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
    Color::Rgb(((r1 + m) * 255.0) as u8, ((g1 + m) * 255.0) as u8, ((b1 + m) * 255.0) as u8)
}

/// A name as it is safe to draw. A file name can hold any byte but '/' and NUL,
/// escape sequences and tabs among them, and what a widget is given goes to the
/// terminal as it is: a name carrying an escape sequence would recolour the
/// screen, move the cursor or clear it, and a tab would shift everything drawn
/// after it out of its column. Control characters become '.', as they do in the
/// viewer, and one column stays one column so the layout still adds up.
pub fn printable_name(name: &str) -> String {
    if !name.chars().any(char::is_control) {
        return name.to_string();
    }
    name.chars().map(|character| if character.is_control() { '.' } else { character }).collect()
}

/// Shorten a path to the last `n` columns, marking the cut with "...". Walks
/// back a character at a time: slicing to a byte offset splits multi-byte
/// characters and panics.
pub fn limit_path_string(path: &Path, n: usize) -> String {
    // A directory's own name is as free as a file's, and this is what draws it.
    let path_string = printable_name(&path.display().to_string());
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

#[cfg(test)]
mod text_tests {
    use super::*;

    #[test]
    fn a_size_is_rounded_to_the_largest_unit_under_it() {
        // Binary, as F11 starts out; no test turns decimal on.
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1 KiB");
        assert_eq!(format_size(10 * 1024 * 1024), "10 MiB");
        assert_eq!(format_size(3 << 30), "3 GiB");
        // Past the last unit the figure just grows.
        assert_eq!(format_size(1u64 << 50), "1024 TiB");
        // What rounds up to a whole unit is written in it.
        assert_eq!(format_size(1024 * 1024 - 1), "1 MiB");
        assert_eq!(format_size(1023 * 1024 + 600), "1 MiB");
        assert_eq!(format_size(1023 * 1024 + 400), "1023 KiB");
    }

    #[test]
    fn thousands_are_marked_off_in_threes_from_the_right() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1 000");
        assert_eq!(grouped(12_345), "12 345");
        assert_eq!(grouped(1_234_567), "1 234 567");
        assert_eq!(grouped(u64::MAX), "18 446 744 073 709 551 615");
    }

    #[test]
    fn control_characters_in_a_name_are_drawn_as_dots() {
        assert_eq!(printable_name("plain.txt"), "plain.txt");
        assert_eq!(printable_name("日本語.txt"), "日本語.txt");
        assert_eq!(printable_name("a\tb\x1b[2J\n"), "a.b.[2J.");
        assert_eq!(display_width(&printable_name("x\x07y")), 3);
    }

    #[test]
    fn a_long_path_keeps_its_end() {
        let path = Path::new("/home/user/projects");
        assert_eq!(limit_path_string(path, 40), "/home/user/projects");
        assert_eq!(limit_path_string(path, 8), "...projects");
        // Wide characters are counted by the columns they take, and never cut.
        assert_eq!(limit_path_string(Path::new("/日本語"), 4), "...本語");
        assert_eq!(limit_path_string(Path::new("/日本語"), 5), "...本語");
        // A control character in it is made safe before it is measured.
        assert_eq!(limit_path_string(Path::new("/tmp/a\x1bb"), 3), "...a.b");
    }

    #[test]
    fn an_extension_always_gets_the_same_colour() {
        assert_eq!(extension_color("rs", false), extension_color("rs", false));
        assert_ne!(extension_color("rs", false), extension_color("rs", true));
        assert_ne!(extension_color("rs", false), extension_color("toml", false));
    }

    #[test]
    fn a_line_is_made_safe_to_draw_and_measured_as_drawn() {
        // A tab is four columns until F11 says otherwise.
        assert_eq!(printable_line("a\tb"), "a    b");
        assert_eq!(printable_line("\x1b[31mred"), ".[31mred");
        assert_eq!(line_display_width("a\tb"), 6);
        assert_eq!(line_display_width("日本"), 4);
    }

    #[test]
    fn a_word_without_braces_is_left_as_it_is() {
        assert_eq!(substitute("--wait", OsStr::new("/x")), "--wait");
        assert_eq!(substitute("{}{}", OsStr::new("ab")), "abab");
        assert_eq!(substitute("", OsStr::new("ab")), "");
    }
}

#[cfg(test)]
mod format_tests {
    use super::format_modified;
    use crate::options::DateFormat;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_relative_date_counts_back_from_now() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let ago = |seconds: u64| format_modified(now - Duration::from_secs(seconds), DateFormat::Relative, now);
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 min ago");
        assert_eq!(ago(3599), "59 min ago");
        assert_eq!(ago(3600 * 5), "5 h ago");
        assert_eq!(ago(86_400 * 29), "29 days ago");
        assert_eq!(ago(86_400 * 60), "2 mo ago");
        assert_eq!(ago(86_400 * 800), "2 yr ago");
        // Ahead of the clock.
        assert_eq!(format_modified(now + Duration::from_secs(90), DateFormat::Relative, now), "just now");
    }

    #[test]
    fn the_fixed_formats_fit_their_column() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        assert_eq!(format_modified(time, DateFormat::Short, time).len(), 14);
        assert_eq!(format_modified(time, DateFormat::Iso, time).len(), 16);
    }

    #[test]
    fn the_exact_form_follows_the_chosen_order() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let iso = super::format_exact(time, DateFormat::Iso);
        assert_eq!(iso.len(), 19);
        assert!(iso.starts_with("20") && iso.as_bytes()[4] == b'-', "{iso}");
        let short = super::format_exact(time, DateFormat::Short);
        assert_eq!(short.len(), 17);
        assert_eq!(short.as_bytes()[2], b'/');
        assert_eq!(super::format_exact(time, DateFormat::Relative), short);
    }
}

#[cfg(all(test, unix))]
mod substitute_tests {
    use super::substitute;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn a_path_goes_in_as_the_bytes_it_is() {
        let path = OsStr::from_bytes(b"/tmp/caf\xe9.txt");
        assert_eq!(substitute("{}", path), path);
        assert_eq!(substitute("--file={}", path).as_bytes(), b"--file=/tmp/caf\xe9.txt");
        assert_eq!(substitute("{}:{}", path).as_bytes(), b"/tmp/caf\xe9.txt:/tmp/caf\xe9.txt");
        assert_eq!(substitute("nvim", path), "nvim");
    }
}

#[cfg(test)]
mod column_tests {
    use super::*;

    #[test]
    fn a_column_counts_what_each_character_is_drawn_as() {
        assert_eq!(display_column("abc", 2), 2);
        assert_eq!(display_column("日本語", 2), 4);
        // A tab is four columns until F11 says otherwise, and a control
        // character the one '.' drawn in its place.
        assert_eq!(display_column("\tx", 1), 4);
        assert_eq!(display_column("\x1bx", 1), 1);
        assert_eq!(display_column("ab", 10), 2);
        assert_eq!(line_display_width("a\x07日"), 4);
    }
}
