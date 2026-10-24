//! F3: a file read for the viewer - as text, as a hexdump or as a picture - and
//! the head of one read for F12's preview.

use crate::constants::{CELL_ASPECT_FALLBACK, CELL_ASPECT_RANGE, HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS, IMAGE_COLOR_DROP_BITS, IMAGE_GLYPH_CONTRAST, IMAGE_MAX_OVERFLOW, IMAGE_MAX_SIDE, IMAGE_RAMP, IMAGE_ZOOM_NORMAL, PREVIEW_HEX_BYTES};
use image::DynamicImage;
use image::imageops::FilterType;
use ratatui::style::Color;
use ratatui::text::Span;
use std::fs::File;
use std::io::{Error, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

mod hex;
mod highlight;
mod picture;
#[cfg(test)]
mod tests;

pub use self::{hex::*, highlight::*, picture::*};

/// What the viewer is showing of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Text,
    Hex,
    Image,
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
    /// Raw contents, held only while hex mode or an image needs them.
    pub bytes: Vec<u8>,
    pub mode: ViewMode,
    /// (anchor, cursor) as (line, column) into the line *as rendered*, so hex
    /// and text modes need no separate handling.
    pub selection: Option<((usize, usize), (usize, usize))>,
    /// Longest rendered line, so horizontal scrolling can stop at the end of
    /// the content instead of running on into empty space.
    pub max_line_width: usize,
    /// A decoded image, drawn as ASCII into `image_lines`.
    pub image: Option<DynamicImage>,
    /// The ASCII drawing of `image`.
    pub image_lines: Vec<String>,
    /// The colour of each character in `image_lines`.
    pub image_colors: Vec<Vec<Color>>,
    /// The colour behind each character. Empty when the option is off, in which
    /// case the terminal shows through the gaps in the glyphs and the darker
    /// parts of a picture lose their colour along with it.
    pub image_backgrounds: Vec<Vec<Color>>,
    /// The width `image_lines` was last drawn at; 0 when not yet.
    pub image_columns: usize,
    /// Cover the whole viewer and scroll the overflow, rather than fit inside it.
    pub image_fill: bool,
    /// Where a drag to pan a picture began: the pointer, and the offsets it
    /// started from. A press sets it, each drag measures against it, so the
    /// picture cannot creep from rounding over a long drag.
    pub pan_from: Option<((u16, u16), (usize, usize))>,
    /// What + and - have scaled the picture to, as a percentage of the Fit or
    /// Fill size, so those two keep deciding what 100% means.
    pub image_zoom: u16,
    /// W: long lines are broken onto as many rows as they need, rather than
    /// scrolled sideways. Text only - a hexdump and a picture have the width
    /// they have.
    pub wrap: bool,
    /// The rows lines are broken into while wrapping, each a line and the
    /// range of its characters on that row; empty when not. Scrolling, and
    /// `total_lines`, count these, while finding, selecting and copying go on
    /// working in whole lines - so a copy never gains a break that is only on
    /// screen, and a word split across two rows is still found.
    pub rows: Vec<(usize, usize, usize)>,
    /// The width `rows` were broken to; 0 when they are not there.
    pub rows_width: usize,
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

/// Read a file into the viewer. `decoded` is a picture somebody has already
/// decoded, from the cache that reads ahead while a folder is stepped through,
/// and skips the slow part. The file is still read either way, since hex mode
/// and the text fallback want the bytes and reading them is quick beside it.
pub fn load_file_content(path: &Path, decoded: Option<(DynamicImage, String)>) -> Result<ViewerState, Error> {
    // Get metadata once (single stat syscall)
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::new(std::io::ErrorKind::InvalidInput, format!("Not a regular file: {}", path.display())));
    }
    let file_size = metadata.len();

    // Check binary first
    if is_binary_file(path)? {
        let bytes = std::fs::read(path)?;
        // An image opens as ASCII art, drawn once the viewer width is known.
        // Anything that fails to decode is shown as the binary it is.
        if let Some((image, syntax_name)) = decoded.or_else(|| load_image(&bytes)) {
            return Ok(ViewerState {
                file_path: path.to_path_buf(),
                content_lines: Vec::new(),
                scroll_offset: 0,
                horizontal_offset: 0,
                total_lines: 1,
                file_size,
                syntax_name,
                from_edit: false,
                bytes,
                mode: ViewMode::Image,
                selection: None,
                max_line_width: 0,
                image: Some(image),
                image_columns: 0,
                image_fill: false,
                pan_from: None,
                image_zoom: IMAGE_ZOOM_NORMAL,
                image_lines: Vec::new(),
                image_colors: Vec::new(),
                image_backgrounds: Vec::new(),
                wrap: false,
                rows: Vec::new(),
                rows_width: 0,
            });
        }

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
            mode: ViewMode::Hex,
            selection: None,
            max_line_width: 0,
            image: None,
            image_columns: 0,
            image_fill: false,
            pan_from: None,
            image_zoom: IMAGE_ZOOM_NORMAL,
            image_lines: Vec::new(),
            image_colors: Vec::new(),
            image_backgrounds: Vec::new(),
            wrap: false,
            rows: Vec::new(),
            rows_width: 0,
        });
    }

    // Load text file. Only the head of it was checked, so a byte that is not
    // UTF-8 further in - a Latin-1 'é' in a file that starts out as plain
    // ASCII - can still turn up. Read strictly, that refused the whole file;
    // it is shown with a replacement character in its place instead, and X
    // still shows the bytes as they are.
    let content = match String::from_utf8(std::fs::read(path)?) {
        Ok(content) => content,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
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
        mode: ViewMode::Text,
        selection: None,
        max_line_width,
        image: None,
        image_columns: 0,
        image_fill: false,
        pan_from: None,
        image_zoom: IMAGE_ZOOM_NORMAL,
        image_lines: Vec::new(),
        image_colors: Vec::new(),
        image_backgrounds: Vec::new(),
        wrap: false,
        rows: Vec::new(),
        rows_width: 0,
    })
}

