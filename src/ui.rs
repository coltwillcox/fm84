use crate::app::AppState;
use crate::constants::*;
use crate::utils::*;
use chrono::Local;
use ratatui::{
    Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use std::path::PathBuf;

// Pre-computed styles used throughout rendering
const STYLE_BORDER: Style = Style::new().fg(COLOR_BORDER);
const STYLE_TITLE: Style = Style::new().fg(COLOR_TITLE);
const STYLE_COLUMNS: Style = Style::new().fg(COLOR_COLUMNS);
const STYLE_FILE: Style = Style::new().fg(COLOR_FILE);
const STYLE_DIR: Style = Style::new().fg(COLOR_DIRECTORY);
const STYLE_DIR_DARK: Style = Style::new().fg(COLOR_DIRECTORY_DARK);

pub fn render_ui<B: Backend>(terminal: &mut Terminal<B>, app_state: &mut AppState) {
    // Update cached clock
    let current_time = Local::now().format(" %H:%M:%S ").to_string();
    if app_state.cached_clock != current_time {
        app_state.cached_clock = current_time;
    }

    let _ = terminal.draw(|f| {
        let area = f.area();

        // Guard against terminal too small to render
        if area.height < 10 || area.width < 30 {
            let msg = Paragraph::new("Terminal too small")
                .alignment(Alignment::Center)
                .style(STYLE_TITLE);
            let y = area.height / 2;
            if y < area.height {
                f.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
            }
            return;
        }

        let chunks_main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Percentage(100), Constraint::Length(1), Constraint::Length(3)])
            .split(area);

        render_top_panel(f, chunks_main[0], &app_state.cached_clock);
        render_path_bar(f, chunks_main[1], &app_state.dir_left, &app_state.dir_right, area.width, app_state.is_left_active);
        if app_state.is_f3_displayed {
            app_state.viewer_viewport_height = render_viewer(f, chunks_main[2], app_state);
        } else if app_state.is_f4_displayed {
            app_state.editor_viewport_height = render_editor(f, chunks_main[2], app_state);
        } else {
            app_state.page_size = render_file_tables(f, chunks_main[2], app_state);
        }
        render_bottom_panel(f, chunks_main[3], app_state);
        render_fkey_bar(f, chunks_main[4]);

        if app_state.is_error_displayed {
            render_error_popup(f, area, app_state);
        } else if app_state.large_file.is_some() {
            render_large_file_popup(f, area, app_state);
        } else if app_state.is_editor_save_prompt {
            render_editor_save_popup(f, area);
        } else if app_state.is_f1_displayed {
            render_help_popup(f, area);
        } else if app_state.is_f5_displayed {
            render_copy_move_popup(f, area, app_state, true);
        } else if app_state.is_f6_displayed {
            render_copy_move_popup(f, area, app_state, false);
        } else if app_state.is_f7_displayed {
            render_create_popup(f, area, app_state);
        } else if app_state.is_f8_displayed {
            render_delete_popup(f, area, app_state);
        }
    });
}

fn render_top_panel(f: &mut ratatui::Frame<'_>, area: Rect, cached_clock: &str) {
    let logo = Span::styled(format!(" {} ", ICON_LOGO), STYLE_TITLE);
    let title = Span::styled(format!(" {} v{} ", TITLE, VERSION), STYLE_TITLE);
    let clock = Span::styled(cached_clock, STYLE_TITLE);

    let block_top = Block::default()
        .title_top(Line::from(logo).left_aligned())
        .title_top(Line::from(title).centered())
        .title_top(Line::from(clock).right_aligned())
        .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
        .border_style(STYLE_BORDER);

    f.render_widget(block_top, area);
}

