use crate::constants::{HEX_BYTES_PER_LINE, HEX_LINE_WIDTH};
use ratatui::style::Color;
use ratatui::text::Span;
use std::fs::File;
use std::io::{Error, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
static HIGHLIGHTER: OnceLock<Highlighter<'static>> = OnceLock::new();

/// Parser and highlighter state as it stands *before* a given line.
pub type LineState = (ParseState, HighlightState);

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn highlighter() -> &'static Highlighter<'static> {
    HIGHLIGHTER.get_or_init(|| {
        let themes = THEME_SET.get_or_init(ThemeSet::load_defaults);
        Highlighter::new(&themes.themes["base16-ocean.dark"])
    })
}

pub struct ViewerState {
    pub file_path: PathBuf,
    pub content_lines: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub total_lines: usize,
    pub file_size: u64,
    pub syntax_name: String,
    pub from_edit: bool,
    /// Raw contents, held only while hex mode needs them.
    pub bytes: Vec<u8>,
    pub hex: bool,
    /// (anchor, cursor) as (line, column) into the line *as rendered*, so hex
    /// and text modes need no separate handling.
    pub selection: Option<((usize, usize), (usize, usize))>,
    /// Longest rendered line, so horizontal scrolling can stop at the end of
    /// the content instead of running on into empty space.
    pub max_line_width: usize,
}

pub fn is_binary_file(path: &Path) -> Result<bool, Error> {
    let mut file = File::open(path)?;
    let mut buffer = [0; 512];
    let bytes_read = file.read(&mut buffer)?;

    // Check for null bytes
    if buffer[..bytes_read].contains(&0) {
        return Ok(true);
    }

    // Check UTF-8 validity (allow incomplete sequence at buffer boundary)
    match std::str::from_utf8(&buffer[..bytes_read]) {
        Ok(_) => Ok(false),
        // error_len() is None for truncated multi-byte sequence at end of buffer
        Err(e) => Ok(e.error_len().is_some()),
    }
}

pub fn load_file_content(path: &Path) -> Result<ViewerState, Error> {
    // Get metadata once (single stat syscall)
    let file_size = std::fs::metadata(path)?.len();

    // Check binary first
    if is_binary_file(path)? {
        let bytes = std::fs::read(path)?;
        return Ok(ViewerState {
            file_path: path.to_path_buf(),
            content_lines: Vec::new(),
            scroll_offset: 0,
            horizontal_offset: 0,
            total_lines: hex_line_count(&bytes),
            file_size,
            syntax_name: "Binary".to_string(),
            from_edit: false,
            bytes,
            hex: true,
            selection: None,
            max_line_width: 0,
        });
    }

    // Load text file
    let content = std::fs::read_to_string(path)?;
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    if content.ends_with('\n') {
        lines.push(String::new());
    }
    // An empty file still gets one (blank) line, the way the editor does it.
    // Claiming a line without holding one made the renderer slice past the end.
    if lines.is_empty() {
        lines.push(String::new());
    }
    let total_lines = lines.len();
    let max_line_width = lines.iter().map(|line| crate::utils::line_display_width(line)).max().unwrap_or(0);
    let syntax_name = detect_syntax(path);

    Ok(ViewerState {
        file_path: path.to_path_buf(),
        content_lines: lines,
        scroll_offset: 0,
        horizontal_offset: 0,
        total_lines,
        file_size,
        from_edit: false,
        syntax_name,
        bytes: Vec::new(),
        hex: false,
        selection: None,
        max_line_width,
    })
}

/// The head of a file, capped in both bytes and lines. Reads lossily so a cut
/// multi-byte character at the cap can't fail the whole preview.
pub fn load_preview(path: &Path, max_bytes: u64, max_lines: usize) -> Vec<String> {
    if is_binary_file(path).unwrap_or(false) {
        return vec!["Binary file".to_string()];
    }

    let Ok(file) = File::open(path) else {
        return vec!["Cannot read file".to_string()];
    };

    let mut buffer = Vec::new();
    if file.take(max_bytes).read_to_end(&mut buffer).is_err() {
        return vec!["Cannot read file".to_string()];
    }

    String::from_utf8_lossy(&buffer).lines().take(max_lines).map(|line| line.to_string()).collect()
}

/// Rows a hexdump of these bytes occupies; at least one, so an empty file still
/// has a line to render.
impl ViewerState {
    /// The selection in document order, or None when nothing is selected.
    pub fn selected_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (anchor, cursor) = self.selection?;
        if anchor == cursor {
            return None;
        }
        Some(if anchor < cursor { (anchor, cursor) } else { (cursor, anchor) })
    }

    /// The selected text, taken from the lines as drawn.
    pub fn selected_text(&self) -> Option<String> {
        let ((first_line, first_col), (last_line, last_col)) = self.selected_range()?;

        let mut text = String::new();
        for index in first_line..=last_line {
            let characters: Vec<char> = self.line_text(index).chars().collect();
            let to = if index == last_line { last_col.min(characters.len()) } else { characters.len() };
            let from = if index == first_line { first_col.min(to) } else { 0 };

            if index > first_line {
                text.push('\n');
            }
            text.extend(&characters[from..to]);
        }
        Some(text)
    }

    /// A line as the viewer draws it, whichever mode is active.
    pub fn line_text(&self, index: usize) -> String {
        if self.hex {
            let offset = index * HEX_BYTES_PER_LINE;
            let stop = (offset + HEX_BYTES_PER_LINE).min(self.bytes.len());
            hex_line(offset, self.bytes.get(offset..stop).unwrap_or(&[]))
        } else {
            self.content_lines.get(index).map(|line| crate::utils::printable_line(line)).unwrap_or_default()
        }
    }
}

