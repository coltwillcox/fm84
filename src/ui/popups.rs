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
    let popup_block = Block::default().title(Line::from(Span::styled(" Error ", style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    popup_body(f, popup_area, vec![Line::from(Span::styled(printable_name(message), style_title()))]);
}

pub(super) fn render_create_popup(f: &mut ratatui::Frame<'_>, area: Rect, is_dir: bool, input: &TextInput) {
    let popup_area = popup_rect(20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(if is_dir { " Create Directory " } else { " Create File " }, style_title())).centered())
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
    popup_body(f, popup_area, vec![input_line, Line::from(Span::styled("Enter - Create    Esc - Cancel", style_columns()))]);
}

pub(super) fn render_delete_popup(f: &mut ratatui::Frame<'_>, area: Rect, items: &[(PathBuf, bool)], to_trash: bool) {
    let count = items.len();
    let popup_area = popup_rect(30, area);

    // Which kind of delete it is, said plainly: one can be undone and the
    // other cannot.
    let verb = if to_trash { "Move to trash" } else { "Delete" };
    let title = if count == 1 {
        let item_type = if items[0].1 { "directory" } else { "file" };
        format!(" {verb}: {item_type} ")
    } else {
        format!(" {verb}: {count} items ")
    };

    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let mut lines = Vec::new();
    let question = if to_trash { "Move {} to the trash?" } else { "Delete {} permanently?" };
    if count == 1 {
        let name = format!("\"{}\"", shown_name(&items[0].0));
        lines.push(Line::from(Span::styled(question.replace("{}", &name), style_title())));
    } else {
        let names: Vec<String> = items.iter().map(|(path, _)| shown_name(path)).collect();
        lines.push(Line::from(Span::styled(question.replace("{}", &format!("{count} items")), style_title())));
        lines.push(Line::from(Span::styled(names.join(", "), style_file())));
    }
    if !to_trash {
        lines.push(Line::from(Span::styled("This cannot be undone", style_file())));
    }
    lines.push(Line::from(Span::styled("Y / Enter - Yes    N / Esc - No", style_columns())));
    popup_body(f, popup_area, lines);
}

/// A file in the way of one being copied or moved: both of them, size and
/// date, so which to keep can be weighed, and the answers.
fn render_job_clash(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, clash: &crate::app::JobClash) {
    let popup_area = popup_rect(40, area);
    let popup_block = Block::default().title(Line::from(Span::styled(" Already there ", style_title())).centered()).borders(Borders::ALL).style(style_border());

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
    let describe = |label: &str, size: Option<u64>, modified: Option<std::time::SystemTime>, other: Option<std::time::SystemTime>| {
        let size = size.map(|bytes| if exact { format!("{} B", grouped(bytes)) } else { format_size(bytes) }).unwrap_or_else(|| "?".to_string());
        let date = modified.map(|at| format_exact(at, app_state.options.date_format)).unwrap_or_else(|| "?".to_string());
        let newer = matches!((modified, other), (Some(this), Some(that)) if this > that);
        format!("{label}  {size:>width$}  {date}{}", if newer { "  newer" } else { "       " })
    };
    let mut lines = vec![Line::from(Span::styled(format!("\"{}\" is already in {folder}", shown_name(&clash.dest)), style_title()))];
    // Copied under another name, the new one is not the name in the question.
    if clash.source.file_name() != clash.dest.file_name() {
        lines.push(Line::from(Span::styled(format!("to be replaced by \"{}\"", shown_name(&clash.source)), style_file())));
    }
    lines.extend([
        Line::from(Span::styled(describe("New     ", clash.source_size, clash.source_modified, clash.dest_modified), style_file())),
        Line::from(Span::styled(describe("Existing", clash.dest_size, clash.dest_modified, clash.source_modified), style_file())),
        Line::from(Span::styled("Y - Overwrite    N - Skip    K - Keep both", style_columns())),
        Line::from(Span::styled("A - Overwrite all    S - Skip all    B - Keep both all", style_columns())),
        Line::from(Span::styled("O - Overwrite all older    Esc - Abort", style_columns())),
    ]);
    if app_state.quit_armed {
        lines.push(Line::from(Span::styled("F10 again - Quit", style_file())));
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
    let Some(job) = &app_state.job else {
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
    let elapsed = job.started.elapsed();
    if elapsed < TRANSFER_POPUP_DELAY {
        return;
    }

    let title = if job.is_cancelling() {
        " Cancelling ".to_string()
    } else {
        match job.kind {
            TransferKind::Copy => " Copying ".to_string(),
            TransferKind::Move => " Moving ".to_string(),
            TransferKind::Delete => " Deleting ".to_string(),
            TransferKind::Trash => " Moving to trash ".to_string(),
            TransferKind::Extract => " Extracting ".to_string(),
            TransferKind::Pack => " Packing ".to_string(),
            TransferKind::Checksum => " Checksumming ".to_string(),
        }
    };
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
        None => "Counting...".to_string(),
    };

    // Once F10 has been pressed the offer stays up for the rest of the job, so
    // there is always something on screen saying how to get out of one that is
    // not going to finish.
    let hint = if app_state.quit_armed { "Esc - Cancel    F10 again - Quit" } else { "Esc - Cancel" };

    // A copy moves bytes and a delete removes entries, so each is counted in
    // what it actually does rather than forcing both into the same figure.
    let detail = match (job.kind, job.total) {
        (TransferKind::Delete, Some(total)) if total > 0 => {
            format!("{} of {} entries    {hint}", job.done, total)
        }
        (TransferKind::Trash, Some(total)) if total > 0 => {
            format!("{} of {} items    {hint}", job.done, total)
        }
        (_, Some(total)) if total > 0 => {
            let rate = job.done as f64 / elapsed.as_secs_f64().max(0.001);
            format!("{} of {} at {}/s    {hint}", format_size(job.done), format_size(total), format_size(rate as u64))
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
        TransferKind::Copy => " Cannot copy ",
        TransferKind::Move => " Cannot move ",
        TransferKind::Delete => " Cannot delete ",
        TransferKind::Trash => " Cannot move to trash ",
        TransferKind::Extract => " Cannot extract ",
        TransferKind::Pack => " Cannot pack ",
        TransferKind::Checksum => " Cannot read ",
    };
    let popup_area = popup_rect(30, area);
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let hint = "R - Retry    S - Skip    A - Skip all    Esc - Abort";
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
        lines.push(Line::from(Span::styled("D - Delete permanently", style_file())));
    }
    if quit_armed {
        lines.push(Line::from(Span::styled("F10 again - Quit", style_file())));
    }
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).wrap(ratatui::widgets::Wrap { trim: true }), popup_area.inner(Margin { vertical: 1, horizontal: 2 }));
}

