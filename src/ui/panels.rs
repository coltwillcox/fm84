//! The two panels and what sits above them: the top bar, the drive and tab
//! strips, the path bar, the file tables, and the preview F12 puts in one.

use super::*;

/// The columns beside Name. Name is never dropped, so it is not one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Column {
    Ext,
    Size,
    Modified,
    Attributes,
}

/// Widest-priority first: as a panel narrows they are given up from the end.
const OPTIONAL_COLUMNS: [Column; 4] = [Column::Ext, Column::Size, Column::Modified, Column::Attributes];

/// Below this, a filename is no longer worth reading, so the next column goes.
const MIN_NAME_WIDTH: u16 = 12;

impl Column {
    fn title(self) -> &'static str {
        match self {
            Column::Ext => "Ext",
            Column::Size => "Size",
            Column::Modified => "Modified",
            Column::Attributes => "Attributes",
        }
    }

    /// What a click on its heading sorts by. Attributes are not a key.
    fn sort_key(self) -> Option<SortKey> {
        match self {
            Column::Ext => Some(SortKey::Extension),
            Column::Size => Some(SortKey::Size),
            Column::Modified => Some(SortKey::Modified),
            Column::Attributes => None,
        }
    }

    fn width(self, options: &Options) -> u16 {
        match self {
            Column::Ext => 5,
            Column::Size => 8,
            Column::Modified => match options.date_format {
                DateFormat::Short => 14,
                DateFormat::Iso => 16,
                // "29 days ago", the longest a relative date gets.
                DateFormat::Relative => 11,
            },
            Column::Attributes => 10,
        }
    }

    fn enabled(self, options: &Options) -> bool {
        match self {
            Column::Ext => options.column_ext,
            Column::Size => options.column_size,
            Column::Modified => options.column_modified,
            Column::Attributes => options.column_attributes,
        }
    }
}

/// The optional columns a panel of this width can carry, of those F11 leaves
/// on. Each costs its own width plus the separator and the spacing either side
/// of it; whatever is left over belongs to Name.
fn visible_columns(options: &Options, panel_width: u16) -> Vec<Column> {
    let mut used = 3; // icon plus the gap after it
    let mut columns = Vec::with_capacity(OPTIONAL_COLUMNS.len());
    for column in OPTIONAL_COLUMNS.into_iter().filter(|column| column.enabled(options)) {
        let next = used + column.width(options) + 3;
        if panel_width.saturating_sub(next) < MIN_NAME_WIDTH {
            break;
        }
        used = next;
        columns.push(column);
    }
    columns
}

/// What the Name column is left with. It is a Fill(1), so it gets whatever the
/// icon, the columns beside it, the spacing between them and the panel's left
/// border have not taken. The rename box needs the figure to keep its cursor in
/// view; `visible_columns` already counts the same three columns per entry.
fn name_width(options: &Options, panel_width: u16) -> u16 {
    let mut used = 3; // icon plus the gap after it
    for column in visible_columns(options, panel_width) {
        used += column.width(options) + 3;
    }
    panel_width.saturating_sub(used + 1) // the panel's left border
}

/// The left / separator / right split every full-width row shares, so the rows
/// that line up with the panels all derive their columns the same way.
fn panel_split(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(50)]).split(area)
}

fn mount_icon(kind: crate::fs_ops::MountKind, style: IconStyle) -> &'static str {
    use crate::fs_ops::MountKind;
    if style == IconStyle::Plain {
        return match kind {
            MountKind::Home => "~",
            MountKind::Disk => "D",
            MountKind::Removable => "U",
            MountKind::Network => "N",
            MountKind::Optical => "O",
        };
    }
    match kind {
        MountKind::Home => ICON_HOME,
        MountKind::Disk => ICON_DRIVE,
        MountKind::Removable => ICON_REMOVABLE,
        MountKind::Network => ICON_NETWORK,
        MountKind::Optical => ICON_OPTICAL,
    }
}

