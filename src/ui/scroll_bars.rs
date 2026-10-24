//! Scroll bars for the viewer and the editor, drawn into the frame's border
//! rather than beside it: the text keeps every column and row it had, and
//! clicks map to it as before. Each shows only when there is more than fits,
//! and each drawn is given back, for a click or a drag to find where it was.

use super::*;
pub(super) use crate::scroll_bar::{Extent, ScrollBar, thumb};

/// The thumbs: the border's own line drawn heavy, so each sits on the line it
/// moves along, in the title's colour to stand out from it.
pub(super) const THUMB_VERTICAL: &str = "┃";
pub(super) const THUMB_HORIZONTAL: &str = "━";

/// A bar down the right-hand border of `frame`, between its corners.
pub(super) fn vertical_bar(f: &mut ratatui::Frame<'_>, frame: Rect, extent: Extent) -> Option<ScrollBar> {
    if frame.width == 0 || frame.height < 3 {
        return None;
    }
    let track = Rect::new(frame.right() - 1, frame.y + 1, 1, frame.height - 2);
    draw(f, ScrollBar { track, vertical: true, extent })
}

/// A bar along the bottom border of `frame`, under the columns `under`
/// covers: the text, and not the line numbers beside it.
pub(super) fn horizontal_bar(f: &mut ratatui::Frame<'_>, frame: Rect, under: Rect, extent: Extent) -> Option<ScrollBar> {
    if frame.height == 0 || under.width == 0 {
        return None;
    }
    let track = Rect::new(under.x, frame.bottom() - 1, under.width, 1);
    draw(f, ScrollBar { track, vertical: false, extent })
}

/// The thumb, over the border line that is the track. None when it all fits.
fn draw(f: &mut ratatui::Frame<'_>, bar: ScrollBar) -> Option<ScrollBar> {
    let length = if bar.vertical { bar.track.height } else { bar.track.width };
    let (start, thumb_length) = thumb(length as usize, bar.extent)?;
    let symbol = if bar.vertical { THUMB_VERTICAL } else { THUMB_HORIZONTAL };
    let buffer = f.buffer_mut();
    for cell in start as u16..(start + thumb_length) as u16 {
        let (x, y) = if bar.vertical { (bar.track.x, bar.track.y + cell) } else { (bar.track.x + cell, bar.track.y) };
        buffer[(x, y)].set_symbol(symbol).set_style(style_title());
    }
    Some(bar)
}