fn render_path_bar(f: &mut ratatui::Frame<'_>, area: Rect, dir_left: &PathBuf, dir_right: &PathBuf, total_width: u16, is_left_active: bool) {
    let length_left = ((total_width as usize).saturating_sub(3)) / 2;
    let length_right = ((total_width as usize).saturating_sub(2)) / 2;

    let path_left = limit_path_string(dir_left, length_left.saturating_sub(8));
    let path_right = limit_path_string(dir_right, length_right.saturating_sub(8));

    let (color_left, color_right) = if is_left_active {
        (STYLE_DIR, STYLE_DIR_DARK)
    } else {
        (STYLE_DIR_DARK, STYLE_DIR)
    };

    let border_line = vec![
        Span::styled("├──", STYLE_BORDER),
        Span::styled(format!(" {} ", path_left), color_left),
        Span::styled(format!("{}─┬──", "─".repeat(length_left.saturating_sub(display_width(&path_left).saturating_add(5)))), STYLE_BORDER),
        Span::styled(format!(" {} ", path_right), color_right),
        Span::styled(format!("{}─┤", "─".repeat(length_right.saturating_sub(display_width(&path_right).saturating_add(5)))), STYLE_BORDER),
    ];

    f.render_widget(Paragraph::new(Line::from(border_line)), area);
}

fn render_file_tables(f: &mut ratatui::Frame<'_>, chunk: Rect, app_state: &mut AppState) -> u16 {
    let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(50)]).split(chunk);

    let widths = [Constraint::Length(2), Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(10), Constraint::Length(1), Constraint::Percentage(15), Constraint::Length(1), Constraint::Length(15)];

    let is_f2_displayed = app_state.is_f2_displayed;
    let table_style = |active: bool| {
        Style::default()
            .bg(if active {
                if is_f2_displayed { COLOR_RENAME_BACKGROUND } else { COLOR_SELECTED_BACKGROUND }
            } else {
                COLOR_SELECTED_BACKGROUND_INACTIVE
            })
            .fg(COLOR_SELECTED_FOREGROUND)
            .add_modifier(Modifier::BOLD)
    };

    // Viewport height (subtract 1 for header row)
    let viewport_height = chunks[0].height.saturating_sub(1) as usize;

    let header = make_header_row();

    // Build only visible rows for left panel
    let (rows_left, offset_left) = build_viewport_rows(app_state, true, viewport_height);
    let mut state_left_view = TableState::default();
    state_left_view.select(app_state.state_left.selected().map(|s| s.saturating_sub(offset_left)));

    let table_left = Table::new(rows_left, widths.clone())
        .block(Block::default().borders(Borders::LEFT).border_style(STYLE_BORDER))
        .header(header.clone())
        .row_highlight_style(table_style(app_state.is_left_active))
        .column_spacing(1);
    f.render_stateful_widget(table_left, chunks[0], &mut state_left_view);

    // Cache the separator string based on height
    let separator_height = chunks[0].height;
    if app_state.cached_separator_height != separator_height {
        app_state.cached_separator_height = separator_height;
        app_state.cached_separator = "│\n".repeat(separator_height.saturating_sub(1) as usize) + "│";
    }
    let separator_vertical = Paragraph::new(Text::raw(&app_state.cached_separator)).style(STYLE_BORDER);
    f.render_widget(separator_vertical, chunks[1]);

    // Build only visible rows for right panel
    let (rows_right, offset_right) = build_viewport_rows(app_state, false, viewport_height);
    let mut state_right_view = TableState::default();
    state_right_view.select(app_state.state_right.selected().map(|s| s.saturating_sub(offset_right)));

    let table_right = Table::new(rows_right, widths)
        .block(Block::default().borders(Borders::RIGHT).border_style(STYLE_BORDER))
        .header(header)
        .row_highlight_style(table_style(!app_state.is_left_active))
        .column_spacing(1);
    f.render_stateful_widget(table_right, chunks[2], &mut state_right_view);

    chunks[0].height
}