/// The icon cell of a file row. Plain marks directories the way MC does and
/// leaves files blank; the brackets and colours already tell them apart.
fn row_icon(is_dir: bool, style: IconStyle) -> &'static str {
    match (style, is_dir) {
        (IconStyle::Plain, true) => "/",
        (IconStyle::Plain, false) => " ",
        (_, true) => ICON_FOLDER,
        (_, false) => ICON_FILE,
    }
}

pub(super) fn logo_icon(style: IconStyle) -> &'static str {
    if style == IconStyle::Plain { "84" } else { ICON_LOGO }
}

/// Lay out a strip of `items` across `area`, recording in `strip` where each
/// one is drawn - measured from the text actually drawn, so a click cannot
/// land anywhere but where it looks.
///
/// When they do not all fit, it shows as many as do, with `<` and `>` at the
/// ends while there are more that way - clicked, or under the wheel, they
/// move a page along. The window follows `focus`, so the keys reach every
/// item without them.
fn lay_strip(strip: &mut Strip, area: Rect, items: Vec<Span<'static>>, focus: Option<usize>) -> Vec<Span<'static>> {
    let widths: Vec<usize> = items.iter().map(|item| display_width(&item.content)).collect();
    let room = area.width as usize;
    let fits = Strip::fits(&widths, room);
    let count = items.len();
    strip.area = area;
    strip.place(widths, room, focus);
    let (first, visible) = (strip.first, strip.visible);

    let mut spans = Vec::with_capacity(visible + 2);
    let mut slots = Vec::with_capacity(visible + 2);
    let mut hits = Vec::with_capacity(visible + 2);
    let mut column;
    if fits {
        spans.push(Span::raw(" "));
        column = 1; // past the space the strip opens with
    } else {
        // The arrows take their columns there or not, so the items stay put
        // as they come and go.
        let back = first > 0;
        spans.push(Span::styled(if back { " <" } else { "  " }, style_dir_dark()));
        if back {
            slots.push((0, ARROW as u16));
            hits.push(StripHit::Back);
        }
        column = ARROW as u16;
    }

    for (index, item) in items.into_iter().enumerate().skip(first).take(visible) {
        let width = display_width(&item.content) as u16;
        slots.push((column, width));
        hits.push(StripHit::Item(index));
        column += width;
        spans.push(item);
    }

    if !fits {
        let forward = first + visible < count;
        spans.push(Span::styled(if forward { "> " } else { "  " }, style_dir_dark()));
        if forward {
            slots.push((column, ARROW as u16));
            hits.push(StripHit::Forward);
        }
    }

    strip.slots = slots;
    strip.hits = hits;
    spans
}

/// One panel's row of drive icons: the mount it is on sits in a block of colour, and while
/// that panel is choosing, the candidate is highlighted and named. The window
/// onto them, when they do not all fit, follows the candidate while choosing
/// and the panel's own drive otherwise.
pub(super) fn drive_strip(app_state: &mut AppState, is_left: bool, area: Rect) -> Line<'static> {
    let current = app_state.current_mount(is_left);
    let picking = match app_state.dialog {
        Some(Dialog::DrivePicker { is_left: side, index }) if side == is_left => Some(index),
        _ => None,
    };

    let icon_style = app_state.options.icon_style;
    let spill = if icon_style.is_wide() { " " } else { "" };
    let items = app_state
        .mounts
        .iter()
        .enumerate()
        .map(|(index, mount)| {
            // A block of colour marks the mount this panel is on, and a brighter
            // one the candidate while choosing - drawn like the row under a
            // panel's cursor, with its foreground and pair of backgrounds. Every
            // slot is the same width, so switching drives moves nothing but the
            // block, and all of it is clickable. Nerd Font glyphs count as one
            // cell but most fonts draw them across two, so there each icon gets a
            // blank cell after it to spill into - otherwise it sits half a cell
            // right of centre. Mono fonts and plain letters fit in the one.
            let style = if Some(index) == picking {
                Style::new().fg(palette().selected_foreground).bg(palette().selected_background)
            } else if Some(index) == current {
                Style::new().fg(palette().selected_foreground).bg(palette().selected_background_inactive)
            } else {
                style_dir_dark()
            };
            Span::styled(format!("  {}  {}", mount_icon(mount.kind, icon_style), spill), style)
        })
        .collect();

    let strip = if is_left { &mut app_state.drive_strip_left } else { &mut app_state.drive_strip_right };
    let mut spans = lay_strip(strip, area, items, picking.or(current));

    // Name whichever is under consideration, else the one this panel is on.
    if let Some(mount) = picking.or(current).and_then(|index| app_state.mounts.get(index)) {
        spans.push(Span::styled(format!(" {}", printable_name(&mount.label)), style_columns()));
    }
    Line::from(spans)
}

