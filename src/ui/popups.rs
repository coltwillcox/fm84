//! The popups over the panels, and how each is sized and laid out: one width
//! for all, no taller than the panels.

use super::*;

/// The area inside a popup's border that its body is laid out in.
pub(super) fn popup_inner(area: Rect) -> Rect {
    area.inner(Margin { vertical: 1, horizontal: 2 })
}

/// Lay a popup's lines out inside its border, centred, with a blank row between
/// each while they all still fit.
///
/// Placing them with a widget per line and a deeper margin each time costs two
/// rows per line, and the moment the popup is shorter than that the innermost
/// widget is handed a zero-height area and silently draws nothing. The line
/// that goes first is the last one, which is the one saying which key answers
/// the prompt - so the dialog would ask a question with no way to see the
/// answer. Here the blank rows go before any content does.
///
/// A line wider than the popup is wrapped at its spaces rather than cut off at
/// the border - an error naming a long path, or the names of a dozen files
/// being copied. When that comes to more rows than there is room for, the
/// longest is shortened, so the line naming the keys still shows.
///
/// Gives back the row each line starts on, or None for one past the bottom.
fn popup_body(f: &mut ratatui::Frame<'_>, area: Rect, lines: Vec<Line<'static>>) -> Vec<Option<u16>> {
    let inner = popup_inner(area);
    let room = inner.height as usize;

    let blocks = fit_rows(lines.into_iter().map(|line| wrap_line(line, inner.width as usize)).collect(), room);
    let rows: usize = blocks.iter().map(Vec::len).sum();
    let airy = (rows + blocks.len()).saturating_sub(1);
    let mut body: Vec<Line> = Vec::with_capacity(airy.max(rows));
    let mut starts = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.into_iter().enumerate() {
        if index > 0 && airy <= room {
            body.push(Line::from(""));
        }
        starts.push(body.len());
        body.extend(block);
    }

    // Sit the block in the middle of whatever is left over.
    let padding = room.saturating_sub(body.len()) / 2;
    let mut out = vec![Line::from(""); padding];
    out.extend(body);
    f.render_widget(Paragraph::new(out).alignment(Alignment::Center), inner);
    starts.into_iter().map(|start| (padding + start < room).then(|| inner.y + (padding + start) as u16)).collect()
}

/// The rows a popup line takes at `width`. Only a line in one style is
/// wrapped, which is every long one a popup has; a line built from several -
/// a text field with its cursor - keeps to its own width already.
pub(super) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width || line.spans.len() != 1 {
        return vec![line];
    }
    let style = line.spans[0].style;
    wrap_text(&line.spans[0].content, width)
        .into_iter()
        .map(|row| Line {
            spans: vec![Span::styled(row, style)],
            style: line.style,
            alignment: line.alignment,
        })
        .collect()
}