/// Build only the rows visible in the viewport, returns (rows, start_offset)
fn build_viewport_rows(app_state: &AppState, is_left: bool, viewport_height: usize) -> (Vec<Row<'static>>, usize) {
    let children = if is_left { &app_state.children_left } else { &app_state.children_right };
    let state = if is_left { &app_state.state_left } else { &app_state.state_right };
    let selected_set = if is_left { &app_state.selected_left } else { &app_state.selected_right };
    let current_dir = if is_left { &app_state.dir_left } else { &app_state.dir_right };
    let selected = state.selected().unwrap_or(0);
    let total = children.len();

    if total == 0 {
        return (Vec::new(), 0);
    }

    // Calculate viewport window centered on selection
    let half_view = viewport_height / 2;
    let start = if selected <= half_view {
        0
    } else if selected + half_view >= total {
        total.saturating_sub(viewport_height)
    } else {
        selected.saturating_sub(half_view)
    };
    let end = (start + viewport_height).min(total);

    let is_renaming_current_side = app_state.is_f2_displayed && (app_state.is_left_active == is_left);
    let border_cell = Cell::from(Span::styled("│", STYLE_BORDER));

    let mut rows = Vec::with_capacity(end - start);

    for index in start..end {
        let child = &children[index];
        let is_renaming_current_item = is_renaming_current_side && (index == selected);
        let is_selected = selected_set.contains(&child.name_full);

        // Keep original icon, change color if selected
        let icon = if child.is_dir { ICON_FOLDER } else { ICON_FILE };
        let file_color = color_for_extension(&child.extension);
        let text_color = if is_selected {
            COLOR_SELECTED_MARKER
        } else if child.is_dir {
            COLOR_DIRECTORY
        } else {
            file_color
        };
        let text_style = Style::default().fg(text_color);

        let (dir_prefix, dir_suffix) = if child.is_dir { ("[", "]") } else { ("", "") };

        let bracket_style = if is_selected {
            Style::default().fg(COLOR_SELECTED_MARKER)
        } else {
            Style::default().fg(COLOR_DIRECTORY_FIX)
        };

        let (name_cell, extension) = if is_renaming_current_item {
            // REVERSED survives Table row_highlight_style override
            let cursor_style = text_style.add_modifier(Modifier::REVERSED);
            let mut spans = vec![Span::styled(dir_prefix, bracket_style)];
            spans.extend(app_state.rename_input.cursor_spans(text_style, cursor_style));
            spans.push(Span::styled(dir_suffix, bracket_style));
            (Cell::from(Line::from(spans)), String::new())
        } else {
            (Cell::from(Line::from(vec![
                Span::styled(dir_prefix, bracket_style),
                Span::styled(child.name.clone(), text_style),
                Span::styled(dir_suffix, bracket_style),
            ])), child.extension.clone())
        };

        // Get size - for directories, show calculated size if available
        let size = if child.is_dir && child.name != ".." {
            if let Some(&calculated_size) = app_state.dir_sizes.get(&current_dir.join(&child.name_full)) {
                format_size(calculated_size)
            } else {
                child.size.clone()
            }
        } else {
            child.size.clone()
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(icon, Style::default().fg(text_color))),
            name_cell,
            border_cell.clone(),
            Cell::from(Span::styled(extension, text_style)),
            border_cell.clone(),
            Cell::from(Span::styled(size, text_style)),
            border_cell.clone(),
            Cell::from(Span::styled(child.modified.clone(), text_style)),
        ]));
    }

    (rows, start)
}

fn make_header_row() -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled("", STYLE_COLUMNS)),
        Cell::from(Span::styled("Name", STYLE_COLUMNS)),
        Cell::from(Span::styled("", STYLE_COLUMNS)),
        Cell::from(Span::styled("Ext", STYLE_COLUMNS)),
        Cell::from(Span::styled("", STYLE_COLUMNS)),
        Cell::from(Span::styled("Size", STYLE_COLUMNS)),
        Cell::from(Span::styled("", STYLE_COLUMNS)),
        Cell::from(Span::styled("Modified", STYLE_COLUMNS)),
    ])
}

