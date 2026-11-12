//! F11: the options, grouped under their headings.

use super::*;

/// F11, as wide as every popup and no taller than the panels it sits over:
/// it scrolls instead, keeping the top and bottom bars in view. `first` is
/// the line of the list it last started at, and is kept to where it starts
/// now. Gives back how many lines of the list it had room for.
pub(super) fn render_options_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, editing: Option<&TextInput>, first: &mut usize) -> usize {
    let popup_width = (area.width * POPUP_WIDTH / 100).max(1) as usize;
    let inner_width = popup_width.saturating_sub(4);
    let cursor_style = Style::new().fg(palette().selected_foreground).bg(palette().selected_background).add_modifier(Modifier::BOLD);

    // Every line of the list, headings among the rows, and which one the
    // cursor is on - the scrolling below works in lines, not rows.
    let mut list: Vec<Line> = Vec::with_capacity(OPTION_ROWS.len() * 2);
    let (mut cursor_line, mut cursor_top) = (0, 0);
    for (index, &row) in OPTION_ROWS.iter().enumerate() {
        if let Some(section) = row.section() {
            if !list.is_empty() {
                list.push(Line::from(""));
            }
            list.push(Line::from(Span::styled(format!(" {}", section), style_title().add_modifier(Modifier::BOLD))));
        }
        if index == app_state.options_cursor {
            cursor_line = list.len();
            // The first row of a group brings its heading into view with it.
            cursor_top = if row.section().is_some() { cursor_line - 1 } else { cursor_line };
        }
        list.push(option_line(app_state, row, index == app_state.options_cursor, inner_width, cursor_style, editing));
    }

    let hint = if editing.is_none() && OPTION_ROWS[app_state.options_cursor] == crate::options::OptionRow::ColumnInfo {
        t!("↑↓ - Move    Enter/←→ - Change    E - Edit the commands    Esc - Close")
    } else if editing.is_none() && OPTION_ROWS[app_state.options_cursor] == crate::options::OptionRow::Theme {
        t!("↑↓ - Move    Enter/←→ - Change    E - Edit the file colours    Esc - Close")
    } else if editing.is_none() {
        t!("↑↓ - Move    Enter/←→ - Change    Esc - Close")
    } else if OPTION_ROWS[app_state.options_cursor] == crate::options::OptionRow::Editor {
        t!("Enter - Save    Esc - Cancel    {} - file")
    } else {
        t!("Enter - Save    Esc - Cancel    {} - directory")
    };
    // The keys, broken onto as many rows as the popup's width needs - as many
    // as the longest of them takes, whichever row the cursor is on, so the
    // list does not jump as it goes by the rows with keys of their own.
    let mut hints = hint_rows(&[hint], inner_width);
    let hint_height = [
        t!("↑↓ - Move    Enter/←→ - Change    E - Edit the commands    Esc - Close"),
        t!("↑↓ - Move    Enter/←→ - Change    E - Edit the file colours    Esc - Close"),
    ]
    .iter()
    .map(|hint| hint_rows(&[hint], inner_width).len())
    .max()
    .unwrap_or(1)
    .max(hints.len());
    hints.resize(hint_height, Line::from(""));

    // A border row each side, a blank row above and below the list, and the
    // keys under it.
    let popup_area = centered(screen_layout(area)[2], popup_width, list.len() + 5 + hint_height);
    let mut popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Options")), style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .style(style_border());

    // On a short terminal the list scrolls, keeping the cursor row in view.
    // The blank rows and the keys around it take the rest.
    let inner = popup_inner(popup_area);
    let room = (inner.height as usize).saturating_sub(2 + hint_height).max(1);
    *first = scroll_to_show(*first, cursor_top, cursor_line, room, list.len());
    let first = *first;
    if room < list.len() {
        // Where the cursor is in the whole list, since some of it is hidden.
        let position = format!(" {}/{} ", app_state.options_cursor + 1, OPTION_ROWS.len());
        popup_block = popup_block.title_bottom(Line::from(Span::styled(position, style_columns())).right_aligned());
    }

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let mut lines = Vec::with_capacity(room + 2 + hint_height);
    lines.push(Line::from(""));
    lines.extend(list.into_iter().skip(first).take(room));

    lines.push(Line::from(""));
    lines.extend(hints);

    f.render_widget(Paragraph::new(lines), inner);
    room
}

/// Where a list of `total` lines, `room` of them on show, starts so that
/// lines `top` to `bottom` are on show: where it started, `first`, unless
/// that leaves them above or below the room - then just as far as brings
/// them in. The cursor moves through the lines on show, and the list only
/// when the cursor would go past either end of them.
fn scroll_to_show(first: usize, top: usize, bottom: usize, room: usize, total: usize) -> usize {
    let furthest = total.saturating_sub(room);
    let first = first.min(furthest);
    if top < first {
        top
    } else if bottom >= first + room {
        (bottom + 1).saturating_sub(room)
    } else {
        first
    }
}

/// One row of the options list, padded out to the full width so the cursor
/// reads as a bar, as it does in the panels.
fn option_line(app_state: &AppState, row: crate::options::OptionRow, is_cursor: bool, row_width: usize, cursor_style: Style, editing: Option<&TextInput>) -> Line<'static> {
    let label = format!("   {}", row.label());
    if is_cursor && let Some(input) = editing {
        let typed = input.cursor_spans(style_input(), style_input_cursor());
        let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
        let gap = row_width.saturating_sub(display_width(&label) + typed_width + 1);
        let mut spans = vec![Span::styled(label, cursor_style), Span::styled(" ".repeat(gap), cursor_style)];
        spans.extend(typed);
        spans.push(Span::styled(" ", cursor_style));
        return Line::from(spans);
    }

    let value = app_state.options.value(row);
    // A long command gives way from the left, keeping the end of it - usually
    // where the arguments that matter are.
    let room = row_width.saturating_sub(display_width(&label) + 3);
    let value = if display_width(&value) > room {
        let tail: String = value.chars().rev().take(room.saturating_sub(1)).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{}", tail)
    } else {
        value
    };
    let gap = row_width.saturating_sub(display_width(&label) + display_width(&value) + 1);
    if is_cursor {
        Line::from(Span::styled(format!("{}{}{} ", label, " ".repeat(gap), value), cursor_style))
    } else {
        Line::from(vec![Span::styled(format!("{}{}", label, " ".repeat(gap)), style_dir()), Span::styled(format!("{} ", value), style_columns())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_scrolls_only_when_the_cursor_would_leave_it() {
        // Ten on show of fifty, at the end.
        assert_eq!(scroll_to_show(40, 49, 49, 10, 50), 40);
        // Up from the end: the cursor moves, the list stays.
        assert_eq!(scroll_to_show(40, 45, 45, 10, 50), 40);
        assert_eq!(scroll_to_show(40, 40, 40, 10, 50), 40);
        // Past the top line: a line at a time.
        assert_eq!(scroll_to_show(40, 39, 39, 10, 50), 39);
        // Down past the bottom: as far as brings it in.
        assert_eq!(scroll_to_show(0, 10, 10, 10, 50), 1);
        // A heading above the cursor's row comes in with it.
        assert_eq!(scroll_to_show(20, 18, 19, 10, 50), 18);
        // A screen grown since: no further than the list allows.
        assert_eq!(scroll_to_show(45, 47, 47, 20, 50), 30);
        // All of it fits.
        assert_eq!(scroll_to_show(3, 5, 5, 60, 50), 0);
    }
}
