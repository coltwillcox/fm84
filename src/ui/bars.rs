//! The bars along the bottom: the status bar, the prompts typed into it, the
//! detail lines and the F-key labels.

use super::*;

fn render_segmented_status_bar(f: &mut ratatui::Frame<'_>, area: Rect, segments: &[&str]) {
    let mut spans = Vec::new();
    spans.push(Span::styled("├─", style_border()));
    let mut used = 3usize; // "├─" (2) + "┤" (1)

    for (i, &seg) in segments.iter().enumerate() {
        let padded = format!(" {} ", seg);
        let needed = display_width(&padded) + usize::from(i > 0);
        // Drop trailing segments that do not fit rather than clip one mid-word
        // and lose the closing corner. The first one always stays.
        if i > 0 && used + needed > area.width as usize {
            break;
        }
        if i > 0 {
            spans.push(Span::styled("─", style_border()));
        }
        used += needed;
        spans.push(Span::styled(padded, style_title()));
    }

    let fill = (area.width as usize).saturating_sub(used);
    spans.push(Span::styled("─".repeat(fill), style_border()));
    spans.push(Span::styled("┤", style_border()));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The find, go-to-line and select prompt, in the status bar's place: the
/// label, what has been typed with the cursor in it, and the border run on to
/// the corner.
fn render_prompt_bar(f: &mut ratatui::Frame<'_>, area: Rect, label: &str, input: &crate::app::TextInput, style: Style) {
    let typed = input.cursor_spans(style_input(), style_input_cursor());
    let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
    let used = 2 + display_width(label) + typed_width + 1 + 1;

    let mut line = vec![Span::styled("├─", style_border()), Span::styled(label.to_string(), style)];
    line.extend(typed);
    line.extend([Span::styled(" ", style), Span::styled("─".repeat((area.width as usize).saturating_sub(used)), style_border()), Span::styled("┤", style_border())]);
    f.render_widget(Paragraph::new(Line::from(line)), area);
}

fn render_status_bar(f: &mut ratatui::Frame<'_>, area: Rect, text: String, style: Style) {
    let text_len = display_width(&text);
    let status_line = vec![
        Span::styled("├─", style_border()),
        Span::styled(text, style),
        Span::styled("─".repeat((area.width as usize).saturating_sub(text_len).saturating_sub(3)), style_border()),
        Span::styled("┤", style_border()),
    ];
    f.render_widget(Paragraph::new(Line::from(status_line)), area);
}

/// Five cells filled in proportion to how full the filesystem is. Both glyphs
/// are Neutral width, so the bar measures the same in every terminal - mixing
/// in an Ambiguous-width glyph like ▓ would double it under a CJK locale.
fn usage_meter(used: u64, total: u64) -> String {
    const CELLS: u64 = 5;
    let filled = used.saturating_mul(CELLS).checked_div(total).unwrap_or(0).min(CELLS);
    "▪".repeat(filled as usize) + &"▫".repeat((CELLS - filled) as usize)
}

/// The widest disk readout that still leaves a dash either side: meter plus
/// figures, then figures alone, then nothing at all.
fn disk_readout(usage: Option<(u64, u64)>, available: usize) -> Option<String> {
    let (used, total) = usage?;
    let figures = format!("{}/{}", format_size(used), format_size(total));
    let options = [format!(" {} {} ", usage_meter(used, total), figures), format!(" {} ", figures)];
    options.into_iter().find(|text| display_width(text) + 2 <= available)
}

pub(super) fn render_bottom_panel(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let status_style = style_title().bg(palette().selected_background);

    if let Some(Dialog::Replacing { .. }) = &app_state.dialog {
        let question = format!(" Replace \"{}\" with \"{}\"?   Y - Yes   N - No   A - All   Esc - Stop ", printable_name(&app_state.find_term), printable_name(&app_state.replace_with));
        render_status_bar(f, area, question, status_style);
        return;
    }
    // Ctrl+L's path is typed over the panel's own, at the top.
    if let Some(Dialog::Prompt(kind, input)) = &app_state.dialog
        && *kind != PromptKind::GoTo
    {
        let label = match kind {
            PromptKind::Find => " Find: ",
            PromptKind::GoToLine => " Go to line: ",
            PromptKind::Select => " Select: ",
            PromptKind::Deselect => " Deselect: ",
            PromptKind::Replace => " Replace: ",
            PromptKind::ReplaceWith => " With: ",
            PromptKind::GoTo => unreachable!("typed in the path bar"),
        };
        render_prompt_bar(f, area, label, input, status_style);
        return;
    }
    if let Some(note) = &app_state.find_note {
        render_status_bar(f, area, format!(" {} ", printable_name(note)), status_style);
        return;
    }

    if let Some(editor_state) = app_state.editor() {
        {
            let filename = shown_name(&editor_state.file_path);
            let modified = if editor_state.modified { " [Modified]" } else { "" };
            let name_seg = format!("{}{}", filename, modified);
            let pos_seg = format!("Ln {}, Col {}", editor_state.cursor_line + 1, editor_state.cursor_col + 1);
            let lines_seg = format!("{} lines", editor_state.lines.len());
            render_segmented_status_bar(f, area, &[&name_seg, &pos_seg, &lines_seg, "F2/Ctrl+S Save", "Ctrl+F Find", "Ctrl+Z/Y Undo", "Esc Exit"]);
        }
    } else if let Screen::Diff(state) = &app_state.screen {
        let at = match (state.current, state.hunks.len()) {
            (Some(current), count) => format!("Difference {}/{}", current + 1, count),
            (None, count) => format!("{count} differences"),
        };
        let rows = format!("Row {}/{}", state.scroll_offset + 1, state.rows.len());
        render_segmented_status_bar(f, area, &[&at, &rows, "F3/N Next", "Shift+F3/P Previous", "Esc Exit"]);
    } else if let Some(viewer_state) = app_state.viewer() {
        {
            let filename = shown_name(&viewer_state.file_path);
            // Lines, not the rows a wrapped one is broken into.
            let line_seg = format!("Line {}/{}", viewer_state.row(viewer_state.scroll_offset).0 + 1, viewer_state.line_count());
            let size_seg = format_size(viewer_state.file_size);
            let zoom_seg = format!("+/- Zoom {}%", viewer_state.image_zoom);
            let mut segments = vec![filename.as_str(), line_seg.as_str(), size_seg.as_str(), viewer_state.syntax_name.as_str()];
            // Bracketed half is the view you are in. Omitted on the notice F4
            // raises for a binary, where there is nothing to toggle.
            if !viewer_state.from_edit {
                segments.push(match (viewer_state.image.is_some(), viewer_state.mode) {
                    (true, ViewMode::Image) => "X [Image] Text Hex",
                    (true, ViewMode::Text) => "X Image [Text] Hex",
                    (true, ViewMode::Hex) => "X Image Text [Hex]",
                    (false, ViewMode::Hex) => "X Text [Hex]",
                    (false, _) => "X [Text] Hex",
                });
                if viewer_state.mode == ViewMode::Image {
                    segments.push(if viewer_state.image_fill { "F Fit [Fill]" } else { "F [Fit] Fill" });
                    segments.push(zoom_seg.as_str());
                } else {
                    if viewer_state.mode == ViewMode::Text {
                        segments.push(if viewer_state.wrap { "W [Wrap]" } else { "W Wrap" });
                    }
                    segments.push("Ctrl+F Find");
                }
            }
            render_segmented_status_bar(f, area, &segments);
        }
    } else if let Some(listing) = if app_state.is_left_active { &app_state.listing_left } else { &app_state.listing_right } {
        // A directory that has not answered yet - a network mount gone quiet.
        // The panel shows what it did until it does.
        let text = format!(" Reading {}…  Esc to stop ", printable_name(&listing.dir.to_string_lossy()));
        render_status_bar(f, area, text, status_style);
    } else if !app_state.search_input.is_empty() {
        // Show search string
        let text = format!(" Search: {} ", app_state.search_input);
        render_status_bar(f, area, text, status_style);
    } else {
        // Show panel stats: selected/total files and selected/total size
        // Returns (count_part, size_part) e.g. ("0/5", "1.2 KiB") or ("2/5", "800 B/1.2 KiB")
        let panel_stat = |children: &[crate::app::Item], selected_set: &std::collections::HashSet<std::ffi::OsString>, current_dir: &PathBuf, dir_sizes: &std::collections::HashMap<PathBuf, u64>| -> (String, String) {
            let item_size = |c: &crate::app::Item| -> u64 { if c.is_dir { dir_sizes.get(&c.path_in(current_dir)).copied().unwrap_or(0) } else { c.size_bytes } };
            let total_count = children.iter().filter(|c| c.name != "..").count();
            let total_size: u64 = children.iter().filter(|c| c.name != "..").map(&item_size).sum();

            if selected_set.is_empty() {
                (format!("0/{}", total_count), format_size(total_size))
            } else {
                let is_selected = |c: &crate::app::Item| c.name != ".." && selected_set.contains(&c.name_os);
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

        let (left_style, right_style) = if app_state.is_left_active { (style_title(), style_dir_dark()) } else { (style_dir_dark(), style_title()) };

        // Disk usage sits at the far end of each panel's dash run, dropping to a
        // shorter form and then out entirely as the terminal narrows.
        let left_disk = disk_readout(app_state.disk_left, left_pad);
        let right_disk = disk_readout(app_state.disk_right, right_pad);
        let dashes = |pad: usize, disk: &Option<String>| pad - disk.as_ref().map_or(0, |text| display_width(text) + 1);

        let mut status_line = vec![
            Span::styled("├─", style_border()),
            Span::styled(format!(" {}", left_count), left_style),
            Span::styled(" - ", style_border()),
            Span::styled(format!("{} ", left_size), left_style),
            Span::styled("─".repeat(dashes(left_pad, &left_disk)), style_border()),
        ];
        if let Some(text) = left_disk {
            status_line.push(Span::styled(text, left_style));
            status_line.push(Span::styled("─", style_border()));
        }
        status_line.extend([
            Span::styled("┴─", style_border()),
            Span::styled(format!(" {}", right_count), right_style),
            Span::styled(" - ", style_border()),
            Span::styled(format!("{} ", right_size), right_style),
            Span::styled("─".repeat(dashes(right_pad, &right_disk)), style_border()),
        ]);
        if let Some(text) = right_disk {
            status_line.push(Span::styled(text, right_style));
            status_line.push(Span::styled("─", style_border()));
        }
        status_line.push(Span::styled("┤", style_border()));

        f.render_widget(Paragraph::new(Line::from(status_line)), area);
    }
}

/// The F-keys the screen showing answers, and what each does there. F8 is
/// named for what F11 has it do, and F3 in the viewer is there only while
/// there is text to search - not over a picture.
fn fkey_labels(app_state: &AppState) -> Vec<(u8, &'static str)> {
    match app_state.screen {
        Screen::Viewer(_) if app_state.can_find() => vec![(3, "Next"), (10, "Quit")],
        Screen::Viewer(_) => vec![(10, "Quit")],
        Screen::Editor(_) => vec![(2, "Save"), (3, "Next"), (4, "Replace"), (10, "Quit")],
        Screen::Diff(_) => vec![(3, "Next"), (10, "Quit")],
        Screen::Panels => vec![
            (1, "Help"),
            (2, "Rename"),
            (3, "View"),
            (4, "Edit"),
            (5, "Copy"),
            (6, "Move"),
            (7, "Mkdir"),
            (8, if app_state.options.delete_to_trash { "Trash" } else { "Delete" }),
            (9, "Terminal"),
            (10, "Quit"),
            (11, "Options"),
            (12, "Preview"),
        ],
    }
}

/// The two rows inside the bottom block, which the F-key labels leave empty.
/// They spell out what the columns cannot hold: the whole name however long,
/// the exact byte count rather than a rounded one, the second on the timestamp,
/// who owns it, and where a symlink points - which is shown nowhere else.
pub(super) fn render_detail(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let Some(detail) = &app_state.cursor_detail else {
        return;
    };
    if area.height < 2 || detail.name.is_empty() {
        return;
    }

    const NAME_MIN: usize = 16;
    let room = area.width as usize;
    let gap = "   ";

    // Every part after the first is preceded by a gap, so its cost is its own
    // width plus that.
    let cost = |parts: &[(&str, Style)]| -> usize { parts.iter().filter(|(text, _)| !text.is_empty()).map(|(text, _)| display_width(text) + gap.len()).sum() };
    let spans = |parts: &[(&str, Style)], lead: bool| -> Vec<Span<'static>> {
        let mut out: Vec<Span<'static>> = Vec::new();
        for (text, style) in parts.iter().filter(|(text, _)| !text.is_empty()) {
            let before = if out.is_empty() && !lead { "" } else { gap };
            out.push(Span::styled(format!("{before}{text}"), *style));
        }
        out
    };

    // The figures go from the end when the row is too narrow, the way the F-key
    // bar drops labels, so what survives is the name rather than a timestamp.
    // What is left of the row goes to the name, shortened from the front to
    // keep its end, where the extension and whatever tells two near-identical
    // names apart both live.
    let mut figures: Vec<(&str, Style)> = vec![(&detail.size, style_file()), (&detail.modified, style_columns())];
    while !figures.is_empty() && NAME_MIN + cost(&figures) > room {
        figures.pop();
    }
    let mut first = vec![Span::styled(tail_of(&detail.name, room.saturating_sub(cost(&figures))), style_title())];
    first.extend(spans(&figures, true));

    let owner: Vec<(&str, Style)> = vec![(&detail.owner, style_columns()), (&detail.attributes, style_file())];
    let mut second = spans(&owner, false);
    if let Some(link) = &detail.link {
        let arrow = format!("{gap}\u{2192} ");
        let left = room.saturating_sub(cost(&owner) + display_width(&arrow));
        if left >= 4 {
            second.push(Span::styled(format!("{arrow}{}", tail_of(link, left)), style_dir()));
        }
    }

    for (offset, row_spans) in [(0, first), (1, second)] {
        let row = Rect::new(area.x, area.y + offset, area.width, 1);
        f.render_widget(Paragraph::new(Line::from(row_spans)), row);
    }
}

/// The last `room` columns of a name, marking the cut. Shortening from the
/// front keeps the end, which is where the extension is and usually where two
/// near-identical names differ.
pub(super) fn tail_of(text: &str, room: usize) -> String {
    let text = &printable_name(text);
    if display_width(text) <= room {
        return text.to_string();
    }
    if room <= 3 {
        return ".".repeat(room);
    }

    let mut kept: Vec<char> = Vec::new();
    let mut used = 3; // the dots standing in for what was cut
    for character in text.chars().rev() {
        let width = display_width(&character.to_string()).max(1);
        if used + width > room {
            break;
        }
        used += width;
        kept.push(character);
    }
    kept.reverse();
    format!("...{}", kept.into_iter().collect::<String>())
}

/// The F-key hints along the bottom border, centred with a dash between
/// each, as titles of the block would be - drawn one by one instead, so a
/// click can be told which it landed on. Those that do not fit whole are
/// dropped from the end: a half-drawn label reads worse than a missing one.
pub(super) fn render_fkey_bar(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) {
    f.render_widget(Block::default().borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT).border_style(style_border()), area);

    // Two corners, then each label with a dash between. The last label's
    // trailing space can fall off the end unnoticed, hence the one spare column.
    let mut labels = Vec::new();
    let mut used = 2;
    for (key, name) in fkey_labels(app_state) {
        let label = format!(" F{key} {name} ");
        let needed = used + display_width(&label) + usize::from(!labels.is_empty());
        if needed > area.width as usize + 1 {
            break;
        }
        used = needed;
        labels.push((key, label));
    }

    app_state.fkey_hits.clear();
    let room = area.width.saturating_sub(2);
    let total = (used - 2) as u16;
    let mut x = area.x + 1 + room.saturating_sub(total) / 2;
    let y = area.bottom().saturating_sub(1);
    for (key, label) in labels {
        let width = (display_width(&label) as u16).min(area.right().saturating_sub(x));
        let cell = Rect::new(x, y, width, 1);
        f.render_widget(Paragraph::new(Span::styled(label, style_title())), cell);
        app_state.fkey_hits.push((cell, key));
        x += width + 1;
    }
}