/// Text broken into rows of at most `width` columns, at the spaces between
/// words, and inside a word only when it is wider than a row by itself.
pub(super) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for word in text.split(' ') {
        if used > 0 && used + 1 + display_width(word) > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        if used > 0 {
            row.push(' ');
            used += 1;
        }
        for character in word.chars() {
            let cell = UnicodeWidthChar::width(character).unwrap_or(0);
            if used > 0 && used + cell > width {
                rows.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(character);
            used += cell;
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

/// Shorten the longest of a popup's lines until they all fit in `room` rows,
/// marking where it was cut. The others - the question, the keys that answer
/// it - are a row or two each, and are what has to be seen.
pub(super) fn fit_rows(mut blocks: Vec<Vec<Line<'static>>>, room: usize) -> Vec<Vec<Line<'static>>> {
    let rows: usize = blocks.iter().map(Vec::len).sum();
    if rows <= room {
        return blocks;
    }
    let Some(longest) = (0..blocks.len()).max_by_key(|&index| blocks[index].len()) else {
        return blocks;
    };
    let others = rows - blocks[longest].len();
    let keep = room.saturating_sub(others).max(1);
    let block = &mut blocks[longest];
    if keep < block.len() {
        block.truncate(keep);
        if let Some(span) = block.last_mut().and_then(|line| line.spans.last_mut()) {
            let mut text = span.content.to_string();
            // Room for the mark: the row was full, or near it.
            text.pop();
            text.push('…');
            span.content = text.into();
        }
    }
    blocks
}

pub(super) fn render_error_popup(f: &mut ratatui::Frame<'_>, area: Rect, message: &str) {
    let popup_area = popup_rect(20, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Error")), style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    popup_body(f, popup_area, vec![Line::from(Span::styled(printable_name(message), style_title()))]);
}

pub(super) fn render_create_popup(f: &mut ratatui::Frame<'_>, area: Rect, is_dir: bool, input: &TextInput) {
    let popup_area = popup_rect(20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", if is_dir { t!("Create Directory") } else { t!("Create File") }), style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // Padded out to the full width so the highlight reads as an input field.
    // Paragraph styles the spans rather than the row, so a bare line would
    // colour only the characters typed so far.
    let typed = input.cursor_spans(style_input(), style_input_cursor());
    let width = popup_inner(popup_area).width as usize;
    let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
    let left = width.saturating_sub(typed_width) / 2;
    let mut spans = vec![Span::raw(" ".repeat(left))];
    spans.extend(typed);
    spans.push(Span::raw(" ".repeat(width.saturating_sub(left + typed_width))));
    let input_line = Line::from(spans).style(style_input());
    popup_body(f, popup_area, vec![input_line, Line::from(Span::styled(t!("Enter - Create    Esc - Cancel"), style_columns()))]);
}

pub(super) fn render_delete_popup(f: &mut ratatui::Frame<'_>, area: Rect, items: &[(PathBuf, bool)], to_trash: bool) {
    let count = items.len();
    let popup_area = popup_rect(30, area);

    // Which kind of delete it is, said plainly: one can be undone and the
    // other cannot.
    let title = match (to_trash, count, count == 1 && items[0].1) {
        (true, 1, true) => t!("Move to trash: directory").to_string(),
        (true, 1, false) => t!("Move to trash: file").to_string(),
        (false, 1, true) => t!("Delete: directory").to_string(),
        (false, 1, false) => t!("Delete: file").to_string(),
        (true, count, _) => tn!(count, "Move to trash: {count} item", "Move to trash: {count} items", count),
        (false, count, _) => tn!(count, "Delete: {count} item", "Delete: {count} items", count),
    };
    let title = format!(" {title} ");

    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let mut lines = Vec::new();
    if count == 1 {
        let name = shown_name(&items[0].0);
        let question = if to_trash { t!("Move \"{name}\" to the trash?", name) } else { t!("Delete \"{name}\" permanently?", name) };
        lines.push(Line::from(Span::styled(question, style_title())));
    } else {
        let names: Vec<String> = items.iter().map(|(path, _)| shown_name(path)).collect();
        let question = if to_trash {
            tn!(count, "Move {count} item to the trash?", "Move {count} items to the trash?", count)
        } else {
            tn!(count, "Delete {count} item permanently?", "Delete {count} items permanently?", count)
        };
        lines.push(Line::from(Span::styled(question, style_title())));
        lines.push(Line::from(Span::styled(names.join(", "), style_file())));
    }
    if !to_trash {
        lines.push(Line::from(Span::styled(t!("This cannot be undone"), style_file())));
    }
    lines.push(Line::from(Span::styled(t!("Y / Enter - Yes    N / Esc - No"), style_columns())));
    popup_body(f, popup_area, lines);
}

/// A file in the way of one being copied or moved: both of them, size and
/// date, so which to keep can be weighed, and the answers.
fn render_job_clash(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, clash: &crate::app::JobClash) {
    let popup_area = popup_rect(40, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Already there")), style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let folder = clash.dest.parent().map(|parent| printable_name(&parent.to_string_lossy())).unwrap_or_default();
    // Size and date, and which of the two is the newer - what Overwrite older
    // goes by.
    // Rounded as the panels round them, unless that makes two different
    // sizes read the same - then to the byte, since telling them apart is
    // what they are shown for.
    let exact = clash.source_size != clash.dest_size && clash.source_size.map(format_size) == clash.dest_size.map(format_size);
    let width = if exact { 15 } else { 9 };
    // The labels as wide as the wider of them, and "newer" as wide as it
    // is in the language shown, so the columns line up either way.
    let (new_label, existing_label, newer_label) = (t!("New"), t!("Existing"), t!("newer"));
    let label_width = display_width(new_label).max(display_width(existing_label));
    let describe = |label: &str, size: Option<u64>, modified: Option<std::time::SystemTime>, other: Option<std::time::SystemTime>| {
        let label = format!("{label}{}", " ".repeat(label_width.saturating_sub(display_width(label))));
        let size = size.map(|bytes| if exact { format!("{} B", grouped(bytes)) } else { format_size(bytes) }).unwrap_or_else(|| "?".to_string());
        let date = modified.map(|at| format_exact(at, app_state.options.date_format)).unwrap_or_else(|| "?".to_string());
        let newer = matches!((modified, other), (Some(this), Some(that)) if this > that);
        let newer = if newer { format!("  {newer_label}") } else { " ".repeat(display_width(newer_label) + 2) };
        format!("{label}  {size:>width$}  {date}{newer}")
    };
    let mut lines = vec![Line::from(Span::styled(t!("\"{name}\" is already in {folder}", name = shown_name(&clash.dest), folder), style_title()))];
    // Copied under another name, the new one is not the name in the question.
    if clash.source.file_name() != clash.dest.file_name() {
        lines.push(Line::from(Span::styled(t!("to be replaced by \"{name}\"", name = shown_name(&clash.source)), style_file())));
    }
    lines.extend([
        Line::from(Span::styled(describe(new_label, clash.source_size, clash.source_modified, clash.dest_modified), style_file())),
        Line::from(Span::styled(describe(existing_label, clash.dest_size, clash.dest_modified, clash.source_modified), style_file())),
        Line::from(Span::styled(t!("Y - Overwrite    N - Skip    K - Keep both"), style_columns())),
        Line::from(Span::styled(t!("A - Overwrite all    S - Skip all    B - Keep both all"), style_columns())),
        Line::from(Span::styled(t!("O - Overwrite all older    Esc - Abort"), style_columns())),
    ]);
    if app_state.quit_armed {
        lines.push(Line::from(Span::styled(t!("F10 again - Quit"), style_file())));
    }
    popup_body(f, popup_area, lines);
}

/// A bar of `width` cells filled in proportion to `fraction`. Same two glyphs
/// as the disk meter, both Neutral width - an Ambiguous-width glyph would
/// measure double under a CJK locale and push the bar out of the popup.
fn progress_bar(fraction: f64, width: usize) -> String {
    let filled = ((fraction.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    "\u{25aa}".repeat(filled) + &"\u{25ab}".repeat(width - filled)
}

/// How a copy, move or delete is getting on. Held back for a moment after the
/// job starts, so the many that finish at once never flash a popup on the way
/// past.
pub(super) fn render_transfer_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let Some(job) = &app_state.workers.job else {
        return;
    };
    if let Some(clash) = &job.clash {
        render_job_clash(f, area, app_state, clash);
        return;
    }
    if let Some(problem) = &job.problem {
        render_job_problem(f, area, job.kind, problem, app_state.quit_armed);
        return;
    }
    if let Some(short) = &job.room {
        render_job_room(f, area, short, app_state.quit_armed);
        return;
    }
    let elapsed = job.started.elapsed();
    if elapsed < TRANSFER_POPUP_DELAY {
        return;
    }

    let title = if job.is_cancelling() {
        t!("Cancelling")
    } else {
        match job.kind {
            TransferKind::Copy => t!("Copying"),
            TransferKind::Move => t!("Moving"),
            TransferKind::Delete => t!("Deleting"),
            TransferKind::Trash => t!("Moving to trash"),
            TransferKind::Extract => t!("Extracting"),
            TransferKind::Pack => t!("Packing"),
            TransferKind::Checksum => t!("Checksumming"),
            TransferKind::Sync => t!("Synchronizing"),
        }
    };
    let title = format!(" {title} ");
    let popup_area = popup_rect(30, area);
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let name = shown_name(&job.current);

    // The counting pass runs before any bytes move, so there is a moment at the
    // start with nothing to measure against, and a rename has nothing to count.
    let bar_width = popup_area.width.saturating_sub(14) as usize;
    let bar = match job.fraction() {
        Some(fraction) => format!("{} {:>3.0}%", progress_bar(fraction, bar_width), fraction * 100.0),
        None => t!("Counting...").to_string(),
    };

    // Once F10 has been pressed the offer stays up for the rest of the job, so
    // there is always something on screen saying how to get out of one that is
    // not going to finish.
    let hint = if app_state.quit_armed { t!("Esc - Cancel    F10 again - Quit") } else { t!("Esc - Cancel") };

    // A copy moves bytes and a delete removes entries, so each is counted in
    // what it actually does rather than forcing both into the same figure.
    let detail = match (job.kind, job.total) {
        (TransferKind::Delete, Some(total)) if total > 0 => {
            format!("{}    {hint}", tn!(total, "{done} of {total} entry", "{done} of {total} entries", done = job.done, total))
        }
        (TransferKind::Trash, Some(total)) if total > 0 => {
            format!("{}    {hint}", tn!(total, "{done} of {total} item", "{done} of {total} items", done = job.done, total))
        }
        (_, Some(total)) if total > 0 => {
            let rate = job.done as f64 / elapsed.as_secs_f64().max(0.001);
            format!("{}    {hint}", t!("{done} of {total} at {rate}/s", done = format_size(job.done), total = format_size(total), rate = format_size(rate as u64)))
        }
        _ => hint.to_string(),
    };

    // One paragraph rather than a widget per line: stacking margins to place
    // them costs two rows each, which silently leaves nothing to draw in once
    // the popup is short.
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(name.to_string(), style_title())),
        Line::from(""),
        Line::from(Span::styled(bar, style_columns())),
        Line::from(""),
        Line::from(Span::styled(detail, style_file())),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), popup_area.inner(Margin { vertical: 1, horizontal: 2 }));
}

/// An entry a copy, move or delete could not deal with. Shown at once,
/// whatever the delay on the progress popup: the job is stopped until it is
/// answered.
fn render_job_problem(f: &mut ratatui::Frame<'_>, area: Rect, kind: TransferKind, problem: &crate::app::JobProblem, quit_armed: bool) {
    let title = match kind {
        TransferKind::Copy => t!("Cannot copy"),
        TransferKind::Move => t!("Cannot move"),
        TransferKind::Delete => t!("Cannot delete"),
        TransferKind::Trash => t!("Cannot move to trash"),
        TransferKind::Extract => t!("Cannot extract"),
        TransferKind::Pack => t!("Cannot pack"),
        TransferKind::Checksum => t!("Cannot read"),
        TransferKind::Sync => t!("Cannot synchronize"),
    };
    let title = format!(" {title} ");
    let popup_area = popup_rect(30, area);
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let hint = t!("R - Retry    S - Skip    A - Skip all    Esc - Abort");
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(shown_name(&problem.path), style_title())),
        Line::from(""),
        Line::from(Span::styled(printable_name(&problem.message), style_columns())),
        Line::from(""),
        Line::from(Span::styled(hint, style_file())),
    ];
    // What the trash will not take can still be deleted for good, on purpose.
    if kind == TransferKind::Trash {
        lines.push(Line::from(Span::styled(t!("D - Delete permanently"), style_file())));
    }
    if quit_armed {
        lines.push(Line::from(Span::styled(t!("F10 again - Quit"), style_file())));
    }
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).wrap(ratatui::widgets::Wrap { trim: true }), popup_area.inner(Margin { vertical: 1, horizontal: 2 }));
}

/// A filesystem without the room for what a copy or move is about to write,
/// asked about before anything is. Shown at once: the job waits on it.
fn render_job_room(f: &mut ratatui::Frame<'_>, area: Rect, short: &crate::fs_ops::Shortfall, quit_armed: bool) {
    let title = format!(" {} ", t!("Not enough room"));
    let popup_area = popup_rect(30, area);
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(printable_name(&crate::app::room_needed(short)), style_columns())),
        Line::from(""),
        Line::from(Span::styled(t!("R - Check again    C - Go on anyway    Esc - Cancel"), style_file())),
    ];
    if quit_armed {
        lines.push(Line::from(Span::styled(t!("F10 again - Quit"), style_file())));
    }
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).wrap(ratatui::widgets::Wrap { trim: true }), popup_area.inner(Margin { vertical: 1, horizontal: 2 }));
}

