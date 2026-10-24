//! What takes the panels' place: the viewer, the diff and the editor.

use super::*;

pub(super) fn render_viewer(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> (usize, usize, Rect) {
    if let Some(viewer_state) = app_state.viewer() {
        let filename = shown_name(&viewer_state.file_path);
        let prefix = if viewer_state.from_edit { "Edit" } else { "View" };
        let title = format!(" {}: {} ", prefix, filename);

        let border_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).border_style(style_border());

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Hex rows carry their own offset, so the gutter is not needed there,
        // and line numbers mean nothing down the side of a picture. F11 can
        // turn it off for text too.
        let no_gutter = viewer_state.mode != ViewMode::Text || !app_state.options.line_numbers;
        // Wide enough for the last line's number - lines, not the rows a
        // wrapped one is broken into.
        let line_num_width = if no_gutter { 0 } else { (viewer_state.line_count().to_string().len() as u16).max(3) + 2 };

        let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(line_num_width), Constraint::Min(0)]).split(inner_area);

        let viewport_height = inner_area.height as usize;
        let start = viewer_state.scroll_offset;
        let end = (start + viewport_height).min(viewer_state.total_lines);
        // Line numbers, unless the gutter is zero-width - it must not be
        // formatted at all then.
        if !no_gutter {
            let num_width = (line_num_width as usize).saturating_sub(1);
            // A wrapped line is numbered on its first row only.
            let line_numbers: Vec<Line> = (start..end)
                .map(|index| {
                    let (line, from, _) = viewer_state.row(index);
                    let number = if from == 0 { (line + 1).to_string() } else { String::new() };
                    Line::from(Span::styled(format!("{number:>num_width$} "), style_columns()))
                })
                .collect();
            let line_number_para = Paragraph::new(line_numbers).style(Style::default().bg(crate::display::gutter()));
            f.render_widget(line_number_para, chunks[0]);
        }

        // An image sits in the middle of the viewer when it is smaller than it -
        // always so for Fit. Centred here rather than padded with spaces, which
        // a copy would pick up. Clicks map through this same area.
        let content_area = if viewer_state.mode == ViewMode::Image {
            centered(chunks[1], viewer_state.image_columns, viewer_state.total_lines)
        } else {
            chunks[1]
        };

        // Render content
        if viewer_state.from_edit {
            let binary_msg = Paragraph::new("Binary file detected. Press Esc to return.").alignment(Alignment::Center).style(style_title());
            f.render_widget(binary_msg, chunks[1]);
        } else {
            // Both modes render from the same line text, so the selection and
            // the copy see exactly what is on screen.
            let selection = viewer_state.selected_range();
            // Not on a picture, whose characters only happen to be letters.
            let marking = app_state.find_shown && viewer_state.mode != ViewMode::Image;
            let needle = marking.then(|| crate::find::Needle::new(&app_state.find_term)).flatten();
            // The line the last row came from, as characters, with its
            // matches: a wrapped line fills row after row, and a minified one
            // can be megabytes, so it is laid out once a frame rather than
            // once for every row it fills.
            let mut held: Option<LaidOut> = None;
            let content_lines: Vec<Line> = (start..end)
                .map(|index| {
                    // The row's piece of its line - the whole line, unless
                    // wrapping. Matches and the selection are worked out on
                    // the whole line and cut down to the piece, so one that
                    // crosses from one row to the next shows on both.
                    let (line, row_from, row_to) = viewer_state.row(index);
                    if held.as_ref().is_none_or(|(held_line, _, _)| *held_line != line) {
                        let whole = viewer_state.line_text(line);
                        let matches = needle.as_ref().map(|needle| needle.find_all(&whole)).unwrap_or_default();
                        held = Some((line, whole.chars().collect(), matches));
                    }
                    let (_, characters, matches) = held.as_ref().expect("held just above");
                    let width = characters.len();
                    let (row_from, row_to) = (row_from.min(width), row_to.min(width));
                    let on_row = |from: usize, to: usize| {
                        let (from, to) = (from.clamp(row_from, row_to), to.clamp(row_from, row_to));
                        (from < to).then(|| (from - row_from, to - row_from))
                    };
                    let text: String = characters[row_from..row_to].iter().collect();
                    let mut spans = match viewer_state.image_colors.get(line) {
                        Some(colors) if viewer_state.mode == ViewMode::Image => colored_spans(&text, colors, viewer_state.image_backgrounds.get(line).map(Vec::as_slice)),
                        _ => vec![Span::styled(text, style_file())],
                    };
                    // Matches come left to right and never overlap, so the
                    // ones on this row are found by halving rather than by
                    // going through every one the line has.
                    let first = matches.partition_point(|&(_, to)| to <= row_from);
                    for (from, to) in matches[first..].iter().take_while(|&&(from, _)| from < row_to).filter_map(|&(from, to)| on_row(from, to)) {
                        spans = overlay_range(spans, from, to, style_match());
                    }

                    if let Some(((first_line, first_col), (last_line, last_col))) = selection
                        && (first_line..=last_line).contains(&line)
                    {
                        let to = if line == last_line { last_col.min(width) } else { width };
                        let from = if line == first_line { first_col.min(to) } else { 0 };
                        if let Some((from, to)) = on_row(from, to) {
                            spans = overlay_range(spans, from, to, style_selection());
                        }
                    }
                    Line::from(skip_columns(spans, viewer_state.horizontal_offset))
                })
                .collect();

            let content_para = Paragraph::new(content_lines).style(style_file());
            f.render_widget(content_para, content_area);
        }

        // The viewport is the whole pane even when an image is centred in part
        // of it: fit and fill are worked out from the space available.
        (viewport_height, chunks[1].width as usize, content_area)
    } else {
        (0, 0, Rect::default())
    }
}

