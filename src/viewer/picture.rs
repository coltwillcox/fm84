//! Pictures drawn as coloured ASCII art, sized to the terminal's own cells.

use super::*;

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
pub(super) fn load_image(bytes: &[u8]) -> Option<(DynamicImage, String)> {
    let format = image::guess_format(bytes).ok()?;
    let image = image::load_from_memory_with_format(bytes, format).ok()?;
    let name = format.extensions_str().first().map_or_else(|| format!("{format:?}"), |ext| ext.to_uppercase());
    let label = format!("{} {}x{}", name, image.width(), image.height());

    let image = if image.width().max(image.height()) > IMAGE_MAX_SIDE { image.thumbnail(IMAGE_MAX_SIDE, IMAGE_MAX_SIDE) } else { image };
    Some((image, label))
}

/// The size to draw an image at in a `width` x `height` viewer, in characters.
/// Fit keeps the whole picture inside it; fill covers it, overflowing in one
/// direction; `zoom` then scales whichever of the two is the baseline.
pub fn image_size_for(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16) -> (usize, usize) {
    image_size_on(image, width, height, fill, zoom, cell_aspect())
}

/// The same, for a cell `cell` times taller than it is wide.
pub(super) fn image_size_on(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16, cell: f64) -> (usize, usize) {
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
pub(super) fn cell_aspect() -> f64 {
    crossterm::terminal::window_size()
        .ok()
        .filter(|size| size.rows > 0 && size.columns > 0 && size.width > 0 && size.height > 0)
        .map(|size| (f64::from(size.height) / f64::from(size.rows)) / (f64::from(size.width) / f64::from(size.columns)))
        .filter(|ratio| CELL_ASPECT_RANGE.contains(ratio))
        .unwrap_or(CELL_ASPECT_FALLBACK)
}

/// A colour channel with its low bits dropped, landing in the middle of the
/// band it now stands for rather than at the dark end of it.
pub(super) fn coarse(channel: u8) -> u8 {
    let band = 1u8 << IMAGE_COLOR_DROP_BITS;
    (channel & !(band - 1)) | (band / 2)
}

/// A `columns` x `rows` grid of characters approximating the image, with the
/// colour of each character. The character carries the brightness, and
/// transparent pixels are left blank, showing the viewer behind them.
///
/// On a dark background a dense character is a bright one. On a light theme
/// it is the other way round: ink is dark, so density stands for darkness.
pub fn image_to_ascii(image: &DynamicImage, columns: usize, rows: usize, backgrounds: bool) -> (Vec<String>, Vec<Vec<Color>>, Vec<Vec<Color>>) {
    image_to_ascii_on(image, columns, rows, crate::display::light_background(), backgrounds)
}

pub(super) fn image_to_ascii_on(image: &DynamicImage, columns: usize, rows: usize, light: bool, backgrounds: bool) -> (Vec<String>, Vec<Vec<Color>>, Vec<Vec<Color>>) {
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
                    let lifted = if luma < 128.0 { value + (255.0 - value) * IMAGE_GLYPH_CONTRAST } else { value * (1.0 - IMAGE_GLYPH_CONTRAST) };
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

#[cfg(test)]
mod tests;