/// Asks before a save writes over what something else has written since the
/// editor read the file: the file as it was then and as it is now, and the
/// answers. A file that has gone is said to have, and cannot be reloaded.
pub(super) fn render_disk_changed_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, opened: Option<crate::app::DiskStamp>, now: Option<crate::app::DiskStamp>) {
    let popup_area = popup_rect(40, area);
    let popup_block = Block::default().title(Line::from(Span::styled(" Changed on disk ", style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let name = app_state.editor().map(|state| shown_name(&state.file_path)).unwrap_or_default();
    let stamp = |label: &str, stamp: Option<crate::app::DiskStamp>| {
        let shown = match stamp {
            Some(stamp) => {
                let date = stamp.modified.map(|at| format_exact(at, app_state.options.date_format)).unwrap_or_else(|| "?".to_string());
                format!("{:>9}  {date}", format_size(stamp.size))
            }
            None => "not there".to_string(),
        };
        Line::from(Span::styled(format!("{label:<12}{shown}"), style_file()))
    };
    let mut lines = match now {
        Some(_) => vec![Line::from(Span::styled(format!("\"{name}\" has changed since it was opened"), style_title()))],
        None => vec![Line::from(Span::styled(format!("\"{name}\" is no longer there"), style_title()))],
    };
    lines.push(stamp("When opened", opened));
    lines.push(stamp("Now", now));
    let keys = match now {
        Some(_) => "O - Overwrite it    R - Reload, losing your edits",
        None => "O - Save it again",
    };
    lines.push(Line::from(Span::styled(keys, style_columns())));
    lines.push(Line::from(Span::styled("Esc - Back to editing", style_columns())));
    popup_body(f, popup_area, lines);
}

/// Alt+F7: where the search starts, the two fields, and what goes in them.
pub(super) fn render_find_files_form(f: &mut ratatui::Frame<'_>, area: Rect, start: &Path, name: &TextInput, containing: &TextInput, on_containing: bool) {
    let popup_area = popup_rect(45, area);
    let popup_block = Block::default().title(Line::from(Span::styled(" Find files ", style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let width = popup_inner(popup_area).width as usize;
    const LABEL: usize = 12;
    // The field the keys go to has the cursor in it and a bar behind it; the
    // other shows what it holds.
    let field = |label: &str, input: &TextInput, focused: bool| {
        let room = width.saturating_sub(LABEL);
        let mut spans = vec![Span::styled(format!("{label:<LABEL$}"), style_columns())];
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
                format!("{:<LABEL$}{:<room$}", "In", tail_of(&start.to_string_lossy(), width.saturating_sub(LABEL)), room = width.saturating_sub(LABEL)),
                style_file(),
            )),
            field("Name", name, !on_containing),
            field("Containing", containing, on_containing),
            Line::from(Span::styled("Name as + takes it: *.jpg;*.png, or */ for directories. Lower case matches either case", style_columns())),
            Line::from(Span::styled("Tab - Next field    Enter - Find    Esc - Cancel", style_columns())),
        ],
    );
}

/// The Find files results: how the search is getting on, and what it has
/// found, shown from where it started, with the cursor on one. The number of
/// results that fit, for PageUp and PageDown.
pub(super) fn render_find_files_results(f: &mut ratatui::Frame<'_>, area: Rect, search: &crate::app::Search) -> usize {
    let popup_area = popup_rect(80, area);
    let title = format!(" Find: {} ", printable_name(&search.label));
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
            let said = format!("{found} found, searching ");
            format!("{said}{}", tail_of(&dir.to_string_lossy(), width.saturating_sub(display_width(&said) + 1)))
        }
        None if found == 0 => "Nothing found".to_string(),
        None => format!("{found} found"),
    };
    let status = match search.unreadable {
        0 => status,
        1 => format!("{status} - 1 directory could not be read"),
        many => format!("{status} - {many} directories could not be read"),
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
    let hint = if search.done { "Enter - Go to    Esc - Close" } else { "Enter - Go to    Esc - Stop and close" };
    lines.push(Line::from(Span::styled(hint, style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
    rows
}

/// Alt+A: a field for each thing to change, the one being typed into barred
/// as Find files' are. With one entry, the mode as it would come out is
/// written beside its field.
pub(super) fn render_attributes_popup(f: &mut ratatui::Frame<'_>, area: Rect, form: &AttributesForm) -> Vec<(Rect, u16)> {
    let popup_area = popup_rect(50, area);
    let popup_block = Block::default().title(Line::from(Span::styled(" Attributes ", style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let width = popup_inner(popup_area).width as usize;
    const LABEL: usize = 10;
    // Room kept after the mode for what it comes to.
    const BECOMES: usize = 12;
    let what = match form.items.as_slice() {
        [(path, is_dir)] => format!("{} \"{}\"", if *is_dir { "Directory" } else { "File" }, shown_name(path)),
        items => format!("{} items - a field left as it is, or empty, changes nothing", items.len()),
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
        lines.push(Line::from(Span::styled("Mode as 755 or u+x,go-w; owner and group by name or number", style_columns())));
    }
    lines.push(Line::from(Span::styled("Tab - Next field    Enter - Apply    Esc - Cancel", style_columns())));
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
    let popup_block = Block::default().title(Line::from(Span::styled(" Bookmarks ", style_title())).centered()).borders(Borders::ALL).style(style_border());
    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    // One blank row and two of keys under the list.
    let rows = (inner.height as usize).saturating_sub(3);
    let home = std::env::var_os("HOME").map(PathBuf::from).filter(|home| !home.as_os_str().is_empty());

    let mut lines = Vec::new();
    if list.is_empty() {
        lines.push(Line::from(Span::styled("No bookmarks yet - A keeps the panel's directory", style_file())));
    }
    let first = if rows > 0 && index >= rows { index + 1 - rows } else { 0 };
    for (number, path) in list.iter().enumerate().skip(first).take(rows) {
        // Home as ~, as Ctrl+L reads it back.
        let shown = match home.as_ref().and_then(|home| path.strip_prefix(home).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
            Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.to_string_lossy()),
            None => path.to_string_lossy().into_owned(),
        };
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
    lines.push(Line::from(Span::styled("Enter/1-9 - Go    A/Ins - Add here    D/Del - Remove", style_columns())).centered());
    lines.push(Line::from(Span::styled("Shift+Up/Down - Move    Esc - Close", style_columns())).centered());
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
    let title = format!(" Branches: {} ", shown_name(&list.dir));
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
    lines.push(Line::from(Span::styled("Enter/Click - Switch    Esc - Close", style_columns())).centered());
    f.render_widget(Paragraph::new(lines), inner);
    hits
}

/// The copy or move dialog: what is going, and a field for where to, opened
/// on the other panel - the file's path there for a single file, the
/// directory for anything else. `is_copy` = true for F5 copy, false for F6
/// move.
/// F5, F6 and Ctrl+Shift+F5: what is chosen, and where it is to go, typed.
/// `verb` names the operation, and `preposition` leads into the destination.
/// `what` names a single source when "file" or "directory" would not say it,
/// and `also` is a key the dialog answers beyond Enter and Esc.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_destination_popup(f: &mut ratatui::Frame<'_>, area: Rect, sources: &[(PathBuf, bool)], what: Option<&str>, verb: &str, preposition: &str, input: &TextInput, also: Option<&str>) {
    let popup_area = popup_rect(35, area);
    let count = sources.len();

    let title = if count == 1 {
        let item_type = what.unwrap_or(if sources[0].1 { "directory" } else { "file" });
        format!(" {} {} ", verb, item_type)
    } else {
        format!(" {} {} items ", verb, count)
    };

    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let source_msg = if count == 1 {
        format!("{} \"{}\" {}:", verb, shown_name(&sources[0].0), preposition)
    } else {
        let names: Vec<String> = sources.iter().map(|(source, _)| shown_name(source)).collect();
        format!("{} {} items {}: {}", verb, count, preposition, names.join(", "))
    };

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
            Line::from(Span::styled(
                [Some(format!("Enter - {verb}")), also.map(str::to_string), Some("Esc - Cancel".to_string())].into_iter().flatten().collect::<Vec<_>>().join("    "),
                style_columns(),
            )),
        ],
    );
}

pub(super) fn render_large_file_popup(f: &mut ratatui::Frame<'_>, area: Rect, large: &crate::app::LargeFile) {
    let name = shown_name(&large.path);
    let verb = if large.is_edit { "Edit" } else { "View" };

    let popup_area = popup_rect(30, area);
    let title = if large.dimensions.is_some() { " Large Picture " } else { " Large File " };
    let popup_block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // A picture is named by its dimensions as well: the figure beside them is
    // what it unpacks to, which says nothing about the size of the file itself.
    let message = match large.dimensions {
        Some((width, height)) => format!("{} \"{}\" ({}x{}, {})?", verb, name, width, height, format_size(large.size)),
        None => format!("{} \"{}\" ({})?", verb, name, format_size(large.size)),
    };
    let note = if large.dimensions.is_some() {
        "Decoding a picture this size needs that much memory."
    } else {
        "Reading a file this large may take a while."
    };
    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled(message, style_title())),
            Line::from(Span::styled(note, style_file())),
            Line::from(Span::styled("Y / Enter - Yes    N / Esc - No", style_columns())),
        ],
    );
}

pub(super) fn render_editor_save_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let popup_area = popup_rect(25, area);
    let popup_block = Block::default().title(Line::from(Span::styled(" Unsaved Changes ", style_title())).centered()).borders(Borders::ALL).style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled("Save changes before closing?", style_title())),
            Line::from(Span::styled("Y - Save    N - Discard    Esc - Cancel", style_columns())),
        ],
    );
}

/// How much of the screen's width every popup takes, as a percentage: one
/// width for all of them, so going from one to the next does not jump about.
pub(super) const POPUP_WIDTH: u16 = 60;

/// A popup `percent_y` of the screen's height and POPUP_WIDTH of its width,
/// centred over the panels and no taller than they are, so the bars above and
/// below always show. What does not fit wraps, shortens or scrolls inside it.
fn popup_rect(percent_y: u16, area: Rect) -> Rect {
    let panels = screen_layout(area)[2];
    let width = area.width as usize * POPUP_WIDTH as usize / 100;
    let height = area.height as usize * percent_y as usize / 100;
    centered(panels, width.max(1), height.max(1))
}
