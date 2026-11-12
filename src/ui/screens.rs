//! What takes the panels' place: the viewer, the diff and the editor.

use super::*;

pub(super) fn render_viewer(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> (usize, usize, Rect, Vec<ScrollBar>, Option<crate::minimap::Minimap>) {
    let mut bars = Vec::new();
    let mut minimap = None;
    let bar_style = app_state.options.scroll_bar;
    if let Some(viewer_state) = app_state.viewer() {
        let filename = shown_name(&viewer_state.file_path);
        let prefix = if viewer_state.from_edit { t!("Edit:") } else { t!("View:") };
        let title = format!(" {} {} ", prefix, filename);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, style_title())).centered())
            .borders(Borders::ALL)
            .border_type(crate::display::border_type())
            .border_style(style_border());

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
        // A picture has nothing to map, nor a file refused as binary.
        let mappable = viewer_state.mode != ViewMode::Image && !viewer_state.from_edit;
        let (text_area, map) = if mappable { make_room(chunks[1], viewer_state.total_lines, viewport_height, &app_state.options) } else { (chunks[1], None) };
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
            text_area
        };

        // Render content
        if viewer_state.from_edit {
            let binary_msg = Paragraph::new(t!("Binary file detected. Press Esc to return.")).alignment(Alignment::Center).style(style_title());
            f.render_widget(binary_msg, chunks[1]);
        } else {
            // Both modes render from the same line text, so the selection and
            // the copy see exactly what is on screen.
            let selection = viewer_state.selected_range();
            // Not on a picture, whose characters only happen to be letters.
            let marking = app_state.find_shown && viewer_state.mode != ViewMode::Image;
            let needle = marking.then(|| app_state.find_needle()).flatten();
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

            let rows = Extent {
                content: viewer_state.total_lines,
                view: viewport_height,
                offset: viewer_state.scroll_offset,
            };
            bars.extend(vertical_bar(f, area, rows, bar_style));
            // Wrapped, every row fits across, and there is nowhere to go.
            if !viewer_state.wrapping() {
                let columns = Extent {
                    content: viewer_state.content_width(),
                    view: text_area.width as usize,
                    offset: viewer_state.horizontal_offset,
                };
                bars.extend(horizontal_bar(f, area, text_area, columns, bar_style));
            }
            if let Some(map) = map {
                // A wrapped line fills row after row, and a minified one can
                // be megabytes: it is laid out once for all the rows the map
                // samples from it, as the text above is.
                let mut held: Option<(usize, Vec<char>)> = None;
                draw_minimap(f, &map, start, |index| {
                    let (line, from, to) = viewer_state.row(index);
                    if held.as_ref().is_none_or(|(held_line, _)| *held_line != line) {
                        held = Some((line, viewer_state.line_text(line).chars().collect()));
                    }
                    let (_, characters) = held.as_ref().expect("held just above");
                    let piece: String = characters[from.min(characters.len())..to.min(characters.len())].iter().collect();
                    let marked = needle.as_ref().is_some_and(|needle| !needle.find_all(&piece).is_empty());
                    MapRow::of(&piece, usize::from(map.area.width), marked)
                });
                minimap = Some(map);
            }
        }

        // The viewport is the whole pane even when an image is centred in part
        // of it: fit and fill are worked out from the space available.
        (viewport_height, text_area.width as usize, content_area, bars, minimap)
    } else {
        (0, 0, Rect::default(), bars, minimap)
    }
}