pub fn hex_line_count(bytes: &[u8]) -> usize {
    bytes.len().div_ceil(HEX_BYTES_PER_LINE).max(1)
}

/// One `hexdump -C` row: offset, sixteen bytes split into two groups, then the
/// printable characters. Short rows are padded so the gutter stays aligned.
pub fn hex_line(offset: usize, bytes: &[u8]) -> String {
    let mut text = String::with_capacity(HEX_LINE_WIDTH);
    text.push_str(&format!("{offset:08x}  "));

    for index in 0..HEX_BYTES_PER_LINE {
        if index == HEX_BYTES_PER_LINE / 2 {
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

pub fn detect_syntax(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()) {
        Some("rs") => "Rust".to_string(),
        Some("py") => "Python".to_string(),
        Some("js") | Some("jsx") => "JavaScript".to_string(),
        Some("ts") | Some("tsx") => "TypeScript".to_string(),
        Some("json") => "JSON".to_string(),
        Some("toml") => "TOML".to_string(),
        Some("yaml") | Some("yml") => "YAML".to_string(),
        Some("md") => "Markdown".to_string(),
        Some("sh") | Some("bash") => "Shell".to_string(),
        Some("c") | Some("h") => "C".to_string(),
        Some("cpp") | Some("hpp") => "C++".to_string(),
        Some("html") => "HTML".to_string(),
        Some("css") => "CSS".to_string(),
        _ => "Plain Text".to_string(),
    }
}

/// Highlight a whole file, recording the parser state before each line.
pub fn highlight_all(content: &[String], extension: &str) -> (Vec<Vec<Span<'static>>>, Vec<LineState>) {
    let mut spans = Vec::new();
    let mut states = Vec::new();
    highlight_from(content, extension, 0, &mut spans, &mut states);
    (spans, states)
}

/// Re-highlight from `from` downward, resuming from the cached parser state
/// rather than re-parsing the file from the top, and stopping again as soon as
/// the state matches the cache - past that point the existing spans still hold.
/// Both vectors are edited in place, so an ordinary keystroke touches a couple
/// of entries no matter how long the file is.
pub fn highlight_from(
    content: &[String],
    extension: &str,
    from: usize,
    spans: &mut Vec<Vec<Span<'static>>>,
    states: &mut Vec<LineState>,
) {
    let ps = syntax_set();
    let hlr = highlighter();
    let syntax = ps
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| ps.find_syntax_plain_text());
    let fresh = || (ParseState::new(syntax), HighlightState::new(hlr, ScopeStack::new()));

    let mut start = from;
    // Lines added or removed shift the cache; splice it back into alignment so
    // index i again describes line i. The spliced slots get rewritten below.
    let delta = content.len() as isize - states.len() as isize;
    if !states.is_empty() && states.len() == spans.len() {
        start = from.min(states.len() - 1);
        let at = (start + 1).min(states.len());
        if delta > 0 {
            let filler = states[start].clone();
            for _ in 0..delta {
                states.insert(at, filler.clone());
                spans.insert(at, Vec::new());
            }
        } else {
            for _ in 0..-delta {
                if at < states.len() {
                    states.remove(at);
                    spans.remove(at);
                }
            }
        }
    }

    // Anything we could not reconcile: start over from the top.
    let resumable = states.len() == content.len() && spans.len() == content.len();
    if !resumable {
        states.clear();
        spans.clear();
        states.reserve(content.len());
        spans.reserve(content.len());
        start = 0;
    }

    let (mut parse, mut hl) = if resumable { states[start].clone() } else { fresh() };
    // The cache is only trustworthy below the spliced region.
    let min_converge = if resumable { start + 1 + delta.max(0) as usize } else { usize::MAX };

    let mut index = start;
    while index < content.len() {
        if index >= min_converge
            && let Some((cached_parse, cached_hl)) = states.get(index)
            && *cached_parse == parse
            && *cached_hl == hl
        {
            break;
        }

        let entering = (parse.clone(), hl.clone());
        let line = &content[index];
        let ops = parse.parse_line(line, ps).unwrap_or_default();
        let line_spans: Vec<Span<'static>> = HighlightIterator::new(&mut hl, &ops, line, hlr)
            .map(|(style, text)| {
                Span::styled(
                    text.to_string(),
                    ratatui::style::Style::default().fg(syntect_to_ratatui_color(style.foreground)),
                )
            })
            .collect();

        if index < states.len() {
            states[index] = entering;
            spans[index] = line_spans;
        } else {
            states.push(entering);
            spans.push(line_spans);
        }
        index += 1;
    }
}

fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(count: usize) -> Vec<String> {
        let template = [
            "pub fn item_{}(value: u32) -> Result<String, Error> {",
            "    let text = format!(\"item {} of the set\", value);",
            "    match value { 0 => Err(Error::Empty), _ => Ok(text) }",
            "}",
        ];
        (0..count)
            .map(|i| template[i % template.len()].replace("{}", &i.to_string()))
            .collect()
    }

    /// Spans reduced to comparable data.
    fn shape(spans: &[Vec<Span<'static>>]) -> Vec<Vec<(String, Option<Color>)>> {
        spans
            .iter()
            .map(|line| line.iter().map(|s| (s.content.to_string(), s.style.fg)).collect())
            .collect()
    }

    fn assert_matches_full(lines: &[String], spans: &[Vec<Span<'static>>], states: &[LineState], case: &str) {
        let (expected, _) = highlight_all(lines, "rs");
        assert_eq!(shape(spans), shape(&expected), "spans diverged after {case}");
        assert_eq!(spans.len(), lines.len(), "span count wrong after {case}");
        assert_eq!(states.len(), lines.len(), "state count wrong after {case}");
    }

    #[test]
    fn incremental_matches_full_rehighlight() {
        let mut lines = sample(200);
        let (mut spans, mut states) = highlight_all(&lines, "rs");
        assert_matches_full(&lines, &spans, &states, "initial load");

        // Edit inside one line - should converge almost immediately.
        lines[50].push_str(" // trailing note");
        highlight_from(&lines, "rs", 50, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "in-line edit");

        // Open a block comment: every line below changes meaning, so the
        // convergence check must NOT stop early here.
        lines.insert(10, "/* opening a block".to_string());
        highlight_from(&lines, "rs", 10, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "block comment opened");

        // Close it again.
        lines.insert(11, "closing it */".to_string());
        highlight_from(&lines, "rs", 11, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "block comment closed");

        // Remove a line (cache shifts the other way).
        lines.remove(11);
        highlight_from(&lines, "rs", 11, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "line removed");

        // Edit the very last line.
        let last = lines.len() - 1;
        lines[last].push_str("// end");
        highlight_from(&lines, "rs", last, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "last-line edit");

        // Append past the old end.
        lines.push("fn tail() {}".to_string());
        highlight_from(&lines, "rs", lines.len() - 1, &mut spans, &mut states);
        assert_matches_full(&lines, &spans, &states, "appended line");
    }
}