fn render_viewer(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> usize {
    if let Some(viewer_state) = &app_state.viewer_state {
        let filename = viewer_state.file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        let prefix = if viewer_state.from_edit { "Edit" } else { "View" };
        let title = format!(" {}: {} ", prefix, filename);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, STYLE_TITLE)).centered())
            .borders(Borders::ALL)
            .border_style(STYLE_BORDER);

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Calculate line number gutter width
        let line_num_width = (viewer_state.total_lines.to_string().len() as u16).max(3) + 2;

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(line_num_width), Constraint::Min(0)])
            .split(inner_area);

        let viewport_height = inner_area.height as usize;
        let start = viewer_state.scroll_offset;
        let end = (start + viewport_height).min(viewer_state.total_lines);
        let num_width = line_num_width as usize - 1;

        // Render line numbers
        let line_numbers: Vec<Line> = (start..end)
            .map(|line_num| Line::from(Span::styled(
                format!("{:>width$} ", line_num + 1, width = num_width),
                STYLE_COLUMNS,
            )))
            .collect();
        let line_number_para = Paragraph::new(line_numbers)
            .style(Style::default().bg(ratatui::style::Color::Black));
        f.render_widget(line_number_para, chunks[0]);

        // Render content
        if viewer_state.is_binary {
            let binary_msg = Paragraph::new("Binary file detected. Press Esc to return.")
                .alignment(Alignment::Center)
                .style(STYLE_TITLE);
            f.render_widget(binary_msg, chunks[1]);
        } else {
            let content_lines: Vec<Line> = viewer_state.content_lines[start..end]
                .iter()
                .map(|line| Line::from(Span::raw(line.replace('\t', TAB_SPACES))))
                .collect();

            let content_para =
                Paragraph::new(content_lines)
                    .style(STYLE_FILE)
                    .scroll((0, viewer_state.horizontal_offset as u16));
            f.render_widget(content_para, chunks[1]);
        }

        viewport_height
    } else {
        0
    }
}