/// Shift+F3: the two files side by side, each headed by its path, with the
/// mark `diff -y` uses between them - `|` changed, `<` only on the left, `>`
/// only on the right. Lines that differ are shaded as a selection is. Gives
/// back the rows shown and the width of each side's text, for scrolling.
/// Returns the rows and the width each side's text had, and the scroll kept
/// to them.
pub(super) fn render_diff(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> (usize, usize, usize, Option<crate::minimap::Minimap>) {
    let block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Compare")), style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .border_style(style_border());
    let inner = block.inner(area);
    let Screen::Diff(state) = &app_state.screen else {
        return (0, 0, 0, None);
    };
    // A difference near the end is jumped to with the rows after it, not past
    // them: the last row stays at the bottom, as scrolling keeps it.
    let scroll = state.scroll_offset.min(state.rows.len().saturating_sub((inner.height as usize).saturating_sub(1)));
    f.render_widget(block, area);

    // The map beside both, under the paths' row: the left file down its
    // left half, the right down its right, and the differences lit.
    let body = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };
    let (body, map) = make_room(body, state.rows.len(), body.height as usize, &app_state.options);
    let inner = Rect { width: body.width, ..inner };
    let [left_area, marker_area, right_area] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(3), Constraint::Fill(1)]).areas(inner);
    let [_, marker_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(marker_area);
    let height = inner.height.saturating_sub(1) as usize;
    let shown = &state.rows[scroll.min(state.rows.len())..(scroll + height).min(state.rows.len())];

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
    if let Some(map) = map {
        draw_minimap(f, &map, scroll, |index| {
            let row = &state.rows[index];
            // Each file half the map's columns.
            let bands = usize::from(map.area.width / 2);
            let full = |lines: &[String], line: Option<usize>| line.and_then(|line| lines.get(line)).map_or_else(|| vec![0.0; bands], |text| crate::minimap::fullness(text, bands));
            let mut fill = full(&state.left, row.left);
            fill.extend(full(&state.right, row.right));
            MapRow {
                fill,
                mark: (!row.same).then(|| palette().selected_marker),
            }
        });
    }
    (height, text_width, scroll, map)
}

/// Returns the rows the text had room for, and the scroll down and across
/// that keep the cursor in view.
pub(super) fn render_editor(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, drawn: &mut Drawn) -> (usize, Option<(usize, usize)>) {
    let line_numbers = app_state.options.line_numbers;
    let mut bars = Vec::new();
    let bar_style = app_state.options.scroll_bar;
    let (viewport_height, content_area, scrolls) = if let Screen::Editor(editor_state) = &app_state.screen {
        let filename = shown_name(&editor_state.file_path);
        let modified = if editor_state.modified { format!(" [{}]", t!("Modified")) } else { String::new() };
        let title = format!(" {} {}{} ", t!("Edit:"), filename, modified);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, style_title())).centered())
            .borders(Borders::ALL)
            .border_type(crate::display::border_type())
            .border_style(style_border());

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Calculate line number gutter width. With the numbers turned off it is
        // zero wide, and the click mapping follows, since it reads the content
        // area as drawn.
        let total_lines = editor_state.lines.len();
        let line_num_width = if line_numbers { (total_lines.to_string().len() as u16).max(3) + 2 } else { 0 };

        let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(line_num_width), Constraint::Min(0)]).split(inner_area);

        let viewport_height = inner_area.height as usize;
        let (text_area, map) = make_room(chunks[1], total_lines, viewport_height, &app_state.options);
        let start = editor_state.scroll_in_view(viewport_height);
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
        let mut h_offset = editor_state.horizontal_offset;
        if editor_state.auto_scroll {
            // In columns, as skip_columns scrolls: a wide character takes two.
            let visual_cursor_col = crate::utils::display_column(&editor_state.lines[editor_state.cursor_line], editor_state.cursor_col);
            let viewport_width = text_area.width as usize;
            if visual_cursor_col < h_offset {
                h_offset = visual_cursor_col;
            }
            if visual_cursor_col >= h_offset + viewport_width {
                h_offset = visual_cursor_col - viewport_width + 1;
            }
        }

        // Syntax colours first, then the selection over them, then the cursor.
        let has_highlighting = !editor_state.highlighted_lines.is_empty();
        // Only marked: the syntax colours under it stay, for the terminal's
        // cursor to sit on, unless show_cursor paints the cell for Classic.
        let cursor_style = Style::new().add_modifier(EDITOR_CURSOR);
        let selection = editor_state.selection();
        let needle = app_state.find_shown.then(|| crate::find::Needle::with(&app_state.recall.find_term, app_state.recall.find_regex).ok().flatten()).flatten();
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
        f.render_widget(content_para, text_area);

        let rows = Extent {
            content: editor_state.lines.len(),
            view: viewport_height,
            offset: start,
        };
        bars.extend(vertical_bar(f, area, rows, bar_style));
        // A column more than the widest line, for the cursor at its end; and
        // never less than what shows, since the editor scrolls right as far
        // as it is asked to.
        let view = text_area.width as usize;
        let columns = Extent {
            content: (editor_state.widest_line() + 1).max(h_offset + view),
            view,
            offset: h_offset,
        };
        bars.extend(horizontal_bar(f, area, text_area, columns, bar_style));
        if let Some(map) = map {
            draw_minimap(f, &map, start, |index| {
                let line = &editor_state.lines[index];
                MapRow::of(line, usize::from(map.area.width), needle.as_ref().is_some_and(|needle| !needle.find_all(line).is_empty()))
            });
            drawn.minimap = Some(map);
        }

        (viewport_height, text_area, Some((start, h_offset)))
    } else {
        (0, Rect::default(), None)
    };

    drawn.editor_content_area = content_area;
    drawn.scroll_bars = bars;
    (viewport_height, scrolls)
}

