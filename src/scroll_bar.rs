//! A scroll bar as the viewer and the editor draw one in their border, and
//! what a click or a drag on it comes to: where the thumb sits for an
//! offset, and the offset for where the thumb is dragged to.

use ratatui::layout::{Position, Rect};

/// What a bar's thumb stands for: the whole of what can be scrolled through,
/// how much of it shows at once, and where the view starts in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub content: usize,
    pub view: usize,
    pub offset: usize,
}

impl Extent {
    /// The furthest the view can start.
    fn furthest(&self) -> usize {
        self.content.saturating_sub(self.view)
    }
}

/// Where the thumb sits along a track `track` cells long, as its first cell
/// and how many it takes. None when it all fits, and there is nothing to
/// scroll. The thumb is as long as the view is against the whole, never less
/// than a cell, and reaches either end of the track exactly when the view
/// reaches that end of the content.
pub fn thumb(track: usize, extent: Extent) -> Option<(usize, usize)> {
    let Extent { content, view, offset } = extent;
    if track == 0 || view == 0 || content <= view {
        return None;
    }
    let length = (track * view).div_ceil(content).clamp(1, track);
    let furthest = extent.furthest();
    let room = track - length;
    // Rounded to the nearest cell, so only an offset at the very end puts the
    // thumb at the end of the track, and only one at the start at its start.
    let start = match offset.min(furthest) {
        0 => 0,
        at if at == furthest => room,
        at => {
            let start = (room * at + furthest / 2) / furthest;
            // Off both ends of the track while the view is off both ends of
            // the content, where the track has the room for it.
            if room >= 2 { start.clamp(1, room - 1) } else { start }
        }
    };
    Some((start, length))
}

/// One bar as last drawn: the cells of its track, one thick, and what it
/// stood for then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollBar {
    pub track: Rect,
    /// Down the side, scrolling lines; otherwise along the bottom, scrolling
    /// columns.
    pub vertical: bool,
    pub extent: Extent,
}

/// What a press on a bar does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    /// On the thumb, this many cells from its start: it is taken hold of
    /// there, and follows the pointer from then on.
    Thumb(usize),
    /// On the track before the thumb, or after it: a page that way.
    Page { forward: bool },
}

impl ScrollBar {
    fn length(&self) -> usize {
        usize::from(if self.vertical { self.track.height } else { self.track.width })
    }

    /// How far along the track the pointer is, in cells, and not held to
    /// the track: a drag carries on when the pointer strays off it.
    fn along(&self, column: u16, row: u16) -> isize {
        if self.vertical { row as isize - self.track.y as isize } else { column as isize - self.track.x as isize }
    }

    /// What a press at this cell does, or None when it is not on the bar.
    pub fn press(&self, column: u16, row: u16) -> Option<BarPress> {
        if !self.track.contains(Position::new(column, row)) {
            return None;
        }
        let (start, length) = thumb(self.length(), self.extent)?;
        let at = self.along(column, row) as usize;
        Some(if at < start {
            BarPress::Page { forward: false }
        } else if at >= start + length {
            BarPress::Page { forward: true }
        } else {
            BarPress::Thumb(at - start)
        })
    }

    /// The offset that puts the thumb under the pointer, held where it was
    /// taken hold of: `grab` cells from its start.
    pub fn drag_to(&self, column: u16, row: u16, grab: usize) -> usize {
        let Some((_, length)) = thumb(self.length(), self.extent) else {
            return 0;
        };
        let room = self.length() - length;
        if room == 0 {
            return 0;
        }
        let start = (self.along(column, row) - grab as isize).clamp(0, room as isize) as usize;
        let furthest = self.extent.furthest();
        (start * furthest + room / 2) / room
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(content: usize, view: usize, offset: usize) -> Option<(usize, usize)> {
        thumb(10, Extent { content, view, offset })
    }

    #[test]
    fn nothing_to_scroll_is_no_bar() {
        assert_eq!(at(10, 10, 0), None);
        assert_eq!(at(3, 10, 0), None);
        assert_eq!(thumb(0, Extent { content: 100, view: 10, offset: 0 }), None);
    }

    #[test]
    fn the_thumb_is_the_view_against_the_whole() {
        // Half of it shows: half the track.
        assert_eq!(at(20, 10, 0), Some((0, 5)));
        assert_eq!(at(20, 10, 10), Some((5, 5)));
        // A sliver of a long file is still a cell to see.
        assert_eq!(at(100_000, 10, 0), Some((0, 1)));
    }

    #[test]
    fn only_the_ends_of_the_content_reach_the_ends_of_the_track() {
        let (content, view) = (1000, 10);
        assert_eq!(at(content, view, 0).unwrap().0, 0);
        assert_eq!(at(content, view, 1).unwrap().0, 1, "a line down is off the top");
        assert_eq!(at(content, view, content - view - 1).unwrap().0, 8, "a line short of the end is off the bottom");
        assert_eq!(at(content, view, content - view).unwrap().0, 9);
        // Past the end, as the editor's horizontal scroll can be, is the end.
        assert_eq!(at(content, view, content * 2).unwrap().0, 9);
    }

    /// Ten rows of track at rows 5 to 14, for 100 lines with 20 showing: a
    /// thumb two cells long.
    fn bar(offset: usize) -> ScrollBar {
        ScrollBar {
            track: Rect::new(30, 5, 1, 10),
            vertical: true,
            extent: Extent { content: 100, view: 20, offset },
        }
    }

    #[test]
    fn a_press_takes_the_thumb_or_pages_towards_itself() {
        let bar = bar(40);
        let (start, length) = thumb(10, bar.extent).unwrap();
        assert_eq!((start, length), (4, 2));
        assert_eq!(bar.press(30, 5 + 4), Some(BarPress::Thumb(0)));
        assert_eq!(bar.press(30, 5 + 5), Some(BarPress::Thumb(1)));
        assert_eq!(bar.press(30, 5), Some(BarPress::Page { forward: false }));
        assert_eq!(bar.press(30, 14), Some(BarPress::Page { forward: true }));
        // Beside it, above it and below it, it is not the bar.
        assert_eq!(bar.press(29, 9), None);
        assert_eq!(bar.press(30, 4), None);
        assert_eq!(bar.press(30, 15), None);
    }

    #[test]
    fn a_dragged_thumb_follows_the_pointer_and_stops_at_the_ends() {
        let bar = bar(0);
        // Taken by its second cell, at row 6, and dragged.
        assert_eq!(bar.drag_to(30, 6, 1), 0);
        assert_eq!(bar.drag_to(30, 10, 1), 40);
        // As far as it goes, and past the end of the track, is the end.
        assert_eq!(bar.drag_to(30, 14, 1), 80);
        assert_eq!(bar.drag_to(30, 40, 1), 80);
        // Off the top, and off to the side, it still follows the row.
        assert_eq!(bar.drag_to(30, 0, 1), 0);
        assert_eq!(bar.drag_to(70, 10, 1), 40);
        // Where a drag puts it, the thumb is drawn under the pointer.
        let moved = ScrollBar {
            extent: Extent { offset: bar.drag_to(30, 10, 1), ..bar.extent },
            ..bar
        };
        assert_eq!(thumb(10, moved.extent).unwrap().0 + 1, 10 - 5);
    }
}
