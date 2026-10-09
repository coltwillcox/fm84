//! A row of things to click along the top - the drives, and the tabs under
//! them - and which of them show when they do not all fit.

use ratatui::layout::Rect;

/// The columns an arrow takes at either end of a strip that does not fit,
/// there or not, so the items stay put as the arrows come and go.
pub const ARROW: usize = 2;

/// One strip as last drawn. When its items do not all fit, it shows a window
/// onto them, with an arrow at either end that has more beyond it.
#[derive(Default)]
pub struct Strip {
    pub area: Rect,
    /// The first item shown.
    pub first: usize,
    /// How many are shown, from `first`.
    pub visible: usize,
    /// The item the window last moved to show. When the one that matters -
    /// a panel's own drive or tab, or the candidate while choosing a drive -
    /// is a different one, the window follows it there; otherwise it stays
    /// where the arrows put it, so scrolling to look is not undone on the
    /// next frame.
    pub followed: Option<usize>,
    /// Where each thing on the strip is drawn, in columns from `area.x`, and
    /// what a click there does.
    pub slots: Vec<(u16, u16)>,
    pub hits: Vec<StripHit>,
    /// Each item's width, and the room there was, as of the last placing -
    /// what the arrows need to work out a page.
    widths: Vec<usize>,
    room: usize,
}

/// What a click on a strip lands on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StripHit {
    Item(usize),
    Back,
    Forward,
}

impl Strip {
    /// Whether every item fits as it is. Strictly less, for the space the
    /// strip opens with.
    pub fn fits(widths: &[usize], room: usize) -> bool {
        widths.iter().sum::<usize>() < room
    }

    /// Settle which items to show, each `widths` wide, in `room` columns,
    /// keeping `focus` in view when it has moved since last time.
    pub fn place(&mut self, widths: Vec<usize>, room: usize, focus: Option<usize>) {
        self.widths = widths;
        self.room = room;
        let count = self.widths.len();
        if Self::fits(&self.widths, room) {
            self.first = 0;
            self.visible = count;
            self.followed = focus;
            return;
        }

        self.first = self.first.min(count - 1);
        if let Some(focus) = focus.filter(|&focus| focus < count)
            && self.followed != Some(focus)
        {
            if focus < self.first {
                self.first = focus;
            }
            while self.first < focus && self.width(self.first..focus + 1) > self.space() {
                self.first += 1;
            }
            self.followed = Some(focus);
        }
        // No empty space at the end while there is more before: items going
        // away, or a page forward, can leave the window running past them.
        while self.first > 0 && self.width(self.first - 1..count) <= self.space() {
            self.first -= 1;
        }
        self.visible = self.fitting_from(self.first);
    }

    /// A page along, from the arrows or the wheel. What shows is settled on
    /// the next placing, which also pulls a page past the end back.
    pub fn scroll(&mut self, forward: bool) {
        let count = self.widths.len();
        if Self::fits(&self.widths, self.room) {
            return;
        }
        if forward {
            if self.first + self.visible < count {
                self.first += self.visible;
            }
        } else {
            // Back as far as a page takes, ending just before the first shown.
            let mut first = self.first;
            while first > 0 && self.width(first - 1..self.first) <= self.space() {
                first -= 1;
            }
            self.first = if first == self.first { first.saturating_sub(1) } else { first };
        }
        self.visible = self.fitting_from(self.first);
    }

    /// The room for items, between the arrows.
    fn space(&self) -> usize {
        self.room.saturating_sub(2 * ARROW)
    }

    fn width(&self, range: std::ops::Range<usize>) -> usize {
        self.widths[range].iter().sum()
    }

    /// How many fit from `first` on. At least one, cut short if it must be,
    /// so a strip too narrow for any still shows something to click.
    fn fitting_from(&self, first: usize) -> usize {
        let mut used = 0;
        let fitting = self.widths[first..]
            .iter()
            .take_while(|&&width| {
                used += width;
                used <= self.space()
            })
            .count();
        fitting.max(1).min(self.widths.len() - first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twelve items five wide, with room for four between the arrows.
    fn crowded() -> (Vec<usize>, usize) {
        (vec![5; 12], 4 * 5 + 2 * ARROW)
    }

    #[test]
    fn the_window_follows_the_focus_but_not_back_after_scrolling() {
        let (widths, room) = crowded();
        let mut strip = Strip::default();

        // The panel's own, the ninth, is brought in.
        strip.place(widths.clone(), room, Some(8));
        assert_eq!((strip.first, strip.visible), (5, 4));

        // Scrolled back to look, it stays there while the focus does not move.
        strip.scroll(false);
        assert_eq!(strip.first, 1);
        strip.place(widths.clone(), room, Some(8));
        assert_eq!(strip.first, 1);

        // The focus moving - a candidate while choosing - brings it back.
        strip.place(widths.clone(), room, Some(10));
        assert_eq!(strip.first, 7);
        strip.place(widths.clone(), room, Some(2));
        assert_eq!(strip.first, 2);
    }

    #[test]
    fn pages_stop_at_either_end() {
        let (widths, room) = crowded();
        let mut strip = Strip::default();
        strip.place(widths.clone(), room, Some(0));
        strip.scroll(true);
        strip.place(widths.clone(), room, Some(0));
        assert_eq!(strip.first, 4);
        strip.scroll(true);
        strip.scroll(true);
        strip.place(widths.clone(), room, Some(0));
        // The last page is a full one, not the last item alone.
        assert_eq!((strip.first, strip.visible), (8, 4));

        strip.scroll(false);
        assert_eq!(strip.first, 4);
        strip.scroll(false);
        strip.scroll(false);
        assert_eq!(strip.first, 0);
    }

    #[test]
    fn items_of_different_widths_page_by_what_fits() {
        // Room for 20 between the arrows.
        let widths = vec![8, 8, 8, 8, 15, 3, 3];
        let room = 20 + 2 * ARROW;
        let mut strip = Strip::default();
        strip.place(widths.clone(), room, Some(0));
        assert_eq!((strip.first, strip.visible), (0, 2));

        // The wide fifth is brought in, and as many after it as fit.
        strip.place(widths.clone(), room, Some(4));
        assert_eq!((strip.first, strip.visible), (4, 2));
        strip.scroll(false);
        assert_eq!((strip.first, strip.visible), (2, 2));
    }

    #[test]
    fn going_away_and_fitting_again() {
        let (widths, room) = crowded();
        let mut strip = Strip::default();
        strip.place(widths, room, Some(11));
        assert_eq!(strip.first, 8);

        // Fewer of them pulls the window back within the end.
        strip.place(vec![5; 6], room, Some(2));
        assert_eq!(strip.first, 2);
        // And with room for all, all show.
        strip.place(vec![5; 3], room, Some(0));
        assert_eq!((strip.first, strip.visible), (0, 3));
    }

    #[test]
    fn too_narrow_for_any_still_shows_one() {
        let mut strip = Strip::default();
        strip.place(vec![30, 30], 10, Some(1));
        assert_eq!((strip.first, strip.visible), (1, 1));
    }
}