/// A tab is named for its directory, cut short past a point so a few fit
/// side by side - the path bar below has the whole of the one shown.
pub(super) fn tab_label(dir: &Path) -> String {
    const LONGEST: usize = 16;
    let name = dir.file_name().map_or_else(|| dir.display().to_string(), |name| name.to_string_lossy().into_owned());
    let name = printable_name(&name);
    if display_width(&name) <= LONGEST {
        return name;
    }
    let mut width = 0;
    let kept: String = name
        .chars()
        .take_while(|&character| {
            width += UnicodeWidthChar::width(character).unwrap_or(0);
            width < LONGEST
        })
        .collect();
    format!("{kept}…")
}

/// One panel's row of tabs, under its drives. The tab it shows sits in a
/// block of colour, the brighter one in the active panel, as the drives do.
pub(super) fn tab_strip(app_state: &mut AppState, is_left: bool, area: Rect) -> Line<'static> {
    let (dirs, active) = app_state.tab_dirs(is_left);
    let panel_active = app_state.is_left_active == is_left;
    let items = dirs
        .iter()
        .enumerate()
        .map(|(index, dir)| {
            let style = if index != active {
                style_dir_dark()
            } else if panel_active {
                Style::new().fg(palette().selected_foreground).bg(palette().selected_background)
            } else {
                Style::new().fg(palette().selected_foreground).bg(palette().selected_background_inactive)
            };
            Span::styled(format!(" {} ", tab_label(dir)), style)
        })
        .collect();

    let tabs = if is_left { &mut app_state.tabs_left } else { &mut app_state.tabs_right };
    Line::from(lay_strip(&mut tabs.strip, area, items, Some(active)))
}

pub(super) fn render_top_panel(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) {
    let cached_clock = app_state.cached_clock.as_str();
    let logo = Span::styled(format!(" {} ", logo_icon(app_state.options.icon_style)), style_title());
    let title = Span::styled(format!(" {} v{} ", TITLE, VERSION), style_title());
    let clock = Span::styled(cached_clock, style_title());
    // Who and where, beside the logo - reversed as root, which is worth
    // seeing before a delete. Left off where it would run into the title.
    let (user_at_host, root) = &app_state.user_at_host;
    let user_style = if *root { style_title().add_modifier(Modifier::REVERSED) } else { style_title() };
    let user = Span::styled(format!(" {} ", printable_name(user_at_host)), user_style);
    // A title of its own, so the border runs between it and the logo as it
    // does between the F-keys.
    let left_width = display_width(&logo.content) + 1 + display_width(&user.content);
    let fits = left_width.max(display_width(&clock.content)) * 2 + display_width(&title.content) + 4 <= area.width as usize;

    let mut block_top = Block::default().title_top(Line::from(logo).left_aligned());
    if fits {
        block_top = block_top.title_top(Line::from(user).left_aligned());
    }
    let block_top = block_top
        .title_top(Line::from(title).centered())
        .title_top(Line::from(clock).right_aligned())
        .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
        .border_style(style_border());

    let inner = block_top.inner(area);
    f.render_widget(block_top, area);

    // Each strip hands its real geometry to the mouse handler, as the panels
    // do. One not drawn this frame has nothing to click either.
    app_state.drive_strip_left.area = Rect::default();
    app_state.drive_strip_right.area = Rect::default();
    app_state.tabs_left.strip.area = Rect::default();
    app_state.tabs_right.strip.area = Rect::default();

    if inner.height > 0 && !app_state.mounts.is_empty() {
        let halves = panel_split(Rect { height: 1, ..inner });
        let left = drive_strip(app_state, true, halves[0]);
        let right = drive_strip(app_state, false, halves[2]);
        f.render_widget(Paragraph::new(left), halves[0]);
        f.render_widget(Paragraph::new(right), halves[2]);
    }
    // The tabs take the row under the drives.
    if inner.height > 1 {
        let halves = panel_split(Rect { y: inner.y + 1, height: 1, ..inner });
        let left = tab_strip(app_state, true, halves[0]);
        let right = tab_strip(app_state, false, halves[2]);
        f.render_widget(Paragraph::new(left), halves[0]);
        f.render_widget(Paragraph::new(right), halves[2]);
    }
}