/// Alt+U: the active panel's entries as blocks, each as big as the room it
/// takes - outlined in its own colour, the chosen one in a heavier line and
/// filled, its name and size inside where they fit. Gives back the blocks
/// drawn, for the arrows and a click.
pub(super) fn render_treemap(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> Vec<(crate::treemap::Tile, Rect)> {
    use crate::treemap::Tile;
    let side = app_state.active_side;
    let panel = app_state.panel(side);
    let total = app_state.space_map(side).map_or(0, |map| map.total);
    let counting = !app_state.workers.dir_sizing.is_empty() || !app_state.workers.dir_size_queue.is_empty();
    let figure = if counting { format!("{}…", format_size(total)) } else { format_size(total) };
    let name = Some(shown_name(&panel.dir)).filter(|name| !name.is_empty()).unwrap_or_else(|| printable_name(&panel.dir.to_string_lossy()));
    let title = format!(" {} {} · {} ", t!("Space map"), name, figure);
    let frame = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .border_style(style_border());
    let inner = frame.inner(area);
    f.render_widget(frame, area);

    let blocks = crate::treemap::layout(&app_state.treemap_sizes(), inner);
    if blocks.is_empty() {
        let empty = if counting { t!("Counting...") } else { t!("Nothing found") };
        f.render_widget(Paragraph::new(empty).alignment(Alignment::Center).style(style_file()), Rect::new(inner.x, inner.y + inner.height / 2, inner.width, 1.min(inner.height)));
        return blocks;
    }
    let cursor = panel.state.selected();
    let sizes = app_state.space_map(side).map(|map| map.sizes).unwrap_or_default();
    for &(tile, rect) in &blocks {
        let (color, name, size) = match tile {
            Tile::Entry(index) => {
                let Some(item) = panel.children.get(index) else { continue };
                let color = if item.is_dir { palette().directory } else { crate::display::file_style(item.leaf(), &item.extension).0 };
                let name = format!("{}{}", printable_name(&item.name_full), if item.is_dir { std::path::MAIN_SEPARATOR_STR } else { "" });
                let path = item.path_in(&panel.dir);
                let still = item.is_dir && app_state.dir_size_so_far(&path).is_some();
                let bytes = format_size(sizes.get(index).copied().unwrap_or(0));
                (color, name, if still { format!("{bytes}…") } else { bytes })
            }
            Tile::Rest(count, bytes) => (palette().directory_dark, format!("+{count}"), format_size(bytes)),
        };
        let chosen = matches!(tile, Tile::Entry(index) if Some(index) == cursor);
        let fill = if chosen { Style::default().bg(palette().selected_background) } else { Style::default() };
        // Too small for an outline: a patch of its colour.
        if rect.width < 3 || rect.height < 2 {
            let patch = if chosen { "█" } else { "▒" };
            for y in rect.top()..rect.bottom() {
                for x in rect.left()..rect.right() {
                    f.buffer_mut()[(x, y)].set_symbol(patch).set_style(Style::default().fg(color));
                }
            }
            continue;
        }
        let outline = Block::default()
            .borders(Borders::ALL)
            .border_type(if chosen { ratatui::widgets::BorderType::Thick } else { crate::display::border_type() })
            .border_style(Style::default().fg(color).add_modifier(if chosen { Modifier::BOLD } else { Modifier::empty() }))
            .style(fill);
        let room = outline.inner(rect);
        f.render_widget(outline, rect);
        let width = room.width as usize;
        let mut lines = vec![Line::from(Span::styled(tail_of(&name, width), Style::default().fg(color).add_modifier(Modifier::BOLD)))];
        if !size.is_empty() {
            lines.push(Line::from(Span::styled(tail_of(&size, width), style_file())));
        }
        if room.height > 0 {
            f.render_widget(Paragraph::new(lines).style(fill), room);
        } else if rect.width > 2 {
            // Only the outline has room: the name along its top.
            let top = Rect::new(rect.x + 1, rect.y, rect.width - 2, 1);
            f.render_widget(Paragraph::new(Span::styled(tail_of(&name, top.width as usize), Style::default().fg(color).add_modifier(Modifier::BOLD))), top);
        }
    }
    blocks
}