fn render_editor(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) -> usize {
    if let Some(editor_state) = &mut app_state.editor_state {
        let filename = editor_state.file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        let modified = if editor_state.modified { " [Modified]" } else { "" };
        let title = format!(" Edit: {}{} ", filename, modified);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, STYLE_TITLE)).centered())
            .borders(Borders::ALL)
            .border_style(STYLE_BORDER);

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Calculate line number gutter width
        let total_lines = editor_state.lines.len();
        let line_num_width = (total_lines.to_string().len() as u16).max(3) + 2;

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(line_num_width), Constraint::Min(0)])
            .split(inner_area);

        let viewport_height = inner_area.height as usize;
        let start = editor_state.scroll_offset;
        let end = (start + viewport_height).min(total_lines);
        let num_width = line_num_width as usize - 1;

        // Render line numbers
        let style_current_line = STYLE_TITLE.add_modifier(Modifier::BOLD);
        let line_numbers: Vec<Line> = (start..end)
            .map(|line_num| {
                let style = if line_num == editor_state.cursor_line { style_current_line } else { STYLE_COLUMNS };
                Line::from(Span::styled(
                    format!("{:>width$} ", line_num + 1, width = num_width),
                    style,
                ))
            })
            .collect();
        let line_number_para = Paragraph::new(line_numbers)
            .style(Style::default().bg(ratatui::style::Color::Black));
        f.render_widget(line_number_para, chunks[0]);

        // Auto-scroll to keep cursor visible (disabled during mouse scrolling)
        if editor_state.auto_scroll {
            let visual_cursor_col: usize = editor_state.lines[editor_state.cursor_line]
                .chars()
                .take(editor_state.cursor_col)
                .map(|c| if c == '\t' { TAB_SPACES.len() } else { 1 })
                .sum();
            let viewport_width = chunks[1].width as usize;
            if visual_cursor_col < editor_state.horizontal_offset {
                editor_state.horizontal_offset = visual_cursor_col;
            }
            if visual_cursor_col >= editor_state.horizontal_offset + viewport_width {
                editor_state.horizontal_offset = visual_cursor_col - viewport_width + 1;
            }
        }
        let h_offset = editor_state.horizontal_offset;

        // Render content with cursor and syntax highlighting
        let has_highlighting = !editor_state.highlighted_lines.is_empty();
        let cursor_style = Style::default().fg(COLOR_SELECTED_FOREGROUND).bg(COLOR_SELECTED_BACKGROUND);
        let mut content_lines: Vec<Line> = Vec::with_capacity(end - start);
        for (idx, line) in editor_state.lines[start..end].iter().enumerate() {
            let actual_line_idx = start + idx;

            if actual_line_idx == editor_state.cursor_line {
                // Expand tabs then split at the visual cursor position
                let visual_col: usize = line.chars().take(editor_state.cursor_col)
                    .map(|c| if c == '\t' { TAB_SPACES.len() } else { 1 })
                    .sum();
                let expanded = line.replace('\t', TAB_SPACES);
                let exp_chars: Vec<char> = expanded.chars().collect();
                let before: String = exp_chars[..visual_col.min(exp_chars.len())].iter().collect();
                let cursor_char = exp_chars.get(visual_col).copied().unwrap_or(' ');
                let after: String = if visual_col < exp_chars.len() {
                    exp_chars[visual_col + 1..].iter().collect()
                } else {
                    String::new()
                };

                content_lines.push(Line::from(vec![
                    Span::styled(before, STYLE_FILE),
                    Span::styled(cursor_char.to_string(), cursor_style),
                    Span::styled(after, STYLE_FILE),
                ]));
            } else if has_highlighting && actual_line_idx < editor_state.highlighted_lines.len() {
                let spans: Vec<Span> = editor_state.highlighted_lines[actual_line_idx]
                    .iter()
                    .map(|span| Span::styled(span.content.replace('\t', TAB_SPACES), span.style))
                    .collect();
                content_lines.push(Line::from(spans));
            } else {
                content_lines.push(Line::from(Span::styled(line.replace('\t', TAB_SPACES), STYLE_FILE)));
            }
        }

        let content_para = Paragraph::new(content_lines)
            .scroll((0, h_offset as u16));
        f.render_widget(content_para, chunks[1]);

        viewport_height
    } else {
        0
    }
}

fn render_segmented_status_bar(f: &mut ratatui::Frame<'_>, area: Rect, segments: &[&str]) {
    let mut spans = Vec::new();
    spans.push(Span::styled("├─", STYLE_BORDER));
    let mut used = 3usize; // "├─" (2) + "┤" (1)

    for (i, &seg) in segments.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("─", STYLE_BORDER));
            used += 1;
        }
        let padded = format!(" {} ", seg);
        used += display_width(&padded);
        spans.push(Span::styled(padded, STYLE_TITLE));
    }

    let fill = (area.width as usize).saturating_sub(used);
    spans.push(Span::styled("─".repeat(fill), STYLE_BORDER));
    spans.push(Span::styled("┤", STYLE_BORDER));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_status_bar(f: &mut ratatui::Frame<'_>, area: Rect, text: String, style: Style) {
    let text_len = display_width(&text);
    let status_line = vec![
        Span::styled("├─", STYLE_BORDER),
        Span::styled(text, style),
        Span::styled(
            "─".repeat((area.width as usize).saturating_sub(text_len).saturating_sub(3)),
            STYLE_BORDER,
        ),
        Span::styled("┤", STYLE_BORDER),
    ];
    f.render_widget(Paragraph::new(Line::from(status_line)), area);
}