/// The directory each panel is in, along the line over them. Ctrl+L edits
/// the active one's in place: `going_to` is what is being typed there.
///
/// After each path, the git branch its directory is on, while the path keeps
/// room enough to read; it is the branch that goes first. Gives back where
/// each side's branch was drawn, left then right, for a click to find.
pub(super) fn render_path_bar(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, going_to: Option<&TextInput>) -> [Option<Rect>; 2] {
    let total_width = area.width;
    let is_left_active = app_state.is_left_active;
    let length_left = ((total_width as usize).saturating_sub(3)) / 2;
    let length_right = ((total_width as usize).saturating_sub(2)) / 2;
    // Fewer columns than this for the path, and the branch is left off.
    const PATH_ROOM: usize = 12;

    let (color_left, color_right) = if is_left_active { (style_dir(), style_dir_dark()) } else { (style_dir_dark(), style_dir()) };
    let branch_label = |branch: &Option<String>| {
        branch.as_ref().map(|branch| match app_state.options.icon_style {
            IconStyle::Plain => format!(" git:{} ", printable_name(branch)),
            _ => format!(" {ICON_BRANCH} {} ", printable_name(branch)),
        })
    };
    // One side's path, padded and followed by the border up to where the next
    // part of the line starts. With which of the spans is the branch, if
    // there is one.
    let side = |dir: &std::path::Path, branch: Option<String>, length: usize, style: Style, editing: Option<&TextInput>, end: &'static str| -> (Vec<Span<'static>>, Option<usize>) {
        let mut spans = vec![Span::styled(" ", style)];
        // The branch, and the dash before it.
        let branch = branch.filter(|label| editing.is_none() && length.saturating_sub(8 + display_width(label) + 1) >= PATH_ROOM);
        let branch_width = branch.as_ref().map_or(0, |label| display_width(label) + 1);
        match editing {
            // As wide as the side has room for, the cursor kept in view.
            Some(input) => spans.extend(input.cursor_spans_within(length.saturating_sub(7), style_input(), style_input_cursor())),
            None => spans.push(Span::styled(limit_path_string(dir, length.saturating_sub(8 + branch_width)), style)),
        }
        spans.push(Span::styled(" ", style));
        let mut label_at = None;
        if let Some(label) = branch {
            spans.push(Span::styled("─", style_border()));
            label_at = Some(spans.len());
            spans.push(Span::styled(label, style_columns()));
        }
        let used: usize = spans.iter().map(|span| display_width(&span.content)).sum();
        spans.push(Span::styled(format!("{}{end}", "─".repeat(length.saturating_sub(used + 3))), style_border()));
        (spans, label_at)
    };

    let mut border_line = vec![Span::styled("├──", style_border())];
    let mut branches = [None, None];
    let sides = [
        (&app_state.dir_left, &app_state.branch_left, length_left, color_left, going_to.filter(|_| is_left_active), "─┬──"),
        (&app_state.dir_right, &app_state.branch_right, length_right, color_right, going_to.filter(|_| !is_left_active), "─┤"),
    ];
    for (branch, (dir, label, length, style, editing, end)) in branches.iter_mut().zip(sides) {
        let (spans, label_at) = side(dir, branch_label(label), length, style, editing, end);
        if let Some(label_at) = label_at {
            // Measured from what is drawn before it, as the strips are.
            let before: usize = border_line.iter().chain(&spans[..label_at]).map(|span| display_width(&span.content)).sum();
            let width = display_width(&spans[label_at].content);
            *branch = Some(Rect::new(area.x + before as u16, area.y, width as u16, 1).intersection(area));
        }
        border_line.extend(spans);
    }

    f.render_widget(Paragraph::new(Line::from(border_line)), area);
    branches
}

