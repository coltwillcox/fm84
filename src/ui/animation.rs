//! F11's Animation, drawn over what a frame drew without it: the cursor's
//! bar on its way to its row, a directory's rows coming up one after
//! another with a beam along the newest, the rows a job made glowing, and
//! a popup wiped open from its middle. The timing is the app's; this only
//! paints.

use super::*;
use crate::app::{Motion, PopupKind, ease_out};
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use std::time::Instant;

/// What a side's table is to be drawn with, worked out before it is drawn.
pub(super) struct PanelMotion {
    /// The first entry on show, and how many are.
    offset: usize,
    entries: usize,
    /// How many of its rows are up, counted from the top.
    shown: usize,
    /// The row the cursor's bar is drawn on while it slides there, the
    /// table drawn without one of its own; None with it where it is.
    bar: Option<u16>,
}

impl PanelMotion {
    /// Whether the table draws its cursor itself this frame.
    pub(super) fn table_cursor(&self) -> bool {
        self.bar.is_none()
    }
}

/// How `side`'s table moves this frame, its `entries` rows on show from
/// `offset` on.
pub(super) fn panel_motion(app_state: &AppState, side: Side, offset: usize, entries: usize, motion: &mut Motion, now: Instant) -> PanelMotion {
    let still = PanelMotion { offset, entries, shown: entries, bar: None };
    if !app_state.options.animation {
        return still;
    }
    let panel = app_state.panel(side);
    let shown = motion.booted_rows(side, &panel.dir, entries, now);
    let renaming = app_state.active_side == side && matches!(app_state.dialog, Some(Dialog::Rename(_)));
    // The tree draws a cursor of its own, and the rename field sits where
    // the cursor is: neither slides, and the next move starts afresh.
    let Some(selected) = panel.state.selected().filter(|_| panel.tree.is_none() && !renaming && entries > 0) else {
        motion.forget_cursor(side);
        return PanelMotion { offset, entries, shown, bar: None };
    };
    let to = selected.saturating_sub(offset).min(entries - 1) as u16;
    let row = motion.cursor_row(side, to, now).round() as u16;
    PanelMotion {
        offset,
        entries,
        shown,
        bar: (row != to).then_some(row.min(entries as u16 - 1)),
    }
}

/// The animations over `side`'s table, just drawn in `inner` - its heading
/// first, then the rows `moving` was worked out for - and `cursor` the style
/// its cursor is drawn in.
pub(super) fn animate_panel(f: &mut ratatui::Frame<'_>, inner: Rect, app_state: &AppState, side: Side, moving: &PanelMotion, cursor: Style, now: Instant) {
    if !app_state.options.animation {
        return;
    }
    let (offset, entries) = (moving.offset, moving.entries);
    let row_area = |row: usize| Rect::new(inner.x, inner.y + 1 + row as u16, inner.width, 1).intersection(inner);
    let buffer = f.buffer_mut();
    if let Some(bar) = moving.bar {
        buffer.set_style(row_area(usize::from(bar)), cursor);
    }

    // What the last copy or move made, glowing and fading.
    if let Some(flash) = &app_state.flash
        && let Some(glow) = flash.glow(now)
    {
        let panel = app_state.panel(side);
        for (row, child) in panel.children.iter().skip(offset).take(entries).enumerate() {
            if !child.is_parent() && flash.paths.contains(&child.path_in(&panel.dir)) {
                tint(buffer, row_area(row), palette().title, glow * 0.7, glow > 0.5);
            }
        }
    }

    // Still coming up: the rows not yet up blank, a beam along the newest.
    if moving.shown < entries {
        for row in moving.shown..entries {
            blank(buffer, row_area(row));
        }
        let beam = row_area(moving.shown.saturating_sub(1));
        tint(buffer, beam, palette().title, 0.35, true);
        for x in beam.left()..beam.right() {
            buffer[(x, beam.y)].set_fg(palette().title);
        }
    }
}

