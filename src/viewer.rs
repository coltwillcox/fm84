use crate::constants::{
    CELL_ASPECT_FALLBACK, CELL_ASPECT_RANGE, HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS, IMAGE_COLOR_DROP_BITS, IMAGE_GLYPH_CONTRAST,
    IMAGE_MAX_OVERFLOW, IMAGE_MAX_SIDE, IMAGE_RAMP, IMAGE_ZOOM_NORMAL, PREVIEW_HEX_BYTES,
};
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

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
static HIGHLIGHTERS: OnceLock<std::collections::HashMap<&'static str, Highlighter<'static>>> = OnceLock::new();

/// Parser and highlighter state as it stands *before* a given line.
pub type LineState = (ParseState, HighlightState);

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The highlighter for the palette showing, so a light theme gets dark code.
/// The theme cannot change while a file is open - F11 is not reachable from
/// the viewer or the editor - so the highlight state an open file carries was
/// always made by the highlighter it goes back to.
fn highlighter() -> &'static Highlighter<'static> {
    let highlighters = HIGHLIGHTERS.get_or_init(|| {
        let themes = THEME_SET.get_or_init(ThemeSet::load_defaults);
        crate::options::Theme::ALL
            .iter()
            .map(|&theme| crate::display::palette_of(theme).syntax_theme)
            .filter_map(|name| themes.themes.get(name).map(|theme| (name, Highlighter::new(theme))))
            .collect()
    });
    highlighters
        .get(crate::display::palette().syntax_theme)
        .unwrap_or_else(|| highlighters.values().next().expect("syntect ships its default themes"))
}

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
    })
}

/// A picture's dimensions and the memory it will occupy once decoded, read from
/// its header alone - not a pixel of it. Four bytes per pixel is what RGBA
/// costs; a photo without transparency needs three, so this errs high, which is
/// the safe side for a warning. None for anything that is not a picture.
pub fn image_cost(path: &Path) -> Option<((u32, u32), u64)> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    Some(((width, height), u64::from(width) * u64::from(height) * 4))
}

/// Decode a picture from a file, for reading one ahead off the main thread.
/// None for anything that is not a picture this build can read.
pub fn decode_image(path: &Path) -> Option<(DynamicImage, String)> {
    load_image(&std::fs::read(path).ok()?)
}

/// Decode an image by its content, not its extension, along with the status bar
/// label for it. None for anything that is not an image this build can read.
fn load_image(bytes: &[u8]) -> Option<(DynamicImage, String)> {
    let format = image::guess_format(bytes).ok()?;
    let image = image::load_from_memory_with_format(bytes, format).ok()?;
    let name = format.extensions_str().first().map_or_else(|| format!("{format:?}"), |ext| ext.to_uppercase());
    let label = format!("{} {}x{}", name, image.width(), image.height());

    let image = if image.width().max(image.height()) > IMAGE_MAX_SIDE {
        image.thumbnail(IMAGE_MAX_SIDE, IMAGE_MAX_SIDE)
    } else {
        image
    };
    Some((image, label))
}

/// The size to draw an image at in a `width` x `height` viewer, in characters.
/// Fit keeps the whole picture inside it; fill covers it, overflowing in one
/// direction; `zoom` then scales whichever of the two is the baseline.
pub fn image_size_for(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16) -> (usize, usize) {
    image_size_on(image, width, height, fill, zoom, cell_aspect())
}