fn render_bottom_panel(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let status_style = STYLE_TITLE.bg(COLOR_SELECTED_BACKGROUND);

    if app_state.is_f4_displayed {
        // Show editor status
        if let Some(editor_state) = &app_state.editor_state {
            let filename = editor_state.file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            let modified = if editor_state.modified { " [Modified]" } else { "" };
            let name_seg = format!("{}{}", filename, modified);
            let pos_seg = format!("Ln {}, Col {}", editor_state.cursor_line + 1, editor_state.cursor_col + 1);
            let lines_seg = format!("{} lines", editor_state.lines.len());
            render_segmented_status_bar(f, area, &[&name_seg, &pos_seg, &lines_seg, "F2/Ctrl+S Save", "Esc Exit"]);
        }
    } else if app_state.is_f3_displayed {
        // Show viewer status
        if let Some(viewer_state) = &app_state.viewer_state {
            let filename = viewer_state.file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            let line_seg = format!("Line {}/{}", viewer_state.scroll_offset + 1, viewer_state.total_lines);
            let size_seg = format_size(viewer_state.file_size);
            render_segmented_status_bar(f, area, &[filename, &line_seg, &size_seg, &viewer_state.syntax_name]);
        }
    } else if !app_state.search_input.is_empty() {
        // Show search string
        let text = format!(" Search: {} ", app_state.search_input);
        render_status_bar(f, area, text, status_style);
    } else {
        // Show panel stats: selected/total files and selected/total size
        // Returns (count_part, size_part) e.g. ("0/5", "1.2 KiB") or ("2/5", "800 B/1.2 KiB")
        let panel_stat = |children: &[crate::app::Item], selected_set: &std::collections::HashSet<String>, current_dir: &PathBuf, dir_sizes: &std::collections::HashMap<PathBuf, u64>| -> (String, String) {
            let item_size = |c: &crate::app::Item| -> u64 {
                if c.is_dir {
                    dir_sizes.get(&current_dir.join(&c.name_full)).copied().unwrap_or(0)
                } else {
                    c.size_bytes
                }
            };
            let total_count = children.iter().filter(|c| c.name != "..").count();
            let total_size: u64 = children.iter().filter(|c| c.name != "..").map(|c| item_size(c)).sum();

            if selected_set.is_empty() {
                (format!("0/{}", total_count), format_size(total_size))
            } else {
                let is_selected = |c: &crate::app::Item| c.name != ".." && selected_set.contains(&c.name_full);
                let sel_count = children.iter().filter(|c| is_selected(c)).count();
                let sel_size: u64 = children.iter().filter(|c| is_selected(c)).map(item_size).sum();
                (format!("{}/{}", sel_count, total_count), format!("{}/{}", format_size(sel_size), format_size(total_size)))
            }
        };

        let (left_count, left_size) = panel_stat(&app_state.children_left, &app_state.selected_left, &app_state.dir_left, &app_state.dir_sizes);
        let (right_count, right_size) = panel_stat(&app_state.children_right, &app_state.selected_right, &app_state.dir_right, &app_state.dir_sizes);

        // " count - size " → len = 1 + count + 3 + size + 1
        let left_stat_len = 1 + left_count.len() + 3 + left_size.len() + 1;
        let right_stat_len = 1 + right_count.len() + 3 + right_size.len() + 1;

        let total_width = area.width as usize;
        let left_pad = (total_width.saturating_sub(3) / 2).saturating_sub(left_stat_len + 1);
        let right_pad = (total_width.saturating_sub(2) / 2).saturating_sub(right_stat_len + 1);

        let (left_style, right_style) = if app_state.is_left_active {
            (STYLE_TITLE, STYLE_DIR_DARK)
        } else {
            (STYLE_DIR_DARK, STYLE_TITLE)
        };

        let status_line = vec![
            Span::styled("├─", STYLE_BORDER),
            Span::styled(format!(" {}", left_count), left_style),
            Span::styled(" - ", STYLE_BORDER),
            Span::styled(format!("{} ", left_size), left_style),
            Span::styled(format!("{}┴─", "─".repeat(left_pad)), STYLE_BORDER),
            Span::styled(format!(" {}", right_count), right_style),
            Span::styled(" - ", STYLE_BORDER),
            Span::styled(format!("{} ", right_size), right_style),
            Span::styled(format!("{}┤", "─".repeat(right_pad)), STYLE_BORDER),
        ];
        f.render_widget(Paragraph::new(Line::from(status_line)), area);
    }
}

