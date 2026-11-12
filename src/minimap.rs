//! F11's minimap: the whole of a text drawn small beside it, two or four
//! columns of half blocks, each half a cell a run of rows. Here is what each half
//! stands for and what a click on it comes to; the UI paints it.

use ratatui::layout::{Position, Rect};
use std::ops::Range;

/// The columns of text the map's columns stand for between them, shared
/// out: with two, the first forty of a line in the left and the next forty
/// in the right; with four, twenty each.
pub const SPAN: usize = 80;
/// The most rows a half cell looks at to tell how full its run is - a
/// long log squeezed into a few cells would otherwise be read whole every
/// frame. Those it looks at are spread through the run.
pub const SAMPLES: usize = 8;

/// A map as last drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Minimap {
    pub area: Rect,
    /// Rows of text each half cell stands for.
    pub per: usize,
    /// How many rows there are, and how many the view shows.
    pub content: usize,
    pub view: usize,
}

impl Minimap {
    /// A map of `content` rows, `view` of them on show, in `area`: a row of
    /// text to a half cell when it fits, and as many as it takes when not.
    pub fn new(area: Rect, content: usize, view: usize) -> Minimap {
        let halves = usize::from(area.height) * 2;
        Minimap {
            area,
            per: content.div_ceil(halves.max(1)).max(1),
            content,
            view,
        }
    }

    /// The rows the `half`th half cell from the top stands for; empty past
    /// the end of the text.
    pub fn rows(&self, half: usize) -> Range<usize> {
        let from = (half * self.per).min(self.content);
        from..(from + self.per).min(self.content)
    }

    /// Up to `SAMPLES` of a half cell's rows, spread through them.
    pub fn sampled(&self, half: usize) -> impl Iterator<Item = usize> {
        let rows = self.rows(half);
        let step = rows.len().div_ceil(SAMPLES).max(1);
        rows.step_by(step)
    }

    /// Whether the half cell shows any of the rows from `offset` on that
    /// the view has on screen.
    pub fn in_view(&self, half: usize, offset: usize) -> bool {
        let rows = self.rows(half);
        !rows.is_empty() && rows.start < offset + self.view && offset < rows.end
    }

    /// Where the view starts for a click or a drag at `row` on screen: the
    /// rows there in its middle, as far as the text allows. Held to the
    /// map, so a drag carries on past its ends; and its first and last
    /// cells go all the way to the start and the end, which their middles
    /// might not reach.
    pub fn offset_at(&self, row: u16) -> usize {
        let furthest = self.content.saturating_sub(self.view);
        let cell = usize::from(row.clamp(self.area.top(), self.area.bottom().saturating_sub(1)) - self.area.top());
        let last = self.content.div_ceil(self.per).saturating_sub(1) / 2;
        if cell == 0 {
            return 0;
        }
        if cell >= last {
            return furthest;
        }
        let middle = self.rows(cell * 2 + 1).start.min(self.content.saturating_sub(1));
        middle.saturating_sub(self.view / 2).min(furthest)
    }

    pub fn contains(&self, column: u16, row: u16) -> bool {
        self.area.contains(Position::new(column, row))
    }
}

/// How full each of `bands` bands across `text` is, from 0 to 1: what is
/// not space, against the band's width. A tab counts as the spaces it is.
pub fn fullness(text: &str, bands: usize) -> Vec<f64> {
    let bands = bands.max(1);
    let band = SPAN / bands;
    let mut filled = vec![0usize; bands];
    let mut column = 0;
    for character in text.chars() {
        if column >= band * bands {
            break;
        }
        match character {
            '\t' => column += 4 - column % 4,
            ' ' => column += 1,
            _ => {
                filled[column / band] += 1;
                column += 1;
            }
        }
    }
    filled.into_iter().map(|count| count as f64 / band as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_text_takes_a_half_cell_a_row() {
        let map = Minimap::new(Rect::new(0, 0, 2, 10), 15, 5);
        assert_eq!(map.per, 1);
        assert_eq!(map.rows(0), 0..1);
        assert_eq!(map.rows(14), 14..15);
        assert!(map.rows(15).is_empty(), "past the end of the text");
    }

    #[test]
    fn a_long_one_shares_them_out() {
        let map = Minimap::new(Rect::new(0, 0, 2, 10), 1000, 30);
        assert_eq!(map.per, 50);
        assert_eq!(map.rows(19), 950..1000);
        assert!(map.sampled(0).count() <= SAMPLES, "spread through, not all fifty");
        assert!(map.sampled(0).all(|row| row < 50));
    }

    #[test]
    fn the_view_is_found_on_the_map() {
        let map = Minimap::new(Rect::new(0, 0, 2, 10), 100, 10);
        assert_eq!(map.per, 5);
        let shown: Vec<usize> = (0..20).filter(|&half| map.in_view(half, 22)).collect();
        assert_eq!(shown, vec![4, 5, 6], "rows 22 to 31");
    }

    #[test]
    fn a_click_puts_the_rows_there_in_the_middle() {
        let map = Minimap::new(Rect::new(5, 3, 2, 10), 100, 10);
        assert_eq!(map.offset_at(3), 0, "the top");
        assert_eq!(map.offset_at(8), 50, "the middle: row 55 in the middle of the view");
        assert_eq!(map.offset_at(12), 90, "the bottom, as far as it goes");
        assert_eq!(map.offset_at(40), 90, "dragged past the end");
        assert_eq!(map.offset_at(0), 0, "and past the top");
        // A cell with sixteen rows: the top one is the very top, not its middle.
        let coarse = Minimap::new(Rect::new(0, 5, 2, 14), 201, 14);
        assert_eq!(coarse.offset_at(5), 0);
        assert_eq!(coarse.offset_at(18), 187);
        // A short text ends part of the way down: its last cell is the end.
        let short = Minimap::new(Rect::new(0, 0, 2, 10), 12, 5);
        assert_eq!(short.offset_at(5), 7);
        // All of it shown: nowhere to go.
        assert_eq!(Minimap::new(Rect::new(0, 0, 2, 10), 5, 10).offset_at(4), 0);
    }

    #[test]
    fn fullness_is_counted_by_band() {
        assert_eq!(fullness("", 2), [0.0, 0.0]);
        assert_eq!(fullness(&"x".repeat(60), 2), [1.0, 0.5]);
        assert_eq!(fullness("    ", 2), [0.0, 0.0], "spaces are not text");
        assert_eq!(fullness("\tx", 2)[0], 1.0 / 40.0);
        assert_eq!(fullness(&"x".repeat(500), 2), [1.0, 1.0], "past both bands, left off");
        assert_eq!(fullness(&"x".repeat(50), 4), [1.0, 1.0, 0.5, 0.0], "four of twenty each");
    }
}
