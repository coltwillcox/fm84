//! Scroll bars for the viewer and the editor, drawn into the frame's border
//! rather than beside it: the text keeps every column and row it had, and
//! clicks map to it as before. Each shows only when there is more than fits,
//! and each drawn is given back, for a click or a drag to find where it was.

use super::*;
pub(super) use crate::scroll_bar::{Extent, ScrollBar, thumb};

/// A bar down the right-hand border of `frame`, between its corners.
pub(super) fn vertical_bar(f: &mut ratatui::Frame<'_>, frame: Rect, extent: Extent, style: ScrollBarStyle) -> Option<ScrollBar> {
    if frame.width == 0 || frame.height < 3 {
        return None;
    }
    let track = Rect::new(frame.right() - 1, frame.y + 1, 1, frame.height - 2);
    draw(f, ScrollBar { track, vertical: true, extent }, style)
}

/// A bar along the bottom border of `frame`, under the columns `under`
/// covers: the text, and not the line numbers beside it.
pub(super) fn horizontal_bar(f: &mut ratatui::Frame<'_>, frame: Rect, under: Rect, extent: Extent, style: ScrollBarStyle) -> Option<ScrollBar> {
    if frame.height == 0 || under.width == 0 {
        return None;
    }
    let track = Rect::new(under.x, frame.bottom() - 1, under.width, 1);
    draw(f, ScrollBar { track, vertical: false, extent }, style)
}

/// The thumb, over the border line that is the track, drawn as F11 says and
/// in the title's colour to stand out from the border. None when it all fits.
fn draw(f: &mut ratatui::Frame<'_>, bar: ScrollBar, style: ScrollBarStyle) -> Option<ScrollBar> {
    let length = if bar.vertical { bar.track.height } else { bar.track.width };
    let (start, thumb_length) = thumb(length as usize, bar.extent)?;
    let (down, along) = style.thumbs();
    let symbol = if bar.vertical { down } else { along };
    let buffer = f.buffer_mut();
    for cell in start as u16..(start + thumb_length) as u16 {
        let (x, y) = if bar.vertical { (bar.track.x, bar.track.y + cell) } else { (bar.track.x + cell, bar.track.y) };
        buffer[(x, y)].set_symbol(symbol).set_style(style_title());
    }
    Some(bar)
}