pub(super) fn render_file_tables(f: &mut ratatui::Frame<'_>, chunk: Rect, app_state: &mut AppState) -> u16 {
    let chunks = panel_split(chunk);

    let columns = visible_columns(&app_state.options, chunks[0].width);
    let mut widths = vec![Constraint::Length(2), Constraint::Fill(1)];
    for column in &columns {
        widths.push(Constraint::Length(1));
        widths.push(Constraint::Length(column.width(&app_state.options)));
    }

    let renaming = matches!(app_state.dialog, Some(Dialog::Rename(_)));
    let table_style = |active: bool| {
        Style::default()
            .bg(if active {
                if renaming { palette().rename_background } else { palette().selected_background }
            } else {
                palette().selected_background_inactive
            })
            .fg(palette().selected_foreground)
            .add_modifier(Modifier::BOLD)
    };

    // Viewport height (subtract 1 for header row)
    let viewport_height = chunks[0].height.saturating_sub(1) as usize;

    let header = make_header_row(&columns, &app_state.options);

    // Build only visible rows for left panel
    let field = name_width(&app_state.options, chunks[0].width);
    let (rows_left, offset_left) = build_viewport_rows(app_state, true, viewport_height, &columns, field);
    let mut state_left_view = TableState::default();
    state_left_view.select(app_state.state_left.selected().map(|s| s.saturating_sub(offset_left)));

    let block_left = Block::default().borders(Borders::LEFT).border_style(style_border());
    let block_right = Block::default().borders(Borders::RIGHT).border_style(style_border());
    // Where each heading lands, laid out as the table lays out its columns,
    // for a click to sort by. Not under the preview, which covers one panel.
    app_state.sort_headers.clear();
    for (area, block, is_left) in [(chunks[0], &block_left, true), (chunks[2], &block_right, false)] {
        if app_state.preview.is_some() && is_left != app_state.is_left_active {
            continue;
        }
        let inner = block.inner(area);
        let cells = Layout::horizontal(widths.clone()).spacing(1).split(Rect { height: 1, ..inner });
        // The icon and Name both sort by name; then a spacer and a column each.
        let name = cells[0].union(cells[1]);
        app_state.sort_headers.push((name, SortKey::Name));
        for (index, column) in columns.iter().enumerate() {
            if let Some(key) = column.sort_key() {
                app_state.sort_headers.push((cells[3 + 2 * index], key));
            }
        }
    }

    let table_left = Table::new(rows_left, widths.clone()).block(block_left).header(header.clone()).row_highlight_style(table_style(app_state.is_left_active)).column_spacing(1);
    f.render_stateful_widget(table_left, chunks[0], &mut state_left_view);

    // Cache the separator string based on height
    let separator_height = chunks[0].height;
    if app_state.cached_separator_height != separator_height {
        app_state.cached_separator_height = separator_height;
        app_state.cached_separator = "│\n".repeat(separator_height.saturating_sub(1) as usize) + "│";
    }
    let separator_vertical = Paragraph::new(Text::raw(&app_state.cached_separator)).style(style_border());
    f.render_widget(separator_vertical, chunks[1]);

    // Build only visible rows for right panel
    let (rows_right, offset_right) = build_viewport_rows(app_state, false, viewport_height, &columns, field);
    let mut state_right_view = TableState::default();
    state_right_view.select(app_state.state_right.selected().map(|s| s.saturating_sub(offset_right)));

    let table_right = Table::new(rows_right, widths).block(block_right).header(header).row_highlight_style(table_style(!app_state.is_left_active)).column_spacing(1);
    f.render_stateful_widget(table_right, chunks[2], &mut state_right_view);

    // Preview takes over the panel the cursor is not in.
    if let Some(preview) = &app_state.preview {
        let (area, is_left) = if app_state.is_left_active { (chunks[2], false) } else { (chunks[0], true) };
        render_preview(f, area, preview, is_left);
    }

    // Hand the real geometry to the mouse handlers.
    app_state.table_area_left = chunks[0];
    app_state.table_area_right = chunks[2];
    app_state.viewport_start_left = offset_left;
    app_state.viewport_start_right = offset_right;

    chunks[0].height
}