/// What a preview has to show: the head of a file as lines, or as the bytes a
/// binary one is made of.
///
/// The bytes are handed over as they are rather than laid out here, because how
/// many of them a row can hold is the pane's business - and the pane can be
/// resized without the cursor moving, which is the only thing that would gather
/// a preview again.
pub enum Preview {
    Lines(Vec<String>),
    Bytes(Vec<u8>),
}

/// The head of a file, capped in both bytes and lines. Reads lossily so a cut
/// multi-byte character at the cap can't fail the whole preview.
pub fn load_preview(path: &Path, max_bytes: u64, max_lines: usize) -> Preview {
    // Checked first: the preview follows the cursor, so opening a named pipe
    // here would freeze the app just for passing over one.
    if !crate::fs_ops::is_regular_file(path) {
        return Preview::Lines(vec!["Not a regular file".to_string()]);
    }
    let binary = is_binary_file(path).unwrap_or(false);

    let unreadable = || Preview::Lines(vec!["Cannot read file".to_string()]);
    let Ok(file) = File::open(path) else {
        return unreadable();
    };

    // A binary is read as far as the hexdump can show and no further; a text
    // file keeps its own cap, which the lines are then counted against.
    let mut buffer = Vec::new();
    if file.take(if binary { PREVIEW_HEX_BYTES } else { max_bytes }).read_to_end(&mut buffer).is_err() {
        return unreadable();
    }
    if binary {
        return Preview::Bytes(buffer);
    }

    Preview::Lines(String::from_utf8_lossy(&buffer).lines().take(max_lines).map(|line| line.to_string()).collect())
}

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
        match self.mode {
            ViewMode::Hex => {
                let offset = index * HEX_BYTES_PER_LINE;
                let stop = (offset + HEX_BYTES_PER_LINE).min(self.bytes.len());
                hex_line(offset, self.bytes.get(offset..stop).unwrap_or(&[]))
            }
            ViewMode::Image => self.image_lines.get(index).cloned().unwrap_or_default(),
            ViewMode::Text => self.content_lines.get(index).map(|line| crate::utils::printable_line(line)).unwrap_or_default(),
        }
    }

    /// Lines in the active mode; at least one, so there is always a row to draw.
    pub fn line_count(&self) -> usize {
        match self.mode {
            ViewMode::Hex => hex_line_count(&self.bytes),
            ViewMode::Image => self.image_lines.len().max(1),
            ViewMode::Text => self.content_lines.len().max(1),
        }
    }

    /// Whether lines are being broken onto rows: W is on, and it is text.
    pub fn wrapping(&self) -> bool {
        self.wrap && self.mode == ViewMode::Text && !self.from_edit
    }

    /// Row `index` on screen, as the line it shows and the range of that
    /// line's characters. Without wrapping, a row is a whole line.
    pub fn row(&self, index: usize) -> (usize, usize, usize) {
        match self.rows.last() {
            None => (index, 0, usize::MAX),
            Some(&last) => self.rows.get(index).copied().unwrap_or(last),
        }
    }

    /// The row on screen that holds character `col` of `line`.
    pub fn row_of(&self, line: usize, col: usize) -> usize {
        if self.rows.is_empty() {
            return line;
        }
        self.rows.partition_point(|&(row_line, from, _)| (row_line, from) <= (line, col)).saturating_sub(1)
    }

    /// The place in the text under `column` of row `row` on screen, as a line
    /// and a character in it - which is where a click lands.
    pub fn position_at(&self, row: usize, column: usize) -> (usize, usize) {
        let (line, from, to) = self.row(row.min(self.total_lines.saturating_sub(1)));
        let length = self.line_text(line).chars().count();
        (line, (from + column + self.horizontal_offset).min(to).min(length))
    }

    /// Break every line onto rows `width` wide, keeping the text that was at
    /// the top of the screen there.
    pub fn rewrap(&mut self, width: usize) {
        let (line, from, _) = self.row(self.scroll_offset);
        let rows: Vec<(usize, usize, usize)> = (0..self.line_count()).flat_map(|index| wrap_ranges(&self.line_text(index), width).into_iter().map(move |(from, to)| (index, from, to))).collect();
        self.rows = rows;
        self.rows_width = width;
        self.total_lines = self.rows.len().max(1);
        self.horizontal_offset = 0;
        self.scroll_offset = self.row_of(line, from).min(self.total_lines.saturating_sub(1));
    }

    /// Back to a row a line, keeping the line that was at the top there.
    pub fn unwrap_rows(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let (line, _, _) = self.row(self.scroll_offset);
        self.rows.clear();
        self.rows_width = 0;
        self.total_lines = self.line_count();
        self.scroll_offset = line;
    }

    /// The mode X moves on to: the picture comes first for an image, then its
    /// bytes as text and as hex.
    pub fn next_mode(&self) -> ViewMode {
        match self.mode {
            ViewMode::Image => ViewMode::Text,
            ViewMode::Text => ViewMode::Hex,
            ViewMode::Hex if self.image.is_some() => ViewMode::Image,
            ViewMode::Hex => ViewMode::Text,
        }
    }
}

/// Where a line breaks to fit `width` columns, as ranges of its characters:
/// after a space where there is one in reach, and inside a word only when it
/// is wider than a row by itself. Always at least one, so an empty line still
/// has a row of its own.
pub fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let characters: Vec<char> = text.chars().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let (mut end, mut used, mut after_space) = (start, 0, None);
        while end < characters.len() {
            let cell = unicode_width::UnicodeWidthChar::width(characters[end]).unwrap_or(0);
            // At least one character a row, however wide, so it moves on.
            if used + cell > width && end > start {
                break;
            }
            used += cell;
            if characters[end] == ' ' {
                after_space = Some(end + 1);
            }
            end += 1;
        }
        if end < characters.len()
            && let Some(after_space) = after_space
        {
            end = after_space;
        }
        ranges.push((start, end));
        start = end;
    }
    if ranges.is_empty() {
        ranges.push((0, 0));
    }
    ranges
}