fn render_fkey_bar(f: &mut ratatui::Frame<'_>, area: Rect) {
    let block_bottom = Block::default()
        .title_bottom(Line::from(Span::styled(" F1 Help ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F2 Rename ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F3 View ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F4 Edit ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F5 Copy ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F6 Move ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F7 Create ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F8 Delete ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F9 Terminal ", STYLE_TITLE)).centered())
        .title_bottom(Line::from(Span::styled(" F10 Quit ", STYLE_TITLE)).centered())
        .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
        .border_style(STYLE_BORDER);
    f.render_widget(block_bottom, area);
}

fn render_error_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) {
    let popup_area = centered_rect(60, 20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Error ", STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    f.render_widget(
        Paragraph::new(app_state.error_message.clone()).alignment(Alignment::Center).style(STYLE_TITLE),
        popup_area.inner(Margin { vertical: 2, horizontal: 2 }),
    );
}

fn render_help_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let help_lines = vec![
        "F1 - This help",
        "F2 - Rename folder/file",
        "F3 - View file",
        "F4 - Edit file (Ctrl+S/F2 save)",
        "F5 - Copy to other panel",
        "F6 - Move to other panel",
        "F7 - Create directory",
        "F8 - Delete folder/file",
        "F9 - Open terminal",
        "F10 - Quit",
        "Space - Select/deselect file",
        "Type to search, Esc to clear",
    ];

    // 2 border rows + 1 top padding + 1 bottom padding + content lines
    let content_height = (help_lines.len() as u16) + 4;
    let popup_height = content_height.min(area.height);
    let popup_width = (area.width * 60 / 100).max(1);
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Help/About ", STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 2, horizontal: 2 });
    let max_len = help_lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let lines: Vec<Line> = help_lines.iter()
        .map(|&text| Line::from(Span::styled(format!("{:<width$}", text, width = max_len), STYLE_TITLE)))
        .collect();
    let help_para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(help_para, inner);
}

fn render_create_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let popup_area = centered_rect(60, 20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Create Directory ", STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    // Show input with block cursor (REVERSED so it's visible against paragraph bg)
    let cursor_style = STYLE_TITLE.add_modifier(Modifier::REVERSED);
    let input_line = Line::from(app_state.create_input.cursor_spans(STYLE_TITLE, cursor_style));
    f.render_widget(
        Paragraph::new(input_line).alignment(Alignment::Center).style(STYLE_TITLE.bg(COLOR_SELECTED_BACKGROUND)),
        popup_area.inner(Margin { vertical: 3, horizontal: 2 }),
    );

    // Instructions
    f.render_widget(
        Paragraph::new("Enter - Create    Esc - Cancel").alignment(Alignment::Center).style(STYLE_COLUMNS),
        popup_area.inner(Margin { vertical: 5, horizontal: 2 }),
    );
}

fn render_delete_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let count = app_state.delete_items.len();
    let popup_area = centered_rect(60, 30, area);

    let title = if count == 1 {
        let item_type = if app_state.delete_items[0].1 { "directory" } else { "file" };
        format!(" Delete {} ", item_type)
    } else {
        format!(" Delete {} items ", count)
    };

    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    // Message
    let message = if count == 1 {
        format!("Delete \"{}\"?", app_state.delete_items[0].0)
    } else {
        let names: Vec<&str> = app_state.delete_items.iter().map(|(name, _)| name.as_str()).collect();
        format!("Delete {} items?\n\n{}", count, names.join(", "))
    };
    f.render_widget(Paragraph::new(message).alignment(Alignment::Center).style(STYLE_TITLE), popup_area.inner(Margin { vertical: 2, horizontal: 2 }));

    // Instructions
    f.render_widget(
        Paragraph::new("Y / Enter - Yes    N / Esc - No").alignment(Alignment::Center).style(STYLE_COLUMNS),
        popup_area.inner(Margin { vertical: 6, horizontal: 2 }),
    );
}