/// Build only the rows visible in the viewport, returns (rows, start_offset)
fn build_viewport_rows(app_state: &AppState, is_left: bool, viewport_height: usize, columns: &[Column], name_width: u16) -> (Vec<Row<'static>>, usize) {
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

    let rename_input = match &app_state.dialog {
        Some(Dialog::Rename(input)) if app_state.is_left_active == is_left => Some(input),
        _ => None,
    };
    let is_renaming_current_side = rename_input.is_some();
    let border_cell = Cell::from(Span::styled("│", style_border()));

    let mut rows = Vec::with_capacity(end - start);
    // One moment for every row, so a relative date reads the same across them.
    let now = std::time::SystemTime::now();
    let date_format = app_state.options.date_format;
    // Name leaves the extension to its own column. Without one - turned off, or
    // given up to a narrow panel - it shows the whole name, or Cargo.lock and
    // Cargo.toml would read the same.
    let has_ext_column = columns.contains(&Column::Ext);

    for (index, child) in children.iter().enumerate().take(end).skip(start) {
        let is_renaming_current_item = is_renaming_current_side && (index == selected);
        let is_selected = selected_set.contains(&child.name_os);

        // Keep original icon, change color if selected
        let icon = row_icon(child.is_dir, app_state.options.icon_style);
        let file_color = color_for_extension(&child.extension);
        let text_color = if is_selected {
            palette().selected_marker
        } else if child.is_dir {
            palette().directory
        } else {
            file_color
        };
        let text_style = Style::default().fg(text_color);

        let (dir_prefix, dir_suffix) = if child.is_dir { ("[", "]") } else { ("", "") };

        let bracket_style = if is_selected { Style::default().fg(palette().selected_marker) } else { Style::default().fg(palette().directory_bracket) };

        let (name_cell, extension) = if is_renaming_current_item {
            // The mark survives Table's row_highlight_style, which adds to a
            // cell's modifiers rather than replacing them.
            let cursor_style = text_style.add_modifier(FIELD_CURSOR);
            // The brackets around a directory sit inside the same column.
            let field = name_width as usize - (dir_prefix.len() + dir_suffix.len()).min(name_width as usize);
            let mut spans = vec![Span::styled(dir_prefix, bracket_style)];
            if let Some(input) = rename_input {
                spans.extend(input.cursor_spans_within(field, text_style, cursor_style));
            }
            spans.push(Span::styled(dir_suffix, bracket_style));
            (Cell::from(Line::from(spans)), String::new())
        } else {
            (
                Cell::from(Line::from(vec![
                    Span::styled(dir_prefix, bracket_style),
                    Span::styled(printable_name(if has_ext_column { &child.name } else { &child.name_full }), text_style),
                    Span::styled(dir_suffix, bracket_style),
                ])),
                printable_name(&child.extension),
            )
        };

        // Get size - for directories, show calculated size if available
        let size = if child.is_dir && child.name != ".." {
            let path = child.path_in(current_dir);
            if let Some(&calculated_size) = app_state.dir_sizes.get(&path) {
                format_size(calculated_size)
            } else if let Some(so_far) = app_state.dir_size_so_far(&path) {
                // Still counting: what it has found so far, marked as not the
                // whole of it.
                if so_far == 0 { "…".to_string() } else { format!("{}…", format_size(so_far)) }
            } else {
                child.size.clone()
            }
        } else {
            child.size.clone()
        };

        let mut cells = vec![Cell::from(Span::styled(icon, Style::default().fg(text_color))), name_cell];
        for column in columns {
            let value = match column {
                Column::Ext => extension.clone(),
                Column::Size => size.clone(),
                Column::Modified => child.modified_at.map(|time| format_modified(time, date_format, now)).unwrap_or_default(),
                Column::Attributes => child.attributes.clone(),
            };
            cells.push(border_cell.clone());
            cells.push(Cell::from(Span::styled(value, text_style)));
        }
        rows.push(Row::new(cells));
    }

    (rows, start)
}