/// The same, for a cell `cell` times taller than it is wide.
fn image_size_on(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16, cell: f64) -> (usize, usize) {
    // The width at which the drawing is exactly `height` rows tall.
    let full_height = height as f64 * cell * image.width() as f64 / image.height().max(1) as f64;
    let base = if fill { width.max(full_height.ceil() as usize) } else { width.min(full_height.floor() as usize) };
    let columns = base * zoom as usize / IMAGE_ZOOM_NORMAL as usize;
    // Covering the width of a one-pixel-wide strip means a drawing 102,400 rows
    // tall - half a second to build and 160 MB to hold, and again on every
    // resize. Hold the overflow to a few screens each way, which lowers the
    // column count without touching the aspect ratio. `full_height` scaled by
    // the same factor is the width at which the drawing is that many screens
    // tall, so the two caps mirror each other.
    let overflow = IMAGE_MAX_OVERFLOW as f64;
    let columns = columns.min(width * IMAGE_MAX_OVERFLOW).min((full_height * overflow).ceil() as usize).max(1);

    let rows = aspect_rows_on(image, columns, cell);
    // Fit overflows only for a picture more than twice as tall as the viewer
    // per column of its width - past 100:1 in a typical pane - where even a
    // single column is too wide to keep the proportions. Squash it into the
    // viewer rather than hand Fit something it cannot show whole; a sliver that
    // thin has no shape left to distort. Zooming in asks to see the picture
    // bigger than the viewer, so it scrolls rather than being squashed.
    if !fill && zoom <= IMAGE_ZOOM_NORMAL && rows > height { (columns, height.max(1)) } else { (columns, rows) }
}

/// Rows that keep the picture's proportions at `columns` wide, on a cell `cell`
/// times taller than it is wide: a square image draws that many times fewer
/// rows than it has columns.
fn aspect_rows_on(image: &DynamicImage, columns: usize, cell: f64) -> usize {
    (image.height() as f64 * columns as f64 / image.width().max(1) as f64 / cell).round().max(1.0) as usize
}

/// How many times taller this terminal's cells are than they are wide, which is
/// what turns a picture's proportions into rows and columns.
///
/// Terminals report their size in pixels alongside their size in cells, and the
/// two together give the cell. Assuming 2 instead - an ordinary monospace cell,
/// and what fm84 did until now - stretches a picture by however far the real
/// cell is from that: Victor Mono at 13pt in kitty measures 9x25, so every
/// picture came out 39% too tall. Costs one ioctl, on the way to a redraw that
/// is about to resample the whole image.
fn cell_aspect() -> f64 {
    crossterm::terminal::window_size()
        .ok()
        .filter(|size| size.rows > 0 && size.columns > 0 && size.width > 0 && size.height > 0)
        .map(|size| (f64::from(size.height) / f64::from(size.rows)) / (f64::from(size.width) / f64::from(size.columns)))
        .filter(|ratio| CELL_ASPECT_RANGE.contains(ratio))
        .unwrap_or(CELL_ASPECT_FALLBACK)
}

/// A colour channel with its low bits dropped, landing in the middle of the
/// band it now stands for rather than at the dark end of it.
fn coarse(channel: u8) -> u8 {
    let band = 1u8 << IMAGE_COLOR_DROP_BITS;
    (channel & !(band - 1)) | (band / 2)
}

/// A `columns` x `rows` grid of characters approximating the image, with the
/// colour of each character. The character carries the brightness, and
/// transparent pixels are left blank, showing the viewer behind them.
///
/// On a dark background a dense character is a bright one. On a light theme
/// it is the other way round: ink is dark, so density stands for darkness.
pub fn image_to_ascii(
    image: &DynamicImage,
    columns: usize,
    rows: usize,
    backgrounds: bool,
) -> (Vec<String>, Vec<Vec<Color>>, Vec<Vec<Color>>) {
    image_to_ascii_on(image, columns, rows, crate::display::light_background(), backgrounds)
}