/// Unified copy/move popup. `is_copy` = true for F5 copy, false for F6 move.
fn render_copy_move_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, is_copy: bool) {
    let popup_area = centered_rect(70, 35, area);
    let (items, verb) = if is_copy {
        (&app_state.copy_items, "Copy")
    } else {
        (&app_state.move_items, "Move")
    };
    let count = items.len();

    let title = if count == 1 {
        let item_type = if items[0].2 { "directory" } else { "file" };
        format!(" {} {} ", verb, item_type)
    } else {
        format!(" {} {} items ", verb, count)
    };

    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    // Source info
    let source_msg = if count == 1 {
        let source_name = items[0].0.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");
        format!("{} \"{}\"", verb, source_name)
    } else {
        let names: Vec<&str> = items.iter()
            .filter_map(|(src, _, _)| src.file_name().and_then(|n| n.to_str()))
            .collect();
        format!("{} {} items: {}", verb, count, names.join(", "))
    };
    f.render_widget(
        Paragraph::new(source_msg).alignment(Alignment::Center).style(STYLE_TITLE),
        popup_area.inner(Margin { vertical: 2, horizontal: 2 }),
    );

    // Destination directory
    let dest_dir = items[0].1.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dest_display = limit_path_string(&dest_dir, popup_area.width as usize - 10);
    f.render_widget(
        Paragraph::new(format!("to: {}", dest_display)).alignment(Alignment::Center).style(STYLE_FILE),
        popup_area.inner(Margin { vertical: 4, horizontal: 2 }),
    );

    // Instructions
    f.render_widget(
        Paragraph::new("Y / Enter - Yes    N / Esc - No").alignment(Alignment::Center).style(STYLE_COLUMNS),
        popup_area.inner(Margin { vertical: 6, horizontal: 2 }),
    );
}

fn render_large_file_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let Some((file_path, size, is_edit)) = &app_state.large_file else {
        return;
    };
    let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown");
    let verb = if *is_edit { "Edit" } else { "View" };

    let popup_area = centered_rect(60, 30, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Large File ", STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    let message = format!("{} \"{}\" ({})?", verb, name, format_size(*size));
    f.render_widget(
        Paragraph::new(message).alignment(Alignment::Center).style(STYLE_TITLE),
        popup_area.inner(Margin { vertical: 2, horizontal: 2 }),
    );

    f.render_widget(
        Paragraph::new("Reading a file this large may take a while.").alignment(Alignment::Center).style(STYLE_FILE),
        popup_area.inner(Margin { vertical: 4, horizontal: 2 }),
    );

    f.render_widget(
        Paragraph::new("Y / Enter - Yes    N / Esc - No").alignment(Alignment::Center).style(STYLE_COLUMNS),
        popup_area.inner(Margin { vertical: 6, horizontal: 2 }),
    );
}

fn render_editor_save_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let popup_area = centered_rect(60, 25, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Unsaved Changes ", STYLE_TITLE)).centered())
        .borders(Borders::ALL)
        .style(STYLE_BORDER);

    f.render_widget(Clear::default(), popup_area);
    f.render_widget(popup_block, popup_area);

    f.render_widget(
        Paragraph::new("Save changes before closing?").alignment(Alignment::Center).style(STYLE_TITLE),
        popup_area.inner(Margin { vertical: 3, horizontal: 2 }),
    );

    f.render_widget(
        Paragraph::new("Y - Save    N - Discard    Esc - Cancel").alignment(Alignment::Center).style(STYLE_COLUMNS),
        popup_area.inner(Margin { vertical: 5, horizontal: 2 }),
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)])
        .split(popup_layout[1])[1]
}