/// Asks before a save writes over what something else has written since the
/// editor read the file: the file as it was then and as it is now, and the
/// answers. A file that has gone is said to have, and cannot be reloaded.
pub(super) fn render_disk_changed_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, opened: Option<crate::app::DiskStamp>, now: Option<crate::app::DiskStamp>) {
    let popup_area = popup_rect(40, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Changed on disk")), style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let name = app_state.editor().map(|state| shown_name(&state.file_path)).unwrap_or_default();
    let (when_opened, now_label) = (t!("When opened"), t!("Now"));
    let label_width = display_width(when_opened).max(display_width(now_label)) + 1;
    let stamp = |label: &str, stamp: Option<crate::app::DiskStamp>| {
        let shown = match stamp {
            Some(stamp) => {
                let date = stamp.modified.map(|at| format_exact(at, app_state.options.date_format)).unwrap_or_else(|| "?".to_string());
                format!("{:>9}  {date}", format_size(stamp.size))
            }
            None => t!("not there").to_string(),
        };
        let pad = " ".repeat(label_width.saturating_sub(display_width(label)));
        Line::from(Span::styled(format!("{label}{pad}{shown}"), style_file()))
    };
    let mut lines = match now {
        Some(_) => vec![Line::from(Span::styled(t!("\"{name}\" has changed since it was opened", name), style_title()))],
        None => vec![Line::from(Span::styled(t!("\"{name}\" is no longer there", name), style_title()))],
    };
    lines.push(stamp(when_opened, opened));
    lines.push(stamp(now_label, now));
    let keys = match now {
        Some(_) => t!("O - Overwrite it    R - Reload, losing your edits"),
        None => t!("O - Save it again"),
    };
    lines.push(Line::from(Span::styled(keys, style_columns())));
    lines.push(Line::from(Span::styled(t!("Esc - Back to editing"), style_columns())));
    popup_body(f, popup_area, lines);
}

/// Alt+F7: where the search starts, the fields, and what goes in them.
pub(super) fn render_find_files_form(f: &mut ratatui::Frame<'_>, area: Rect, start: &Path, form: &crate::app::FindForm) {
    let popup_area = popup_rect(45, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Find files")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let width = popup_inner(popup_area).width as usize;
    let labels = crate::app::find_fields();
    let (in_label, regex_label) = (t!("In"), t!("Regex"));
    // As wide as the widest label, in whichever language, and a gap.
    let label_width = labels.iter().chain([&in_label, &regex_label]).map(|label| display_width(label)).max().unwrap_or(0) + 2;
    let label = |label: &str| format!("{label}{}", " ".repeat(label_width.saturating_sub(display_width(label))));
    // The field the keys go to has the cursor in it and a bar behind it; the
    // other shows what it holds.
    let field = |name: &str, input: &TextInput, focused: bool| {
        let room = width.saturating_sub(label_width);
        let mut spans = vec![Span::styled(label(name), style_columns())];
        let typed = if focused {
            input.cursor_spans_within(room, style_input(), style_input_cursor())
        } else {
            vec![Span::styled(tail_of(&input.text, room), style_file())]
        };
        let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
        spans.extend(typed);
        // The bar is the field's alone; the label beside it keeps its colour
        // on the background it was picked for.
        let padding = " ".repeat(room.saturating_sub(typed_width));
        spans.push(if focused { Span::styled(padding, style_input()) } else { Span::raw(padding) });
        Line::from(spans)
    };
    popup_body(
        f,
        popup_area,
        vec![
            // Padded out like the fields, so the labels line up.
            Line::from(Span::styled(
                format!("{}{:<room$}", label(in_label), tail_of(&start.to_string_lossy(), width.saturating_sub(label_width)), room = width.saturating_sub(label_width)),
                style_file(),
            )),
            field(labels[0], &form.fields[0], form.focus == 0),
            field(labels[1], &form.fields[1], form.focus == 1),
            field(labels[2], &form.fields[2], form.focus == 2),
            field(labels[3], &form.fields[3], form.focus == 3),
            {
                let focused = form.focus == crate::app::FIND_REGEX_ROW;
                let check = format!("[{}] {}", if form.regex { "x" } else { " " }, t!("Containing is a regular expression"));
                let room = width.saturating_sub(label_width);
                let check = format!("{:<room$}", tail_of(&check, room));
                Line::from(vec![Span::styled(label(regex_label), style_columns()), Span::styled(check, if focused { style_input() } else { style_file() })])
            },
            Line::from(Span::styled(t!("Name as + takes it: *.jpg;*.png, or */ for directories. Lower case matches either case"), style_columns())),
            Line::from(Span::styled(t!("Size as >10M, <1K or 1M..10M; Modified as <7d, >1y, 1d..7d, 2026-10-01 or >2026-10-01"), style_columns())),
            Line::from(Span::styled(t!("Tab - Field  Space - Regex  Enter - Find  Esc - Cancel"), style_columns())),
        ],
    );
}

/// The Find files results: how the search is getting on, and what it has
/// found, shown from where it started, with the cursor on one. The number of
/// results that fit, for PageUp and PageDown.
pub(super) fn render_find_files_results(f: &mut ratatui::Frame<'_>, area: Rect, search: &crate::app::Search) -> usize {
    let popup_area = popup_rect(80, area);
    let title = format!(" {} {} ", t!("Find:"), printable_name(&search.label));
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // A row for how it is going, one blank under it, and one for the keys.
    let rows = (inner.height as usize).saturating_sub(3);

    let found = search.results.len();
    let status = match &search.looking_in {
        Some(dir) => {
            let said = format!("{} ", tn!(found, "{found} found, searching", "{found} found, searching", found));
            format!("{said}{}", tail_of(&dir.to_string_lossy(), width.saturating_sub(display_width(&said) + 1)))
        }
        None if found == 0 => t!("Nothing found").to_string(),
        None => tn!(found, "{found} found", "{found} found", found),
    };
    let status = match search.unreadable {
        0 => status,
        count => format!("{status} - {}", tn!(count, "{count} directory could not be read", "{count} directories could not be read", count)),
    };

    let mut lines = vec![Line::from(Span::styled(status, style_title())), Line::from("")];
    // The scroll is kept where the cursor last pushed it, unless the list has
    // since grown taller and could show more - and the cursor is always on
    // it, however the terminal was resized.
    let mut first = search.scroll.min(found.saturating_sub(rows)).min(search.cursor);
    if rows > 0 && search.cursor >= first + rows {
        first = search.cursor + 1 - rows;
    }
    for (index, (path, is_dir)) in search.results.iter().enumerate().skip(first).take(rows) {
        let shown = path.strip_prefix(&search.start).unwrap_or(path).to_string_lossy().into_owned();
        let shown = if *is_dir { format!("{shown}{}", std::path::MAIN_SEPARATOR) } else { shown };
        // Shortened from the front, which keeps the name.
        let text = tail_of(&shown, width);
        let pad = " ".repeat(width.saturating_sub(display_width(&text)));
        let style = match (index == search.cursor, *is_dir) {
            (true, _) => style_title().bg(palette().selected_background),
            (false, true) => style_dir(),
            (false, false) => style_file(),
        };
        lines.push(Line::from(Span::styled(format!("{text}{pad}"), style)));
    }
    while lines.len() < rows + 2 {
        lines.push(Line::from(""));
    }
    let hint = if search.done {
        t!("Enter - Go to    F - Feed to panel    Esc - Close")
    } else {
        t!("Enter - Go to    F - Feed to panel    Esc - Stop and close")
    };
    lines.push(Line::from(Span::styled(hint, style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
    rows
}

/// Alt+A: a field for each thing to change, the one being typed into barred
/// as Find files' are. With one entry, the mode as it would come out is
/// written beside its field.
pub(super) fn render_attributes_popup(f: &mut ratatui::Frame<'_>, area: Rect, form: &AttributesForm) -> Vec<(Rect, u16)> {
    let popup_area = popup_rect(50, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Attributes")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let width = popup_inner(popup_area).width as usize;
    const LABEL: usize = 10;
    // Room kept after the mode for what it comes to.
    const BECOMES: usize = 12;
    let what = match form.items.as_slice() {
        [(path, false)] => t!("File \"{name}\"", name = shown_name(path)),
        [(path, true)] => t!("Directory \"{name}\"", name = shown_name(path)),
        items => tn!(
            items.len(),
            "{count} item - a field left as it is, or empty, changes nothing",
            "{count} items - a field left as it is, or empty, changes nothing",
            count = items.len()
        ),
    };
    let mut lines = vec![Line::from(Span::styled(tail_of(&what, width), style_file()))];
    for (index, field) in form.fields.iter().enumerate() {
        let focused = index == form.focus;
        let becomes = match (field.kind, form.items.as_slice(), form.modes.first()) {
            (AttributeKind::Mode, [(_, is_dir)], Some(&mode)) => crate::attributes::ModeChange::parse(&field.input.text).ok().map(|change| mode_preview(change.apply(mode, *is_dir))),
            _ => None,
        };
        let room = width.saturating_sub(LABEL + if field.kind == AttributeKind::Mode { BECOMES } else { 0 });
        let mut spans = vec![Span::styled(format!("{:<LABEL$}", field.kind.label()), style_columns())];
        let typed = if focused {
            field.input.cursor_spans_within(room, style_input(), style_input_cursor())
        } else {
            vec![Span::styled(tail_of(&field.input.text, room), style_file())]
        };
        let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
        spans.extend(typed);
        let padding = " ".repeat(room.saturating_sub(typed_width));
        spans.push(if focused { Span::styled(padding, style_input()) } else { Span::raw(padding) });
        // Lines are centred, so the mode's row is kept as wide as the rest.
        if field.kind == AttributeKind::Mode {
            spans.push(Span::styled(format!("{:>BECOMES$}", becomes.unwrap_or_default()), style_file()));
        }
        lines.push(Line::from(spans));
    }
    if form.fields.len() > 1 {
        lines.push(Line::from(Span::styled(t!("Mode as 755 or u+x,go-w; owner and group by name or number"), style_columns())));
    }
    lines.push(Line::from(Span::styled(t!("Tab - Next field    Enter - Apply    Esc - Cancel"), style_columns())));
    // Each field's row, for a click: the lines are as wide as the body, so
    // the text starts just after the label.
    let inner = popup_inner(popup_area);
    let starts = popup_body(f, popup_area, lines);
    starts[1..=form.fields.len()].iter().flatten().map(|&y| (Rect::new(inner.x, y, inner.width, 1), inner.x + LABEL as u16)).collect()
}

#[cfg(unix)]
fn mode_preview(mode: u32) -> String {
    crate::fs_ops::mode_string(mode)
}

#[cfg(not(unix))]
fn mode_preview(_mode: u32) -> String {
    String::new()
}

/// Ctrl+D: the bookmarks, numbered for 1 to 9, the active panel's
/// directory marked when it is one of them. Each is shortened from the front,
/// keeping the end of it, which tells them apart.
pub(super) fn render_bookmarks_popup(f: &mut ratatui::Frame<'_>, area: Rect, list: &[PathBuf], index: usize, here: &Path) {
    let popup_area = popup_rect(60, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Bookmarks")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // One blank row and two of keys under the list.
    let rows = (inner.height as usize).saturating_sub(3);

    let mut lines = Vec::new();
    if list.is_empty() {
        lines.push(Line::from(Span::styled(t!("No bookmarks yet - A keeps the panel's directory"), style_file())));
    }
    let first = if rows > 0 && index >= rows { index + 1 - rows } else { 0 };
    for (number, path) in list.iter().enumerate().skip(first).take(rows) {
        let shown = from_home(path);
        let label = if number < 9 { format!("{} ", number + 1) } else { "  ".to_string() };
        let mark = if path == here { "• " } else { "  " };
        let room = width.saturating_sub(label.len() + mark.len());
        let text = format!("{label}{mark}{}", tail_of(&printable_name(&shown), room));
        let pad = " ".repeat(width.saturating_sub(display_width(&text)));
        let style = if number == index { style_title().bg(palette().selected_background) } else { style_dir() };
        lines.push(Line::from(Span::styled(format!("{text}{pad}"), style)));
    }
    while lines.len() < rows + 1 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(t!("Enter/1-9 - Go    A/Ins - Add here    D/Del - Remove"), style_columns())).centered());
    lines.push(Line::from(Span::styled(t!("Shift+Up/Down - Move    Esc - Close"), style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
}

/// Alt+M: the user's commands, each with what it runs beside it, numbered as
/// the bookmarks are - laid out as they are, too.
pub(super) fn render_user_menu_popup(f: &mut ratatui::Frame<'_>, area: Rect, menu: &crate::app::UserMenu) {
    let popup_area = popup_rect(60, area);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("User Menu")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // One blank row and one of keys under the list.
    let rows = (inner.height as usize).saturating_sub(2);

    let mut lines = Vec::new();
    if menu.entries.is_empty() {
        lines.push(Line::from(Span::styled(t!("No commands yet - E adds them, as label = command, in"), style_file())));
        if let Some(file) = &menu.file {
            lines.push(Line::from(Span::styled(tail_of(&from_home(file), width), style_dir())));
        }
    }
    // The labels in a column as wide as the longest, the commands after.
    let label_width = menu.entries.iter().map(|entry| display_width(&printable_name(&entry.label))).max().unwrap_or(0).min(width / 2);
    let first = if rows > 0 && menu.index >= rows { menu.index + 1 - rows } else { 0 };
    for (number, entry) in menu.entries.iter().enumerate().skip(first).take(rows) {
        let key = if number < 9 { format!("{} ", number + 1) } else { "  ".to_string() };
        let label = cut_to(&printable_name(&entry.label), label_width);
        let label = format!("{label}{}", " ".repeat(label_width.saturating_sub(display_width(&label))));
        let room = width.saturating_sub(key.len() + label_width + 3);
        let command = cut_to(&printable_name(&entry.command), room);
        let used = key.len() + label_width + 3 + display_width(&command);
        let pad = " ".repeat(width.saturating_sub(used));
        let (style, quiet) = if number == menu.index {
            (style_title().bg(palette().selected_background), style_columns().bg(palette().selected_background))
        } else {
            (style_dir(), style_columns())
        };
        lines.push(Line::from(vec![Span::styled(format!("{key}{label}   "), style), Span::styled(format!("{command}{pad}"), quiet)]));
    }
    while lines.len() < rows + 1 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(t!("Enter/1-9 - Run    E - Edit the menu    Esc - Close"), style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
}

/// Alt+O: the programs the file can be opened with, named in a column with
/// what each runs beside it - laid out as the user menu is.
pub(super) fn render_open_with_popup(f: &mut ratatui::Frame<'_>, area: Rect, menu: &crate::app::OpenWith) {
    let popup_area = popup_rect(60, area);
    let name = menu.path.file_name().map(|name| printable_name(&name.to_string_lossy())).unwrap_or_default();
    let title = format!(" {} ", t!("Open {name} With", name = cut_to(&name, (popup_area.width as usize).saturating_sub(16))));
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    let rows = (inner.height as usize).saturating_sub(2);

    let mut lines = Vec::new();
    let label_width = menu.openers.iter().map(|opener| display_width(&printable_name(&opener.label))).max().unwrap_or(0).min(width / 2);
    let first = if rows > 0 && menu.index >= rows { menu.index + 1 - rows } else { 0 };
    for (number, opener) in menu.openers.iter().enumerate().skip(first).take(rows) {
        let key = if number < 9 { format!("{} ", number + 1) } else { "  ".to_string() };
        let label = cut_to(&printable_name(&opener.label), label_width);
        let label = format!("{label}{}", " ".repeat(label_width.saturating_sub(display_width(&label))));
        let room = width.saturating_sub(key.len() + label_width + 3);
        let detail = if opener.terminal { format!("[{}] {}", t!("terminal"), opener.detail) } else { opener.detail.clone() };
        let detail = cut_to(&printable_name(&detail), room);
        let used = key.len() + label_width + 3 + display_width(&detail);
        let pad = " ".repeat(width.saturating_sub(used));
        let (style, quiet) = if number == menu.index {
            (style_title().bg(palette().selected_background), style_columns().bg(palette().selected_background))
        } else {
            (style_dir(), style_columns())
        };
        lines.push(Line::from(vec![Span::styled(format!("{key}{label}   "), style), Span::styled(format!("{detail}{pad}"), quiet)]));
    }
    while lines.len() < rows + 1 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(t!("Enter/1-9 - Open    E - Edit associations    Esc - Close"), style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
}

/// F2 with a selection: the pattern, find and replace and case, and under
/// them every old name beside its new one, with what is wrong with any that
/// cannot be had. The table's rows are kept for Page Up and Page Down.
/// Returns the rows the table had room for, and its scroll kept to them.
pub(super) fn render_multi_rename_popup(f: &mut ratatui::Frame<'_>, area: Rect, rename: &crate::app::MultiRename) -> (usize, usize) {
    let popup_area = popup_rect(80, area);
    let count = rename.entries.len();
    let title = format!(" {} ", tn!(count, "Rename {count} Entry", "Rename {count} Entries", count));
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    let labels = [t!("Name"), t!("Find"), t!("Replace"), t!("Case")];
    let label_width = labels.iter().map(|label| display_width(label)).max().unwrap_or(0) + 2;
    let label = |label: &str| format!("{label}{}", " ".repeat(label_width.saturating_sub(display_width(label))));
    let room = width.saturating_sub(label_width);

    let mut lines = Vec::new();
    for (index, name) in labels[..3].iter().enumerate() {
        let input = &rename.fields[index];
        let mut spans = vec![Span::styled(label(name), style_columns())];
        let focused = rename.focus == index;
        let typed = if focused {
            input.cursor_spans_within(room, style_input(), style_input_cursor())
        } else {
            vec![Span::styled(tail_of(&input.text, room), style_file())]
        };
        let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
        spans.extend(typed);
        let padding = " ".repeat(room.saturating_sub(typed_width));
        spans.push(if focused { Span::styled(padding, style_input()) } else { Span::raw(padding) });
        lines.push(Line::from(spans));
    }
    let case = format!("◂ {} ▸", rename.case.label());
    let case_style = if rename.focus == crate::app::CASE_ROW { style_input() } else { style_file() };
    lines.push(Line::from(vec![Span::styled(label(labels[3]), style_columns()), Span::styled(case, case_style)]));
    for hint in [t!("[N] name   [E] extension   [C] counter"), t!("[N2-5] letters 2 to 5   [C10+5:3] from 10, by 5, 3 digits")] {
        lines.push(Line::from(Span::styled(cut_to(hint, width), style_columns())));
    }
    lines.push(Line::from(""));

    // Old names on the left, new on the right, each half the width.
    let half = width.saturating_sub(3) / 2;
    let pad = |text: &str, to: usize| format!("{text}{}", " ".repeat(to.saturating_sub(display_width(text))));
    lines.push(Line::from(vec![Span::styled(pad(t!("Old name"), half), style_title()), Span::styled(format!("   {}", t!("New name")), style_title())]));
    // Above the table, and the blank row and keys below it.
    let rows = (inner.height as usize).saturating_sub(lines.len() + 2);
    let room = rows.max(1);
    let scroll = rename.scroll.min(rename.entries.len().saturating_sub(room));
    let preview = rename.preview();
    for (entry, (new, problem)) in rename.entries.iter().zip(&preview).skip(scroll).take(rows) {
        let old = cut_to(&printable_name(&entry.old), half);
        let mut spans = vec![Span::styled(pad(&old, half), style_dir()), Span::styled(" → ", style_columns())];
        match problem {
            Some(problem) => {
                let note = format!("  {}", problem.label());
                let new = cut_to(&printable_name(new), half.saturating_sub(display_width(&note)));
                spans.push(Span::styled(new, Style::new().fg(palette().selected_marker)));
                spans.push(Span::styled(note, Style::new().fg(palette().selected_marker).add_modifier(Modifier::BOLD)));
            }
            None if *new == entry.old => spans.push(Span::styled(cut_to(&printable_name(new), half), style_columns())),
            None => spans.push(Span::styled(cut_to(&printable_name(new), half), style_file())),
        }
        lines.push(Line::from(spans));
    }
    let problems = preview.iter().filter(|(_, problem)| problem.is_some()).count();
    while lines.len() < (inner.height as usize).saturating_sub(1) {
        lines.push(Line::from(""));
    }
    let keys = if problems > 0 {
        let said = tn!(problems, "{problems} cannot be renamed as it stands    Esc - Cancel", "{problems} cannot be renamed as they stand    Esc - Cancel", problems);
        Line::from(Span::styled(cut_to(&said, width), Style::new().fg(palette().selected_marker))).centered()
    } else {
        Line::from(Span::styled(cut_to(t!("Tab - Field   ←/→ - Case   Enter - Rename   Esc - Cancel"), width), style_columns())).centered()
    };
    lines.push(keys);
    f.render_widget(Paragraph::new(lines), inner);
    (room, scroll)
}

/// Alt+S: the plan. Each entry that differs by its path from both
/// directories, what each side has of it - its size and age - and between
/// them what is to happen: copied one way or the other, deleted from the
/// side that has it, or left. The newer side of two files is the brighter.
/// Returns the rows the list had room for, and its scroll kept to them and to
/// the cursor.
pub(super) fn render_sync_popup(f: &mut ratatui::Frame<'_>, area: Rect, plan: &crate::app::SyncPlan, date_format: crate::options::DateFormat) -> (usize, usize) {
    use crate::app::SyncAction;
    // As wide as the panels: a path and both sides' sizes and ages to fit.
    let panels = screen_layout(area)[2];
    let popup_area = centered(panels, (panels.width as usize).saturating_sub(4).max(1), popup_rect(80, area).height as usize);
    let popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Synchronize")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    let half = width.saturating_sub(5) / 2;
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{:<half$}", tail_of(&plan.left.to_string_lossy(), half)), style_dir()),
        Span::styled("     ", style_columns()),
        Span::styled(tail_of(&plan.right.to_string_lossy(), half), style_dir()),
    ])];
    let found = plan.entries.len();
    let mut status = match &plan.looking_in {
        Some(dir) => {
            let said = format!("{} ", tn!(found, "{found} differs, comparing", "{found} differ, comparing", found));
            let dir = if dir.as_os_str().is_empty() { ".".to_string() } else { dir.to_string_lossy().into_owned() };
            format!("{said}{}", tail_of(&dir, width.saturating_sub(display_width(&said))))
        }
        None if found == 0 => t!("The directories hold the same files").to_string(),
        None => tn!(found, "{found} differs", "{found} differ", found),
    };
    match plan.unreadable {
        0 => {}
        count => status.push_str(&format!(" - {}", tn!(count, "{count} directory could not be read", "{count} directories could not be read", count))),
    }
    lines.push(Line::from(Span::styled(status, style_title())));
    lines.push(Line::from(""));

    // Each side: its size and age, in the panels' date format.
    let now = std::time::SystemTime::now();
    // The ages go when the path would be left too little room.
    let dated = width >= 2 * (9 + 1 + 16) + 7 + 16;
    let side_width = if dated { 9 + 1 + 16 } else { 9 };
    let path_width = width.saturating_sub(2 * side_width + 7);
    let side = |side: Option<crate::sync::Entry>| match side {
        None => format!("{:side_width$}", ""),
        Some(side) => {
            let size = if side.is_dir { "<DIR>".to_string() } else { crate::utils::format_size(side.size) };
            if !dated {
                return format!("{:>9}", cut_to(&size, 9));
            }
            let date = side.modified.map(|time| crate::utils::format_modified(time, date_format, now)).unwrap_or_default();
            format!("{:>9} {:<16}", cut_to(&size, 9), cut_to(&date, 16))
        }
    };
    // Three columns, whatever the language calls a delete.
    let del = format!("{:<3}", cut_to(t!("del"), 3));
    let (delete_left, delete_right) = (format!(" ✗ {del} "), format!(" {del} ✗ "));
    let rows = (inner.height as usize).saturating_sub(lines.len() + 3);
    let room = rows.max(1);
    let mut scroll = plan.scroll.min(plan.entries.len().saturating_sub(room)).min(plan.cursor);
    if rows > 0 && plan.cursor >= scroll + rows {
        scroll = plan.cursor + 1 - rows;
    }
    for (index, entry) in plan.entries.iter().enumerate().skip(scroll).take(rows) {
        let difference = &entry.difference;
        let mut path = difference.path.to_string_lossy().into_owned();
        if entry.is_dir() {
            path.push(std::path::MAIN_SEPARATOR);
        }
        let path = tail_of(&printable_name(&path), path_width);
        let (arrow, arrow_style) = match entry.action {
            SyncAction::ToRight => ("  ──▶  ", style_dir()),
            SyncAction::ToLeft => ("  ◀──  ", style_dir()),
            SyncAction::DeleteLeft => (delete_left.as_str(), Style::new().fg(palette().selected_marker).add_modifier(Modifier::BOLD)),
            SyncAction::DeleteRight => (delete_right.as_str(), Style::new().fg(palette().selected_marker).add_modifier(Modifier::BOLD)),
            SyncAction::Skip if difference.left.is_some() && difference.right.is_some() => ("   ≠   ", style_columns()),
            SyncAction::Skip => ("       ", style_columns()),
        };
        let newer = difference.newer();
        let side_style = |is_newer: bool| if is_newer { style_file() } else { style_columns() };
        let mut spans = vec![
            Span::styled(format!("{path:<path_width$}"), style_dir()),
            Span::styled(side(difference.left), side_style(newer != Some(Side::Right))),
            Span::styled(arrow, arrow_style),
            Span::styled(side(difference.right), side_style(newer != Some(Side::Left))),
        ];
        if index == plan.cursor {
            for span in &mut spans {
                span.style = span.style.bg(palette().selected_background);
            }
        }
        lines.push(Line::from(spans));
    }
    while lines.len() < (inner.height as usize).saturating_sub(2) {
        lines.push(Line::from(""));
    }
    let [right, left, deleting, skipped] = plan.counts();
    let tally = format!(
        "{right} ──▶    {left} ◀──    {}    {}",
        t!("{deleting} to delete", deleting),
        tn!(skipped, "{skipped} left as it is", "{skipped} left as they are", skipped)
    );
    lines.push(Line::from(Span::styled(cut_to(&tally, width), style_title())).centered());
    let keys = if plan.armed {
        Line::from(Span::styled(
            cut_to(&t!("Enter again to run, deleting {deleting}    Any other key - Not yet", deleting), width),
            Style::new().fg(palette().selected_marker).add_modifier(Modifier::BOLD),
        ))
        .centered()
    } else if !plan.done {
        Line::from(Span::styled(cut_to(t!("Comparing...    Esc - Stop and close"), width), style_columns())).centered()
    } else {
        Line::from(Span::styled(cut_to(t!("→ ← Copy  Del Delete  Space Leave  > < Mirror  R Reset  Enter Run  Esc Close"), width), style_columns())).centered()
    };
    lines.push(keys);
    f.render_widget(Paragraph::new(lines), inner);
    (room, scroll)
}

/// `text` no wider than `room` columns, cut from the end with an ellipsis.
fn cut_to(text: &str, room: usize) -> String {
    if display_width(text) <= room {
        return text.to_string();
    }
    let mut used = 0;
    let mut kept: String = text
        .chars()
        .take_while(|&character| {
            used += char_display_width(character);
            used < room
        })
        .collect();
    if room > 0 {
        kept.push('…');
    }
    kept
}

/// A directory with home as ~, as Ctrl+L reads it back.
fn from_home(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| !home.as_os_str().is_empty());
    match home.as_ref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

/// Alt+Down: the directories visited, newest first, under the words that
/// narrow them, the active panel's own marked - laid out as the bookmarks
/// are.
pub(super) fn render_history_popup(f: &mut ratatui::Frame<'_>, area: Rect, list: &crate::app::HistoryList, here: &Path) {
    let popup_area = popup_rect(60, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Directory History")), style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // The field and a blank row above the list; a blank row and the keys
    // under it.
    let rows = (inner.height as usize).saturating_sub(4);

    let label = format!("{} ", t!("Filter:"));
    let label_width = display_width(&label);
    let typed = list.input.cursor_spans_within(width.saturating_sub(label_width), style_input(), style_input_cursor());
    let mut field = vec![Span::styled(label, style_columns())];
    let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
    field.extend(typed);
    field.push(Span::styled(" ".repeat(width.saturating_sub(label_width + typed_width)), style_input()));
    let mut lines = vec![Line::from(field), Line::from("")];

    if list.shown.is_empty() {
        lines.push(Line::from(Span::styled(t!("Nothing visited matches"), style_file())));
    }
    let first = if rows > 0 && list.index >= rows { list.index + 1 - rows } else { 0 };
    for (number, path) in list.shown.iter().enumerate().skip(first).take(rows) {
        let mark = if path == here { "• " } else { "  " };
        let text = format!("{mark}{}", tail_of(&printable_name(&from_home(path)), width.saturating_sub(mark.len())));
        let pad = " ".repeat(width.saturating_sub(display_width(&text)));
        let style = if number == list.index { style_title().bg(palette().selected_background) } else { style_dir() };
        lines.push(Line::from(Span::styled(format!("{text}{pad}"), style)));
    }
    while lines.len() < rows + 3 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(t!("Type to narrow    Up/Down - Choose    Enter - Go    Esc - Close"), style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
}

/// The branches of the repository a panel is in, the one checked out marked,
/// to change to another. Gives back each row drawn and the branch on it, for
/// a click to find.
pub(super) fn render_branches_popup(f: &mut ratatui::Frame<'_>, area: Rect, list: &BranchList) -> Vec<(Rect, usize)> {
    // As tall as the list, up to what the bookmarks have: the border, a row
    // of margin above and below, and the blank row and the keys under it.
    let most = popup_rect(60, area);
    let popup_area = centered(most, most.width as usize, list.names.len() + 6);
    let title = format!(" {} {} ", t!("Branches:"), shown_name(&list.dir));
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // One blank row and one of keys under the list.
    let rows = (inner.height as usize).saturating_sub(2);
    let first = if rows > 0 && list.index >= rows { list.index + 1 - rows } else { 0 };

    let mut lines = Vec::new();
    let mut hits = Vec::new();
    for (number, name) in list.names.iter().enumerate().skip(first).take(rows) {
        let mark = if Some(number) == list.current { "• " } else { "  " };
        let text = format!("{mark}{}", tail_of(&printable_name(name), width.saturating_sub(mark.len())));
        let pad = " ".repeat(width.saturating_sub(display_width(&text)));
        let style = if number == list.index { style_title().bg(palette().selected_background) } else { style_dir() };
        hits.push((Rect::new(inner.x, inner.y + lines.len() as u16, inner.width, 1), number));
        lines.push(Line::from(Span::styled(format!("{text}{pad}"), style)));
    }
    while lines.len() < rows + 1 {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(t!("Enter/Click - Switch    Esc - Close"), style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
    hits
}

/// The copy or move dialog: what is going, and a field for where to, opened
/// on the other panel - the file's path there for a single file, the
/// directory for anything else. `is_copy` = true for F5 copy, false for F6
/// move.
/// What a destination is asked for, each said whole: a sentence put
/// together from a verb and a preposition reads right in English alone.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Destination {
    Copy,
    Move,
    Symlink,
    HardLink,
    Extract,
    Pack,
}

impl Destination {
    fn title(self, count: usize, is_dir: bool) -> String {
        use Destination::*;
        match (self, count, is_dir) {
            (Extract, _, _) => t!("Extract archive").to_string(),
            (Copy, 1, false) => t!("Copy file").to_string(),
            (Copy, 1, true) => t!("Copy directory").to_string(),
            (Copy, count, _) => tn!(count, "Copy {count} item", "Copy {count} items", count),
            (Move, 1, false) => t!("Move file").to_string(),
            (Move, 1, true) => t!("Move directory").to_string(),
            (Move, count, _) => tn!(count, "Move {count} item", "Move {count} items", count),
            (Symlink, 1, false) => t!("Symlink file").to_string(),
            (Symlink, 1, true) => t!("Symlink directory").to_string(),
            (Symlink, count, _) => tn!(count, "Symlink {count} item", "Symlink {count} items", count),
            (HardLink, 1, false) => t!("Hard link file").to_string(),
            (HardLink, 1, true) => t!("Hard link directory").to_string(),
            (HardLink, count, _) => tn!(count, "Hard link {count} item", "Hard link {count} items", count),
            (Pack, 1, false) => t!("Pack file").to_string(),
            (Pack, 1, true) => t!("Pack directory").to_string(),
            (Pack, count, _) => tn!(count, "Pack {count} item", "Pack {count} items", count),
        }
    }

    /// What goes, and where to: one by its name, more by how many.
    fn source(self, count: usize, name: &str, names: &str) -> String {
        use Destination::*;
        match (self, count) {
            (Copy, 1) => t!("Copy \"{name}\" to:", name),
            (Copy, count) => tn!(count, "Copy {count} item to: {names}", "Copy {count} items to: {names}", count, names),
            (Move, 1) => t!("Move \"{name}\" to:", name),
            (Move, count) => tn!(count, "Move {count} item to: {names}", "Move {count} items to: {names}", count, names),
            (Symlink, 1) => t!("Symlink \"{name}\" at:", name),
            (Symlink, count) => tn!(count, "Symlink {count} item at: {names}", "Symlink {count} items at: {names}", count, names),
            (HardLink, 1) => t!("Hard link \"{name}\" at:", name),
            (HardLink, count) => tn!(count, "Hard link {count} item at: {names}", "Hard link {count} items at: {names}", count, names),
            (Extract, _) => t!("Extract \"{name}\" to:", name),
            (Pack, 1) => t!("Pack \"{name}\" into:", name),
            (Pack, count) => tn!(count, "Pack {count} item into: {names}", "Pack {count} items into: {names}", count, names),
        }
    }

    fn enter(self) -> &'static str {
        match self {
            Destination::Copy => t!("Enter - Copy"),
            Destination::Move => t!("Enter - Move"),
            Destination::Symlink => t!("Enter - Symlink"),
            Destination::HardLink => t!("Enter - Hard link"),
            Destination::Extract => t!("Enter - Extract"),
            Destination::Pack => t!("Enter - Pack"),
        }
    }
}

/// F5, F6 and Ctrl+Shift+F5: what is chosen, and where it is to go, typed.
/// `also` is a key the dialog answers beyond Enter and Esc.
pub(super) fn render_destination_popup(f: &mut ratatui::Frame<'_>, area: Rect, sources: &[(PathBuf, bool)], kind: Destination, input: &TextInput, also: Option<&str>) {
    let popup_area = popup_rect(35, area);
    let count = sources.len();

    let title = format!(" {} ", kind.title(count, sources.first().is_some_and(|(_, is_dir)| *is_dir)));

    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let name = sources.first().map(|(source, _)| shown_name(source)).unwrap_or_default();
    let names: Vec<String> = sources.iter().map(|(source, _)| shown_name(source)).collect();
    let source_msg = kind.source(count, &name, &names.join(", "));

    // The destination, the width of the popup, scrolling to keep the cursor
    // in view on a path longer than that. Padded out to the full width so the
    // highlight reads as an input field.
    let width = popup_inner(popup_area).width as usize;
    let typed = input.cursor_spans_within(width, style_input(), style_input_cursor());
    let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
    let mut spans = typed;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(typed_width))));
    let input_line = Line::from(spans).style(style_input());

    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled(source_msg, style_title())),
            input_line,
            Line::from(Span::styled([Some(kind.enter()), also, Some(t!("Esc - Cancel"))].into_iter().flatten().collect::<Vec<_>>().join("    "), style_columns())),
        ],
    );
}

pub(super) fn render_large_file_popup(f: &mut ratatui::Frame<'_>, area: Rect, large: &crate::app::LargeFile) {
    let name = shown_name(&large.path);
    let popup_area = popup_rect(30, area);
    let title = format!(" {} ", if large.dimensions.is_some() { t!("Large Picture") } else { t!("Large File") });
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // A picture is named by its dimensions as well: the figure beside them is
    // what it unpacks to, which says nothing about the size of the file itself.
    let size = format_size(large.size);
    let message = match (large.dimensions, large.is_edit) {
        (Some((width, height)), true) => t!("Edit \"{name}\" ({width}x{height}, {size})?", name, width, height, size),
        (Some((width, height)), false) => t!("View \"{name}\" ({width}x{height}, {size})?", name, width, height, size),
        (None, true) => t!("Edit \"{name}\" ({size})?", name, size),
        (None, false) => t!("View \"{name}\" ({size})?", name, size),
    };
    let note = if large.dimensions.is_some() {
        t!("Decoding a picture this size needs that much memory.")
    } else {
        t!("Reading a file this large may take a while.")
    };
    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled(message, style_title())),
            Line::from(Span::styled(note, style_file())),
            Line::from(Span::styled(t!("Y / Enter - Yes    N / Esc - No"), style_columns())),
        ],
    );
}

pub(super) fn render_editor_save_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let popup_area = popup_rect(25, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Unsaved Changes")), style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled(t!("Save changes before closing?"), style_title())),
            Line::from(Span::styled(t!("Y - Save    N - Discard    Esc - Cancel"), style_columns())),
        ],
    );
}

/// How much of the screen's width every popup takes, as a percentage: one
/// width for all of them, so going from one to the next does not jump about.
pub(super) const POPUP_WIDTH: u16 = 60;

/// A popup `percent_y` of the screen's height and POPUP_WIDTH of its width,
/// centred over the panels and no taller than they are, so the bars above and
/// below always show. What does not fit wraps, shortens or scrolls inside it.
pub(super) fn popup_rect(percent_y: u16, area: Rect) -> Rect {
    let panels = screen_layout(area)[2];
    let width = area.width as usize * POPUP_WIDTH as usize / 100;
    let height = area.height as usize * percent_y as usize / 100;
    centered(panels, width.max(1), height.max(1))
}
