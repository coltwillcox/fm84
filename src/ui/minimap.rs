//! F11's minimap, drawn beside the viewer's, the editor's or the diff's
//! text: each half cell as dark or as bright as its rows are full, the
//! matches of a search and the differences between two files lit on it,
//! and the rows on screen shaded behind.

use super::*;
use crate::minimap::{Minimap, fullness};
use ratatui::style::Color;

/// Columns the map takes from the text: a gap, then its two.
const MAP_WIDTH: u16 = 3;
/// The narrowest the text is left beside a map; narrower, there is none.
const LEAST_TEXT: u16 = 30;

/// What one row of text shows on the map: how full each band of it is, and
/// a colour to light it in instead - a match, or a difference.
pub(super) struct MapRow {
    pub fill: [f64; 2],
    pub mark: Option<Color>,
}

impl MapRow {
    /// A row of text as it reads, lit when `marked`.
    pub(super) fn of(text: &str, marked: bool) -> MapRow {
        MapRow {
            fill: fullness(text),
            mark: marked.then(|| palette().title),
        }
    }
}

/// How full a line is across both bands, from 0 to 1: one column of the
/// map for each file, beside each other, as the diff has them.
pub(super) fn fullness_of(text: &str) -> f64 {
    let [left, right] = fullness(text);
    (left + right) / 2.0
}

/// The text's `area` with room taken off its right for a map, when F11 has
/// one, there is more than the view shows, and the text is left room
/// enough: the text's area, and the map's.
pub(super) fn make_room(area: Rect, content: usize, view: usize, options: &Options) -> (Rect, Option<Minimap>) {
    if !options.minimap || content <= view || area.width < LEAST_TEXT + MAP_WIDTH || area.height == 0 {
        return (area, None);
    }
    let text = Rect { width: area.width - MAP_WIDTH, ..area };
    let map = Rect::new(area.right() - 2, area.y, 2, area.height);
    (text, Some(Minimap::new(map, content, view)))
}

/// The map drawn, the view starting at `offset`, each row of text read with
/// `row` - only the few each half cell samples.
pub(super) fn draw_minimap(f: &mut ratatui::Frame<'_>, map: &Minimap, offset: usize, mut row: impl FnMut(usize) -> MapRow) {
    let behind = crate::display::background().unwrap_or(palette().background);
    let shaded = blend(behind, palette().border, 0.35);
    let mut half = |half: usize| -> [Color; 2] {
        let base = if map.in_view(half, offset) { shaded } else { behind };
        let (mut fill, mut mark, mut count) = ([0.0; 2], None, 0);
        for at in map.sampled(half) {
            let MapRow { fill: [left, right], mark: lit } = row(at);
            fill[0] += left;
            fill[1] += right;
            mark = mark.or(lit);
            count += 1;
        }
        if let Some(mark) = mark {
            return [mark; 2];
        }
        fill.map(|sum| shade(base, sum / count.max(1) as f64))
    };
    let buffer = f.buffer_mut();
    for cell in 0..map.area.height {
        let ([upper_left, upper_right], [lower_left, lower_right]) = (half(usize::from(cell) * 2), half(usize::from(cell) * 2 + 1));
        for (x, upper, lower) in [(map.area.x, upper_left, lower_left), (map.area.x + 1, upper_right, lower_right)] {
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