/// The popup drawn over `before`, the frame as it was under it, opened as
/// far as it has got: rows hidden from the middle out, and a beam along
/// the edges of what is open. The popup is whatever the drawing of it
/// changed, so every popup opens so, whatever its shape.
pub(super) fn wipe_popup(f: &mut ratatui::Frame<'_>, before: &Buffer, app_state: &AppState, motion: &mut Motion, now: Instant) {
    let buffer = f.buffer_mut();
    let area = changed(before, buffer);
    let kind = area.and(popup_kind(app_state));
    let (Some(area), Some(open)) = (area, motion.popup_open(kind, now)) else {
        return;
    };
    if open >= 1.0 {
        return;
    }
    let shown = ((f64::from(area.height) * ease_out(open)).ceil() as u16).clamp(1, area.height);
    let top = area.y + (area.height - shown) / 2;
    let bottom = top + shown;
    for y in (area.top()..area.bottom()).filter(|&y| y < top || y >= bottom) {
        for x in area.left()..area.right() {
            buffer[(x, y)] = before[(x, y)].clone();
        }
    }
    for y in [top, bottom - 1] {
        for x in area.left()..area.right() {
            buffer[(x, y)].set_symbol("━").set_fg(palette().title).set_bg(base_background()).modifier = Modifier::BOLD;
        }
    }
}

/// Which popup is on show, for one new to wipe open and the same one drawn
/// again not to.
fn popup_kind(app_state: &AppState) -> Option<PopupKind> {
    if app_state.workers.job.is_some() {
        Some(PopupKind::Job)
    } else if app_state.error.is_some() {
        Some(PopupKind::Error)
    } else {
        app_state.dialog.as_ref().filter(|dialog| dialog.is_popup()).map(|dialog| PopupKind::Dialog(std::mem::discriminant(dialog)))
    }
}

/// The smallest area holding every cell `after` has that `before` had not.
fn changed(before: &Buffer, after: &Buffer) -> Option<Rect> {
    if before.area != after.area {
        return None;
    }
    let area = after.area;
    let (mut left, mut top, mut right, mut bottom) = (u16::MAX, u16::MAX, 0, 0);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if before[(x, y)] != after[(x, y)] {
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x + 1), bottom.max(y + 1));
            }
        }
    }
    (left < right).then(|| Rect::new(left, top, right - left, bottom - top))
}

/// An area put back to nothing drawn on it.
fn blank(buffer: &mut Buffer, area: Rect) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            cell.reset();
            if let Some(background) = crate::display::background() {
                cell.set_bg(background);
            }
        }
    }
}

/// An area's background taken `amount` of the way to `colour`, and made
/// bold as well when `bold`.
fn tint(buffer: &mut Buffer, area: Rect, colour: Color, amount: f64, bold: bool) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buffer[(x, y)];
            let under = if matches!(cell.bg, Color::Reset) { base_background() } else { cell.bg };
            cell.set_bg(blend(under, colour, amount));
            if bold {
                cell.modifier.insert(Modifier::BOLD);
            }
        }
    }
}

/// What is behind everything: the theme's painted background, or its own
/// where the terminal's shows through.
fn base_background() -> Color {
    crate::display::background().unwrap_or(palette().background)
}

/// `from` taken `amount` of the way to `to`, from 0 to 1. Colours that are
/// not red, green and blue - a terminal's own - go over half way.
pub(super) fn blend(from: Color, to: Color, amount: f64) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount).round() as u8;
            Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
        }
        _ if amount > 0.5 => to,
        _ => from,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_blend_from_one_to_the_other() {
        let (black, white) = (Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255));
        assert_eq!(blend(black, white, 0.0), black);
        assert_eq!(blend(black, white, 1.0), white);
        assert_eq!(blend(black, white, 0.5), Color::Rgb(128, 128, 128));
        assert_eq!(blend(Color::Reset, white, 0.2), Color::Reset);
        assert_eq!(blend(Color::Reset, white, 0.8), white);
    }

    #[test]
    fn the_change_a_popup_made_is_found() {
        let area = Rect::new(0, 0, 20, 10);
        let before = Buffer::empty(area);
        let mut after = before.clone();
        assert_eq!(changed(&before, &after), None);
        after[(3, 2)].set_symbol("x");
        after[(8, 6)].set_symbol("y");
        assert_eq!(changed(&before, &after), Some(Rect::new(3, 2, 6, 5)));
    }
}
