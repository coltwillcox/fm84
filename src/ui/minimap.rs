//! F11's minimap, drawn beside the viewer's, the editor's or the diff's
//! text: each half cell as dark or as bright as its rows are full, the
//! matches of a search and the differences between two files lit on it,
//! and the rows on screen shaded behind.

use super::*;
use crate::minimap::{Minimap, fullness};
use ratatui::style::Color;

/// The narrowest the text is left beside a map; narrower, there is none.
const LEAST_TEXT: u16 = 30;

/// What one row of text shows on the map: how full each band of it is, one
/// to a column, and a colour to light it in instead - a match, or a
/// difference.
pub(super) struct MapRow {
    pub fill: Vec<f64>,
    pub mark: Option<Color>,
}

impl MapRow {
    /// A row of text as it reads across the map's `columns`, lit when
    /// `marked`.
    pub(super) fn of(text: &str, columns: usize, marked: bool) -> MapRow {
        MapRow {
            fill: fullness(text, columns),
            mark: marked.then(|| palette().title),
        }
    }
}

/// The text's `area` with room taken off its right for a map, when F11 has
/// one, there is more than the view shows, and the text is left room
/// enough: the text's area, and the map's.
/// The map is as wide as F11 has it, with a column's gap before it.
pub(super) fn make_room(area: Rect, content: usize, view: usize, options: &Options) -> (Rect, Option<Minimap>) {
    let width = options.minimap_width;
    if width == 0 || content <= view || area.width < LEAST_TEXT + width + 1 || area.height == 0 {
        return (area, None);
    }
    let text = Rect { width: area.width - width - 1, ..area };
    let map = Rect::new(area.right() - width, area.y, width, area.height);
    (text, Some(Minimap::new(map, content, view)))
}

/// The map drawn, the view starting at `offset`, each row of text read with
/// `row` - only the few each half cell samples.
pub(super) fn draw_minimap(f: &mut ratatui::Frame<'_>, map: &Minimap, offset: usize, mut row: impl FnMut(usize) -> MapRow) {
    let behind = crate::display::background().unwrap_or(palette().background);
    let shaded = blend(behind, palette().border, 0.35);
    let columns = usize::from(map.area.width);
    let mut half = |half: usize| -> Vec<Color> {
        let base = if map.in_view(half, offset) { shaded } else { behind };
        let (mut fill, mut mark, mut count) = (vec![0.0; columns], None, 0);
        for at in map.sampled(half) {
            let read = row(at);
            for (sum, band) in fill.iter_mut().zip(read.fill) {
                *sum += band;
            }
            mark = mark.or(read.mark);
            count += 1;
        }
        if let Some(mark) = mark {
            return vec![mark; columns];
        }
        fill.into_iter().map(|sum| shade(base, sum / count.max(1) as f64)).collect()
    };
    let buffer = f.buffer_mut();
    for cell in 0..map.area.height {
        let (upper, lower) = (half(usize::from(cell) * 2), half(usize::from(cell) * 2 + 1));
        for (x, (upper, lower)) in (map.area.x..).zip(upper.into_iter().zip(lower)) {
            let at = &mut buffer[(x, map.area.y + cell)];
            at.reset();
            at.set_symbol("▀").set_fg(upper).set_bg(lower);
        }
    }
}

/// `base` brightened towards the text's colour as far as `fill` goes; left
/// as it is for a band with nothing in it.
fn shade(base: Color, fill: f64) -> Color {
    if fill <= 0.0 { base } else { blend(base, palette().file, 0.25 + 0.6 * fill.min(1.0)) }
}