fn render_preview(f: &mut ratatui::Frame<'_>, area: Rect, preview: &crate::app::PreviewState, is_left: bool) {
    let block = Block::default().borders(if is_left { Borders::LEFT } else { Borders::RIGHT }).border_style(style_border());
    let inner = block.inner(area);

    clear(f, area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    let room = inner.height as usize - 1;
    let body = if preview.bytes.is_empty() {
        preview.lines.iter().take(room).map(|line| line.replace('\t', &" ".repeat(tab_width()))).collect()
    } else {
        hex_preview(&preview.bytes, inner.width, room)
    };

    let mut lines = vec![Line::from(Span::styled(format!(" {}", printable_name(&preview.label)), style_columns()))];
    lines.extend(body.into_iter().map(|line| Line::from(Span::styled(format!(" {line}"), style_file()))));

    f.render_widget(Paragraph::new(lines), inner);
}

/// The hexdump layouts the preview can fall back through, widest first: sixteen
/// bytes a row as the viewer writes them, then eight, then eight with no offset
/// at all - by then it is a third of the row - and finally four, which fits the
/// narrowest panel fm84 will draw.
///
/// Four digits of offset throughout, where the viewer writes eight: a preview
/// reads PREVIEW_HEX_BYTES and stops, so the other four would be zeroes in
/// every row, and dropping them is what lets a 160-column terminal show all
/// sixteen bytes.
const PREVIEW_HEX_LAYOUTS: [(usize, usize); 4] = [(16, 4), (8, 4), (8, 0), (4, 0)];

// Which holds only while a preview reads little enough to be offset in four.
const _: () = assert!(crate::constants::PREVIEW_HEX_BYTES <= 0xffff);

/// The head of a binary file as a hexdump, in the widest layout the pane holds.
/// Laid out here rather than where the bytes were read, because a pane changes
/// width without the cursor moving, and the cursor moving is what gathers a
/// preview again.
pub(super) fn hex_preview(bytes: &[u8], width: u16, rows: usize) -> Vec<String> {
    // Every preview line opens with a space.
    let room = width.saturating_sub(1) as usize;
    let (per_line, digits) = PREVIEW_HEX_LAYOUTS
        .into_iter()
        .find(|&(per_line, digits)| crate::viewer::hex_row_width(per_line, digits) <= room)
        .unwrap_or(PREVIEW_HEX_LAYOUTS[PREVIEW_HEX_LAYOUTS.len() - 1]);

    bytes.chunks(per_line).take(rows).enumerate().map(|(index, chunk)| crate::viewer::hex_row(index * per_line, chunk, per_line, digits)).collect()
}

/// The headings, the one the panels are sorted by marked with which way.
/// Every column is wide enough for its title and the arrow.
fn make_header_row(columns: &[Column], options: &Options) -> Row<'static> {
    let arrow = if options.sort_descending { " ↓" } else { " ↑" };
    let heading = |title: &str, key: Option<SortKey>| {
        let title = if key == Some(options.sort_key) { format!("{title}{arrow}") } else { title.to_string() };
        Cell::from(Span::styled(title, style_columns()))
    };
    let mut cells = vec![Cell::from(Span::styled("", style_columns())), heading("Name", Some(SortKey::Name))];
    for column in columns {
        cells.push(Cell::from(Span::styled("", style_columns())));
        cells.push(heading(column.title(), column.sort_key()));
    }
    Row::new(cells)
}
