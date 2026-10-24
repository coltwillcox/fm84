//! F11: the options, grouped under their headings.

use super::*;

/// F11, as wide as every popup and no taller than the panels it sits over:
/// it scrolls instead, keeping the top and bottom bars in view.
pub(super) fn render_options_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, editing: Option<&TextInput>) {
    let popup_width = (area.width * POPUP_WIDTH / 100).max(1) as usize;
    let inner_width = popup_width.saturating_sub(4);
    let cursor_style = Style::new().fg(palette().selected_foreground).bg(palette().selected_background).add_modifier(Modifier::BOLD);

    // Every line of the list, headings among the rows, and which one the
    // cursor is on - the scrolling below works in lines, not rows.
    let mut list: Vec<Line> = Vec::with_capacity(OPTION_ROWS.len() * 2);
    let mut cursor_line = 0;
    for (index, &row) in OPTION_ROWS.iter().enumerate() {
        if let Some(section) = row.section() {
            if !list.is_empty() {
                list.push(Line::from(""));
            }
            list.push(Line::from(Span::styled(format!(" {}", section), style_title().add_modifier(Modifier::BOLD))));
        }
        if index == app_state.options_cursor {
            cursor_line = list.len();
        }
        list.push(option_line(app_state, row, index == app_state.options_cursor, inner_width, cursor_style, editing));
    }

    // A border row each side, a blank row above and below the list, and two
    // for the key hint under it.
    let popup_area = centered(screen_layout(area)[2], popup_width, list.len() + 6);
    let mut popup_block = Block::default().title(Line::from(Span::styled(" Options ", style_title())).centered()).borders(Borders::ALL).style(style_border());

    // On a short terminal the list scrolls, keeping the cursor row in view.
    // The blank rows and the hint around it take three.
    let inner = popup_inner(popup_area);
    let room = (inner.height as usize).saturating_sub(3).max(1);
    let first = (cursor_line + 1).saturating_sub(room);
    if room < list.len() {
        // Where the cursor is in the whole list, since some of it is hidden.
        let position = format!(" {}/{} ", app_state.options_cursor + 1, OPTION_ROWS.len());
        popup_block = popup_block.title_bottom(Line::from(Span::styled(position, style_columns())).right_aligned());
    }

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let mut lines = Vec::with_capacity(room + 3);
    lines.push(Line::from(""));
    lines.extend(list.into_iter().skip(first).take(room));

    let hint = if editing.is_none() {
        "↑↓ - Move    Enter/←→ - Change    Esc - Close"
    } else if OPTION_ROWS[app_state.options_cursor] == crate::options::OptionRow::Editor {
        "Enter - Save    Esc - Cancel    {} - file"
    } else {
        "Enter - Save    Esc - Cancel    {} - directory"
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(hint, style_columns())).centered());

    f.render_widget(Paragraph::new(lines), inner);
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