/// Shift+F3: the two files side by side, each headed by its path, with the
/// mark `diff -y` uses between them - `|` changed, `<` only on the left, `>`
/// only on the right. Lines that differ are shaded as a selection is. Gives
/// back the rows shown and the width of each side's text, for scrolling.
pub(super) fn render_diff(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) -> (usize, usize) {
    let block = Block::default().title(Line::from(Span::styled(" Compare ", style_title())).centered()).borders(Borders::ALL).border_style(style_border());
    let inner = block.inner(area);
    // A difference near the end is jumped to with the rows after it, not past
    // them: the last row stays at the bottom, as scrolling keeps it.
    if let Screen::Diff(state) = &mut app_state.screen {
        let rows = (inner.height as usize).saturating_sub(1);
        state.scroll_offset = state.scroll_offset.min(state.rows.len().saturating_sub(rows));
    }
    let app_state = &*app_state;
    let Screen::Diff(state) = &app_state.screen else {
        return (0, 0);
    };
    f.render_widget(block, area);

    let [left_area, marker_area, right_area] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(3), Constraint::Fill(1)]).areas(inner);
    let [_, marker_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(marker_area);
    let height = inner.height.saturating_sub(1) as usize;
    let shown = &state.rows[state.scroll_offset.min(state.rows.len())..(state.scroll_offset + height).min(state.rows.len())];

    let markers: Vec<Line> = shown.iter().map(|row| Line::from(Span::styled(format!(" {} ", row.marker()), style_columns().add_modifier(Modifier::BOLD)))).collect();
    f.render_widget(Paragraph::new(markers), marker_area);

    let gutter_width = if app_state.options.line_numbers { (state.left.len().max(state.right.len()).to_string().len() as u16).max(3) + 1 } else { 0 };
    let mut text_width = 0;
    for (area, path, lines, is_left) in [(left_area, &state.left_path, &state.left, true), (right_area, &state.right_path, &state.right, false)] {
        let [header_area, body_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        // A path cut short gains "..." on top of what it keeps, so it keeps
        // three columns fewer - or the end of the file's name was clipped.
        let width = header_area.width as usize;
        let header = match crate::utils::limit_path_string(path, width) {
            whole if display_width(&whole) <= width => whole,
            _ => crate::utils::limit_path_string(path, width.saturating_sub(3)),
        };
        f.render_widget(Paragraph::new(Line::from(Span::styled(header, style_title()))), header_area);

        let [gutter_area, text_area] = Layout::horizontal([Constraint::Length(gutter_width), Constraint::Min(0)]).areas(body_area);
        text_width = text_area.width as usize;
        let mut numbers = Vec::with_capacity(shown.len());
        let mut texts = Vec::with_capacity(shown.len());
        for row in shown {
            let line = if is_left { row.left } else { row.right };
            let style = if row.same || line.is_none() { style_file() } else { style_file().patch(style_selection()) };
            let number = line.map(|index| (index + 1).to_string()).unwrap_or_default();
            numbers.push(Line::from(Span::styled(format!("{number:>width$} ", width = (gutter_width as usize).saturating_sub(1)), style_columns())));
            let text = line.and_then(|index| lines.get(index)).map(|line| crate::utils::printable_line(line)).unwrap_or_default();
            texts.push(Line::from(skip_columns(vec![Span::raw(text)], state.horizontal_offset)).style(style));
        }
        if gutter_width > 0 {
            f.render_widget(Paragraph::new(numbers).style(Style::default().bg(crate::display::gutter())), gutter_area);
        }
        f.render_widget(Paragraph::new(texts), text_area);
    }
    (height, text_width)
}

pub(super) fn render_editor(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) -> usize {
    let line_numbers = app_state.options.line_numbers;
    let (viewport_height, content_area) = if let Screen::Editor(editor_state) = &mut app_state.screen {
        let filename = shown_name(&editor_state.file_path);
        let modified = if editor_state.modified { " [Modified]" } else { "" };
        let title = format!(" Edit: {}{} ", filename, modified);

        let border_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).border_style(style_border());

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Calculate line number gutter width. With the numbers turned off it is
        // zero wide, and the click mapping follows, since it reads the content
        // area as drawn.
        let total_lines = editor_state.lines.len();
        let line_num_width = if line_numbers { (total_lines.to_string().len() as u16).max(3) + 2 } else { 0 };

        let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(line_num_width), Constraint::Min(0)]).split(inner_area);

        let viewport_height = inner_area.height as usize;
        editor_state.keep_in_view(viewport_height);
        let start = editor_state.scroll_offset;
        let end = (start + viewport_height).min(total_lines);
        let num_width = (line_num_width as usize).saturating_sub(1);

        // Render line numbers
        let style_current_line = style_title().add_modifier(Modifier::BOLD);
        let numbers: Vec<Line> = (start..end)
            .map(|line_num| {
                let style = if line_num == editor_state.cursor_line { style_current_line } else { style_columns() };
                Line::from(Span::styled(format!("{:>width$} ", line_num + 1, width = num_width), style))
            })
            .collect();
        if line_numbers {
            let line_number_para = Paragraph::new(numbers).style(Style::default().bg(crate::display::gutter()));
            f.render_widget(line_number_para, chunks[0]);
        }

        // Auto-scroll to keep cursor visible (disabled during mouse scrolling)
        if editor_state.auto_scroll {
            // In columns, as skip_columns scrolls: a wide character takes two.
            let visual_cursor_col = crate::utils::display_column(&editor_state.lines[editor_state.cursor_line], editor_state.cursor_col);
            let viewport_width = chunks[1].width as usize;
            if visual_cursor_col < editor_state.horizontal_offset {
                editor_state.horizontal_offset = visual_cursor_col;
            }
            if visual_cursor_col >= editor_state.horizontal_offset + viewport_width {
                editor_state.horizontal_offset = visual_cursor_col - viewport_width + 1;
            }
        }
        let h_offset = editor_state.horizontal_offset;

        // Syntax colours first, then the selection over them, then the cursor.
        let has_highlighting = !editor_state.highlighted_lines.is_empty();
        // Only marked: the syntax colours under it stay, for the terminal's
        // cursor to sit on, unless show_cursor paints the cell for Classic.
        let cursor_style = Style::new().add_modifier(EDITOR_CURSOR);
        let selection = editor_state.selection();
        let needle = app_state.find_shown.then(|| crate::find::Needle::new(&app_state.find_term)).flatten();
        let mut content_lines: Vec<Line> = Vec::with_capacity(end - start);

        for (idx, line) in editor_state.lines[start..end].iter().enumerate() {
            let actual_line_idx = start + idx;

            let mut spans: Vec<Span<'static>> = if has_highlighting && actual_line_idx < editor_state.highlighted_lines.len() {
                editor_state.highlighted_lines[actual_line_idx].iter().map(|span| Span::styled(printable_line(&span.content), span.style)).collect()
            } else {
                vec![Span::styled(printable_line(line), style_file())]
            };

            for (from, to) in needle.as_ref().map(|needle| needle.find_all(line)).unwrap_or_default() {
                spans = overlay_range(spans, visual_column(line, from), visual_column(line, to), style_match());
            }

            if let Some(((first_line, first_col), (last_line, last_col))) = selection
                && (first_line..=last_line).contains(&actual_line_idx)
            {
                let from = if actual_line_idx == first_line { visual_column(line, first_col) } else { 0 };
                let to = if actual_line_idx == last_line { visual_column(line, last_col) } else { visual_column(line, line.chars().count()) };
                spans = overlay_range(spans, from, to, style_selection());
            }

            if actual_line_idx == editor_state.cursor_line {
                spans = place_cursor(spans, visual_column(line, editor_state.cursor_col), cursor_style);
            }

            content_lines.push(Line::from(skip_columns(spans, h_offset)));
        }

        let content_para = Paragraph::new(content_lines);
        f.render_widget(content_para, chunks[1]);

        (viewport_height, chunks[1])
    } else {
        (0, Rect::default())
    };

    app_state.editor_content_area = content_area;
    viewport_height
}