fn image_to_ascii_on(
    image: &DynamicImage,
    columns: usize,
    rows: usize,
    light: bool,
    backgrounds: bool,
) -> (Vec<String>, Vec<Vec<Color>>, Vec<Vec<Color>>) {
    let (columns, rows) = (columns.max(1) as u32, rows.max(1) as u32);
    let small = image.resize_exact(columns, rows, FilterType::Triangle).to_rgba8();

    let mut lines = Vec::with_capacity(rows as usize);
    let mut inks = Vec::with_capacity(rows as usize);
    let mut backs = Vec::with_capacity(rows as usize);

    for row in small.rows() {
        let mut line = String::with_capacity(columns as usize);
        let mut ink_row = Vec::with_capacity(columns as usize);
        let mut back_row = Vec::with_capacity(columns as usize);

        for pixel in row {
            let [red, green, blue, alpha] = pixel.0;
            let luma = 0.299 * red as f64 + 0.587 * green as f64 + 0.114 * blue as f64;
            let ink = if light { 255.0 - luma } else { luma };
            let level = ink * alpha as f64 / 255.0 / 255.0;
            line.push(IMAGE_RAMP[(level * (IMAGE_RAMP.len() - 1) as f64).round() as usize] as char);

            let colour = [coarse(red), coarse(green), coarse(blue)];
            if backgrounds {
                // The cell's own colour goes behind, so it reads right whatever
                // the glyph covers, and the glyph is pulled away from it far
                // enough to stay legible - lighter on a dark pixel, darker on a
                // bright one, so neither end of the picture flattens out.
                let shift = |value: u8| {
                    let value = value as f64;
                    let lifted =
                        if luma < 128.0 { value + (255.0 - value) * IMAGE_GLYPH_CONTRAST } else { value * (1.0 - IMAGE_GLYPH_CONTRAST) };
                    lifted.round() as u8
                };
                ink_row.push(Color::Rgb(shift(colour[0]), shift(colour[1]), shift(colour[2])));
                back_row.push(Color::Rgb(colour[0], colour[1], colour[2]));
            } else {
                ink_row.push(Color::Rgb(colour[0], colour[1], colour[2]));
            }
        }

        lines.push(line);
        inks.push(ink_row);
        if backgrounds {
            backs.push(back_row);
        }
    }

    (lines, inks, backs)
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
            ViewMode::Text => {
                self.content_lines.get(index).map(|line| crate::utils::printable_line(line)).unwrap_or_default()
            }
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

    /// Every hexdump layout measures what it says it does, and the viewer's own
    /// still comes to the width the horizontal scroll is bounded by.
    #[test]
    fn hex_rows_measure_what_they_claim() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        for (per_line, digits) in [(16, 8), (8, 4), (8, 0), (4, 0), (1, 8)] {
            let full = hex_row(0, &bytes[..per_line], per_line, digits);
            assert_eq!(full.chars().count(), hex_row_width(per_line, digits), "{per_line} bytes, {digits} digits: {full}");
            // A short last row pads its bytes, so only the gutter is shorter.
            let short = hex_row(0, &bytes[..1], per_line, digits);
            assert_eq!(short.chars().count(), hex_row_width(per_line, digits) - (per_line - 1), "{short}");
        }
        assert_eq!(hex_row_width(HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS), crate::constants::HEX_LINE_WIDTH);
        assert_eq!(hex_line(0, &bytes[..16]), hex_row(0, &bytes[..16], HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS));
    }

    /// A binary preview hands over bytes for the pane to lay out; a text one
    /// still comes as lines.
    #[test]
    fn previews_binary_as_bytes_and_text_as_lines() {
        let dir = std::env::temp_dir().join(format!("fm84-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let binary = dir.join("binary");
        std::fs::write(&binary, [0x7f, b'E', b'L', b'F', 0, 1, 2, 3]).unwrap();
        let Preview::Bytes(bytes) = load_preview(&binary, 1024, 10) else {
            panic!("a file with a null byte in it is a binary");
        };
        assert_eq!(bytes, [0x7f, b'E', b'L', b'F', 0, 1, 2, 3]);

        let text = dir.join("text");
        std::fs::write(&text, "one\ntwo\nthree\n").unwrap();
        let Preview::Lines(lines) = load_preview(&text, 1024, 2) else {
            panic!("text is still lines");
        };
        assert_eq!(lines, ["one", "two"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

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


#[cfg(test)]
mod image_tests {
    use super::*;
    use crate::constants::{IMAGE_MAX_DECODED, IMAGE_ZOOM_NORMAL, IMAGE_ZOOM_STEPS};
    use image::{Rgba, RgbaImage};

    /// Sizes as an ordinary 2:1 cell gives them. The tests below are written
    /// around that ratio, and the terminal a test run happens to be watched
    /// from is no business of theirs.
    fn image_size_for(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16) -> (usize, usize) {
        image_size_on(image, width, height, fill, zoom, CELL_ASPECT_FALLBACK)
    }

    fn solid(width: u32, height: u32, pixel: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(pixel)))
    }

    #[test]
    fn colours_share_bands_and_stay_in_range() {
        // Neighbouring values collapse into one band, centred within it.
        let band = 1u8 << IMAGE_COLOR_DROP_BITS;
        assert_eq!(coarse(0), band / 2);
        assert_eq!(coarse(band - 1), band / 2);
        assert_eq!(coarse(band), band + band / 2);
        assert!((0..=255u8).all(|value| coarse(value).abs_diff(value) <= band / 2));
    }

    #[test]
    fn ramp_ends_map_to_black_and_white() {
        assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 255]), 4, 2, false, false).0[0], "    ");
        assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 255]), 4, 2, false, false).0[0], "@@@@");
    }

    #[test]
    fn a_light_background_turns_the_ramp_round() {
        assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 255]), 4, 2, true, false).0[0], "@@@@");
        assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 255]), 4, 2, true, false).0[0], "    ");
    }

    #[test]
    fn transparent_is_left_blank() {
        for light in [false, true] {
            assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 0]), 4, 2, light, false).0[0], "    ");
            assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 0]), 4, 2, light, false).0[0], "    ");
        }
    }

    #[test]
    fn backgrounds_carry_the_cell_colour() {
        let red = [200u8, 40, 60, 255];
        let want = Color::Rgb(coarse(200), coarse(40), coarse(60));

        // Off: nothing behind the characters, and the character itself carries
        // the colour, exactly as before.
        let (_, inks, backs) = image_to_ascii_on(&solid(4, 4, red), 4, 2, false, false);
        assert!(backs.is_empty());
        assert_eq!(inks[0][0], want);

        // On: the cell's own colour goes behind it, so the colour is right
        // whatever the glyph happens to cover.
        let (_, inks, backs) = image_to_ascii_on(&solid(4, 4, red), 4, 2, false, true);
        assert_eq!(backs.len(), inks.len());
        assert_eq!(backs[0].len(), inks[0].len());
        assert_eq!(backs[0][0], want);
        // And the glyph is moved off it, or there would be nothing to see.
        assert_ne!(inks[0][0], backs[0][0]);
    }

    #[test]
    fn a_glyph_lifts_off_a_dark_cell_and_sinks_into_a_bright_one() {
        let level = |colour: Color| match colour {
            Color::Rgb(red, green, blue) => red as u32 + green as u32 + blue as u32,
            _ => unreachable!("image cells are always true colour"),
        };

        let (_, inks, backs) = image_to_ascii_on(&solid(2, 2, [20, 20, 20, 255]), 2, 1, false, true);
        assert!(level(inks[0][0]) > level(backs[0][0]), "a dark cell needs a lighter glyph");

        let (_, inks, backs) = image_to_ascii_on(&solid(2, 2, [240, 240, 240, 255]), 2, 1, false, true);
        assert!(level(inks[0][0]) < level(backs[0][0]), "a bright cell needs a darker glyph");
    }

    #[test]
    fn rows_are_halved_for_the_cell_aspect() {
        let square = solid(100, 100, [128, 128, 128, 255]);
        let (columns, rows) = image_size_for(&square, 40, 40, false, IMAGE_ZOOM_NORMAL);
        assert_eq!((columns, rows), (40, 20));

        let (lines, colors, _) = image_to_ascii_on(&square, columns, rows, false, false);
        assert_eq!(lines.len(), 20);
        assert!(lines.iter().all(|line| line.chars().count() == 40));
        assert_eq!(colors.len(), 20);
        assert!(colors.iter().flatten().all(|color| *color == Color::Rgb(coarse(128), coarse(128), coarse(128))));
        // A very wide image still keeps one row.
        assert_eq!(image_size_for(&solid(1000, 1, [0, 0, 0, 255]), 10, 10, false, IMAGE_ZOOM_NORMAL).1, 1);
    }

    /// A drawing keeps the picture's shape on whatever cell the terminal has,
    /// not only on the 2:1 one fm84 used to assume. Measured as the drawing's
    /// shape on screen - columns of cells that wide, rows of cells that tall -
    /// against the picture's own.
    #[test]
    fn keeps_proportions_on_any_cell() {
        for cell in [1.0, 1.6, 2.0, 2.4, 2.78, 3.5] {
            for (width, height) in [(400u32, 300u32), (100, 100), (192, 108), (60, 90)] {
                let image = solid(width, height, [128, 128, 128, 255]);
                let (columns, rows) = image_size_on(&image, 120, 40, false, IMAGE_ZOOM_NORMAL, cell);
                let drawn = columns as f64 / (rows as f64 * cell);
                let wanted = f64::from(width) / f64::from(height);
                // A row is a coarse unit: at 40 rows one of them is 2.5% of the
                // height, and the count is rounded to a whole one.
                assert!(
                    (drawn / wanted - 1.0).abs() < 0.05,
                    "{width}x{height} on a {cell} cell drew {columns}x{rows}, shape {drawn:.3} against {wanted:.3}"
                );
            }
        }
    }

    /// A terminal that says nothing useful about its cells leaves the drawing
    /// where it always was.
    #[test]
    fn cell_aspect_is_believable() {
        let ratio = cell_aspect();
        assert!(CELL_ASPECT_RANGE.contains(&ratio), "{ratio}");
    }

    #[test]
    fn decodes_by_content_and_rejects_the_rest() {
        let mut png = Vec::new();
        solid(3, 2, [255, 0, 0, 255]).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (image, label) = load_image(&png).unwrap();
        assert_eq!((image.width(), image.height()), (3, 2));
        assert_eq!(label, "PNG 3x2");
        assert!(load_image(b"\0\x01\x02 not an image").is_none());
    }

    #[test]
    fn fit_stays_inside_and_fill_covers() {
        // Cell aspect halves rows, so a square image is twice as wide as tall.
        let square = solid(100, 100, [0, 0, 0, 255]);
        assert_eq!(image_size_for(&square, 80, 20, false, IMAGE_ZOOM_NORMAL).0, 40);
        assert_eq!(image_size_for(&square, 80, 20, true, IMAGE_ZOOM_NORMAL).0, 80);
        assert_eq!(image_size_for(&square, 30, 20, false, IMAGE_ZOOM_NORMAL).0, 30);
        assert_eq!(image_size_for(&square, 30, 20, true, IMAGE_ZOOM_NORMAL).0, 40);

        // Fit is inside the viewer for every shape, including the slivers that
        // have to be squashed to get there.
        for (image_width, image_height) in [(100, 100), (1024, 576), (1, 1024), (4, 1024), (1024, 1)] {
            let image = solid(image_width, image_height, [0, 0, 0, 255]);
            for (width, height) in [(80, 20), (30, 20), (200, 50), (7, 3)] {
                let (columns, rows) = image_size_for(&image, width, height, false, IMAGE_ZOOM_NORMAL);
                assert!(
                    columns <= width && rows <= height,
                    "{image_width}x{image_height} fit in {width}x{height}: {columns}x{rows}"
                );
                let drawn = image_to_ascii_on(&image, columns, rows, false, false).0;
                assert_eq!(drawn.len(), rows);
            }
        }

        // Fill covers the viewer, except where the cap in the test below stops it.
        for (width, height) in [(80, 20), (30, 20), (200, 50), (7, 3)] {
            let (columns, rows) = image_size_for(&square, width, height, true, IMAGE_ZOOM_NORMAL);
            assert!(columns >= width && rows >= height);
        }
    }

    #[test]
    fn extreme_aspects_stay_cheap_to_draw() {
        const GREY: [u8; 4] = [128, 128, 128, 255];
        let (width, height) = (200, 50);
        // Both caps at once is the worst the sizing can produce.
        let budget = width * height * IMAGE_MAX_OVERFLOW * IMAGE_MAX_OVERFLOW;
        for (image_width, image_height) in [(1, 1024), (2, 1024), (4, 1024), (1024, 1), (1024, 2), (1024, 576)] {
            let image = solid(image_width, image_height, GREY);
            for fill in [false, true] {
                // Every rung of the zoom ladder, since zooming in is the one
                // thing that could put the blow-up back.
                for zoom in IMAGE_ZOOM_STEPS {
                    let (columns, rows) = image_size_for(&image, width, height, fill, zoom);
                    let (lines, colors, _) = image_to_ascii_on(&image, columns, rows, false, false);
                    let cells = columns * lines.len();
                    assert!(
                        cells <= budget,
                        "{image_width}x{image_height} fill={fill} zoom={zoom}: {columns}x{} is {cells} cells",
                        lines.len()
                    );
                    assert_eq!(colors.len(), lines.len());
                }
            }
        }

        // A photo is untouched by the cap: fill still covers the pane exactly.
        assert_eq!(image_size_for(&solid(1024, 576, GREY), width, height, true, IMAGE_ZOOM_NORMAL).0, width);
        assert_eq!(image_size_for(&solid(1024, 576, GREY), width, height, false, IMAGE_ZOOM_NORMAL).0, 177);
    }

    #[test]
    fn cost_comes_from_the_header_not_the_file_size() {
        let dir = std::env::temp_dir().join(format!("fm84-image-cost-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let png = dir.join("picture.png");
        solid(600, 400, [1, 2, 3, 255]).save(&png).unwrap();
        assert_eq!(image_cost(&png).unwrap(), ((600, 400), 600 * 400 * 4));
        // The gap between the two is the whole point: this one is 164x.
        assert!(std::fs::metadata(&png).unwrap().len() * 100 < 600 * 400 * 4);

        let text = dir.join("notes.txt");
        std::fs::write(&text, "hello\n").unwrap();
        assert!(image_cost(&text).is_none());
        assert!(image_cost(&dir.join("absent.png")).is_none());

        // Where the prompt falls: a 24 MP photo opens straight away, while the
        // 12000x12000 picture that unpacks to 549 MiB is asked about first.
        const { assert!(6000 * 4000 * 4 < IMAGE_MAX_DECODED) };
        const { assert!(12000 * 12000 * 4 > IMAGE_MAX_DECODED) };

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zoom_scales_the_fitted_size_and_stops_at_the_cap() {
        let (width, height) = (200, 50);
        let photo = solid(1024, 576, [128, 128, 128, 255]);
        let fitted = image_size_for(&photo, width, height, false, IMAGE_ZOOM_NORMAL).0;

        // Each rung scales the fitted width by its own percentage.
        for zoom in IMAGE_ZOOM_STEPS {
            let (columns, _) = image_size_for(&photo, width, height, false, zoom);
            let wanted = fitted * zoom as usize / 100;
            assert!(columns.abs_diff(wanted) <= 1, "zoom {zoom}%: {columns} columns, wanted about {wanted}");
        }

        // Zooming out shrinks it, zooming in overflows the pane and scrolls.
        assert!(image_size_for(&photo, width, height, false, 25).0 < fitted);
        assert!(image_size_for(&photo, width, height, false, 400).1 > height);

        // The ladder tops out where the drawing is capped, so the last rung is
        // the last one that changes anything.
        assert_eq!(*IMAGE_ZOOM_STEPS.last().unwrap() as usize, IMAGE_MAX_OVERFLOW * 100);
        assert!(IMAGE_ZOOM_STEPS.contains(&IMAGE_ZOOM_NORMAL));
        assert!(IMAGE_ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn x_cycles_image_text_hex_and_text_hex() {
        let dir = std::env::temp_dir().join(format!("fm84-view-modes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("picture.png");
        solid(3, 2, [255, 0, 0, 255]).save(&png).unwrap();
        let text = dir.join("notes.txt");
        std::fs::write(&text, "hello\n").unwrap();

        let mut state = load_file_content(&png, None).unwrap();
        let mut seen = vec![state.mode];
        for _ in 0..3 {
            state.mode = state.next_mode();
            seen.push(state.mode);
        }
        assert_eq!(seen, [ViewMode::Image, ViewMode::Text, ViewMode::Hex, ViewMode::Image]);

        let mut state = load_file_content(&text, None).unwrap();
        let mut seen = vec![state.mode];
        for _ in 0..2 {
            state.mode = state.next_mode();
            seen.push(state.mode);
        }
        assert_eq!(seen, [ViewMode::Text, ViewMode::Hex, ViewMode::Text]);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
