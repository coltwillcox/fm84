use crate::app::{AppState, Dialog, PromptKind, Screen, TextInput, TransferKind};
use crate::constants::*;
use crate::display::{palette, tab_width};
use crate::options::{Clock, DateFormat, IconStyle, OPTION_ROWS, Options};
use crate::strip::{ARROW, Strip, StripHit};
use crate::utils::*;
use crate::viewer::ViewMode;
use chrono::Local;
use ratatui::{
    Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

// The styles used throughout rendering, from whichever palette is showing.
fn style_border() -> Style { Style::new().fg(palette().border) }
fn style_title() -> Style { Style::new().fg(palette().title) }
fn style_columns() -> Style { Style::new().fg(palette().columns) }
fn style_file() -> Style { Style::new().fg(palette().file) }
fn style_dir() -> Style { Style::new().fg(palette().directory) }
fn style_dir_dark() -> Style { Style::new().fg(palette().directory_dark) }
fn style_selection() -> Style { Style::new().bg(palette().selected_background_inactive) }
// The other matches of a search. Marked rather than coloured, so it reads in
// every theme and leaves the syntax colours underneath alone.
fn style_match() -> Style { Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD) }

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

pub fn render_ui<B: Backend>(terminal: &mut Terminal<B>, app_state: &mut AppState) {
    // Update cached clock
    let current_time = match app_state.options.clock {
        Clock::Hours24 => Local::now().format(" %H:%M:%S ").to_string(),
        Clock::Hours12 => Local::now().format(" %I:%M:%S %p ").to_string(),
        Clock::Off => String::new(),
    };
    if app_state.cached_clock != current_time {
        app_state.cached_clock = current_time;
    }

    let _ = terminal.draw(|f| {
        let area = f.area();
        paint_background(f, area);

        // Guard against terminal too small to render
        if area.height < 10 || area.width < 30 {
            let msg = Paragraph::new("Terminal too small")
                .alignment(Alignment::Center)
                .style(style_title());
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

        render_top_panel(f, chunks_main[0], app_state);
        render_path_bar(f, chunks_main[1], &app_state.dir_left, &app_state.dir_right, area.width, app_state.is_left_active);
        match app_state.screen {
            Screen::Viewer(_) => {
                let (height, width, content_area) = render_viewer(f, chunks_main[2], app_state);
                app_state.viewer_viewport_height = height;
                app_state.viewer_viewport_width = width;
                app_state.viewer_content_area = content_area;
            }
            Screen::Editor(_) => app_state.editor_viewport_height = render_editor(f, chunks_main[2], app_state),
            Screen::Panels => app_state.page_size = render_file_tables(f, chunks_main[2], app_state),
        }
        render_bottom_panel(f, chunks_main[3], app_state);
        render_fkey_bar(f, chunks_main[4]);
        render_detail(f, chunks_main[4].inner(Margin { vertical: 0, horizontal: 2 }), app_state);

        // The rename field, the prompt and the drive picker are drawn where
        // they sit, in the panel, the status bar and the drive strip.
        match &app_state.dialog {
            Some(Dialog::Help) => render_help_popup(f, area),
            Some(Dialog::Options { editing }) => render_options_popup(f, area, app_state, editing.as_ref()),
            Some(Dialog::Create { is_dir, input }) => render_create_popup(f, area, *is_dir, input),
            Some(Dialog::Delete { items, to_trash }) => render_delete_popup(f, area, items, *to_trash),
            Some(Dialog::Transfer { items, is_copy }) => render_copy_move_popup(f, area, items, *is_copy),
            Some(Dialog::Overwrite(prompt)) => render_overwrite_popup(f, area, prompt),
            Some(Dialog::LargeFile(large)) => render_large_file_popup(f, area, large),
            Some(Dialog::SaveChanges { .. }) => render_editor_save_popup(f, area),
            Some(Dialog::Rename(_) | Dialog::Prompt(..) | Dialog::DrivePicker { .. }) | None => {}
        }
        // Over everything: a job, which holds the keys while it runs, or an
        // error, which holds them until it is put away.
        if app_state.job.is_some() {
            render_transfer_popup(f, area, app_state);
        } else if let Some(message) = &app_state.error {
            render_error_popup(f, area, message);
        }
    });
}

/// A path's last component, lossy where it has to be and with anything that
/// would reach the terminal taken out of it.
fn shown_name(path: &std::path::Path) -> String {
    path.file_name().map(|name| printable_name(&name.to_string_lossy())).unwrap_or_default()
}

/// Fill an area with the theme's background, when F11 has it painted. Cells
/// drawn over it keep it unless they set their own.
fn paint_background(f: &mut ratatui::Frame<'_>, area: Rect) {
    if let Some(background) = crate::display::background() {
        f.render_widget(Block::new().style(Style::new().bg(background)), area);
    }
}

/// Blank an area for a popup to draw on. Clear alone would hand it back to
/// the terminal's background, showing through a painted theme.
fn clear(f: &mut ratatui::Frame<'_>, area: Rect) {
    f.render_widget(Clear, area);
    paint_background(f, area);
}

/// The left / separator / right split every full-width row shares, so the rows
/// that line up with the panels all derive their columns the same way.
fn panel_split(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Percentage(50)])
        .split(area)
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

fn logo_icon(style: IconStyle) -> &'static str {
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
fn drive_strip(app_state: &mut AppState, is_left: bool, area: Rect) -> Line<'static> {
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
fn tab_label(dir: &Path) -> String {
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
fn tab_strip(app_state: &mut AppState, is_left: bool, area: Rect) -> Line<'static> {
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

fn render_top_panel(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) {
    let cached_clock = app_state.cached_clock.as_str();
    let logo = Span::styled(format!(" {} ", logo_icon(app_state.options.icon_style)), style_title());
    let title = Span::styled(format!(" {} v{} ", TITLE, VERSION), style_title());
    let clock = Span::styled(cached_clock, style_title());

    let block_top = Block::default()
        .title_top(Line::from(logo).left_aligned())
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

fn render_path_bar(f: &mut ratatui::Frame<'_>, area: Rect, dir_left: &std::path::Path, dir_right: &std::path::Path, total_width: u16, is_left_active: bool) {
    let length_left = ((total_width as usize).saturating_sub(3)) / 2;
    let length_right = ((total_width as usize).saturating_sub(2)) / 2;

    let path_left = limit_path_string(dir_left, length_left.saturating_sub(8));
    let path_right = limit_path_string(dir_right, length_right.saturating_sub(8));

    let (color_left, color_right) = if is_left_active {
        (style_dir(), style_dir_dark())
    } else {
        (style_dir_dark(), style_dir())
    };

    let border_line = vec![
        Span::styled("├──", style_border()),
        Span::styled(format!(" {} ", path_left), color_left),
        Span::styled(format!("{}─┬──", "─".repeat(length_left.saturating_sub(display_width(&path_left).saturating_add(5)))), style_border()),
        Span::styled(format!(" {} ", path_right), color_right),
        Span::styled(format!("{}─┤", "─".repeat(length_right.saturating_sub(display_width(&path_right).saturating_add(5)))), style_border()),
    ];

    f.render_widget(Paragraph::new(Line::from(border_line)), area);
}

fn render_file_tables(f: &mut ratatui::Frame<'_>, chunk: Rect, app_state: &mut AppState) -> u16 {
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

    let header = make_header_row(&columns);

    // Build only visible rows for left panel
    let field = name_width(&app_state.options, chunks[0].width);
    let (rows_left, offset_left) = build_viewport_rows(app_state, true, viewport_height, &columns, field);
    let mut state_left_view = TableState::default();
    state_left_view.select(app_state.state_left.selected().map(|s| s.saturating_sub(offset_left)));

    let table_left = Table::new(rows_left, widths.clone())
        .block(Block::default().borders(Borders::LEFT).border_style(style_border()))
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
    let separator_vertical = Paragraph::new(Text::raw(&app_state.cached_separator)).style(style_border());
    f.render_widget(separator_vertical, chunks[1]);

    // Build only visible rows for right panel
    let (rows_right, offset_right) = build_viewport_rows(app_state, false, viewport_height, &columns, field);
    let mut state_right_view = TableState::default();
    state_right_view.select(app_state.state_right.selected().map(|s| s.saturating_sub(offset_right)));

    let table_right = Table::new(rows_right, widths)
        .block(Block::default().borders(Borders::RIGHT).border_style(style_border()))
        .header(header)
        .row_highlight_style(table_style(!app_state.is_left_active))
        .column_spacing(1);
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
fn build_viewport_rows(
    app_state: &AppState,
    is_left: bool,
    viewport_height: usize,
    columns: &[Column],
    name_width: u16,
) -> (Vec<Row<'static>>, usize) {
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

        let bracket_style = if is_selected {
            Style::default().fg(palette().selected_marker)
        } else {
            Style::default().fg(palette().directory_bracket)
        };

        let (name_cell, extension) = if is_renaming_current_item {
            // REVERSED survives Table row_highlight_style override
            let cursor_style = text_style.add_modifier(Modifier::REVERSED);
            // The brackets around a directory sit inside the same column.
            let field = name_width as usize - (dir_prefix.len() + dir_suffix.len()).min(name_width as usize);
            let mut spans = vec![Span::styled(dir_prefix, bracket_style)];
            if let Some(input) = rename_input {
                spans.extend(input.cursor_spans_within(field, text_style, cursor_style));
            }
            spans.push(Span::styled(dir_suffix, bracket_style));
            (Cell::from(Line::from(spans)), String::new())
        } else {
            (Cell::from(Line::from(vec![
                Span::styled(dir_prefix, bracket_style),
                Span::styled(printable_name(if has_ext_column { &child.name } else { &child.name_full }), text_style),
                Span::styled(dir_suffix, bracket_style),
            ])), printable_name(&child.extension))
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

        let mut cells = vec![
            Cell::from(Span::styled(icon, Style::default().fg(text_color))),
            name_cell,
        ];
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
    let block = Block::default()
        .borders(if is_left { Borders::LEFT } else { Borders::RIGHT })
        .border_style(style_border());
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
fn hex_preview(bytes: &[u8], width: u16, rows: usize) -> Vec<String> {
    // Every preview line opens with a space.
    let room = width.saturating_sub(1) as usize;
    let (per_line, digits) = PREVIEW_HEX_LAYOUTS
        .into_iter()
        .find(|&(per_line, digits)| crate::viewer::hex_row_width(per_line, digits) <= room)
        .unwrap_or(PREVIEW_HEX_LAYOUTS[PREVIEW_HEX_LAYOUTS.len() - 1]);

    bytes
        .chunks(per_line)
        .take(rows)
        .enumerate()
        .map(|(index, chunk)| crate::viewer::hex_row(index * per_line, chunk, per_line, digits))
        .collect()
}

fn make_header_row(columns: &[Column]) -> Row<'static> {
    let mut cells = vec![
        Cell::from(Span::styled("", style_columns())),
        Cell::from(Span::styled("Name", style_columns())),
    ];
    for column in columns {
        cells.push(Cell::from(Span::styled("", style_columns())));
        cells.push(Cell::from(Span::styled(column.title(), style_columns())));
    }
    Row::new(cells)
}

fn render_viewer(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) -> (usize, usize, Rect) {
    if let Some(viewer_state) = app_state.viewer() {
        let filename = shown_name(&viewer_state.file_path);
        let prefix = if viewer_state.from_edit { "Edit" } else { "View" };
        let title = format!(" {}: {} ", prefix, filename);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, style_title())).centered())
            .borders(Borders::ALL)
            .border_style(style_border());

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Hex rows carry their own offset, so the gutter is not needed there,
        // and line numbers mean nothing down the side of a picture. F11 can
        // turn it off for text too.
        let no_gutter = viewer_state.mode != ViewMode::Text || !app_state.options.line_numbers;
        let line_num_width = if no_gutter {
            0
        } else {
            (viewer_state.total_lines.to_string().len() as u16).max(3) + 2
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(line_num_width), Constraint::Min(0)])
            .split(inner_area);

        let viewport_height = inner_area.height as usize;
        let start = viewer_state.scroll_offset;
        let end = (start + viewport_height).min(viewer_state.total_lines);
        // Line numbers, unless the gutter is zero-width - it must not be
        // formatted at all then.
        if !no_gutter {
            let num_width = (line_num_width as usize).saturating_sub(1);
            let line_numbers: Vec<Line> = (start..end)
                .map(|line_num| Line::from(Span::styled(
                    format!("{:>width$} ", line_num + 1, width = num_width),
                    style_columns(),
                )))
                .collect();
            let line_number_para = Paragraph::new(line_numbers)
                .style(Style::default().bg(crate::display::gutter()));
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
            let binary_msg = Paragraph::new("Binary file detected. Press Esc to return.")
                .alignment(Alignment::Center)
                .style(style_title());
            f.render_widget(binary_msg, chunks[1]);
        } else {
            // Both modes render from the same line text, so the selection and
            // the copy see exactly what is on screen.
            let selection = viewer_state.selected_range();
            // Not on a picture, whose characters only happen to be letters.
            let marking = app_state.find_shown && viewer_state.mode != ViewMode::Image;
            let needle = marking.then(|| crate::find::Needle::new(&app_state.find_term)).flatten();
            let content_lines: Vec<Line> = (start..end)
                .map(|index| {
                    let text = viewer_state.line_text(index);
                    let width = text.chars().count();
                    let matches = needle.as_ref().map(|needle| needle.find_all(&text)).unwrap_or_default();
                    let mut spans = match viewer_state.image_colors.get(index) {
                        Some(colors) if viewer_state.mode == ViewMode::Image => {
                            colored_spans(&text, colors, viewer_state.image_backgrounds.get(index).map(Vec::as_slice))
                        }
                        _ => vec![Span::styled(text, style_file())],
                    };
                    for (from, to) in matches {
                        spans = overlay_range(spans, from, to, style_match());
                    }

                    if let Some(((first_line, first_col), (last_line, last_col))) = selection
                        && (first_line..=last_line).contains(&index)
                    {
                        let to = if index == last_line { last_col.min(width) } else { width };
                        let from = if index == first_line { first_col.min(to) } else { 0 };
                        spans = overlay_range(spans, from, to, style_selection());
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

/// A `width` x `height` area in the middle of `area`, shrunk to fit inside it.
fn centered(area: Rect, width: usize, height: usize) -> Rect {
    let width = width.min(area.width as usize) as u16;
    let height = height.min(area.height as usize) as u16;
    Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height)
}

/// A line of image characters, one span per run of the same colour.
fn colored_spans(text: &str, colors: &[ratatui::style::Color], backgrounds: Option<&[ratatui::style::Color]>) -> Vec<Span<'static>> {
    // A run has to agree on both colours, so a background breaks one wherever
    // either changes. Since the background is derived from the same pixel, that
    // is the same place the foreground already broke.
    let paint = |(fg, bg): (ratatui::style::Color, Option<ratatui::style::Color>)| match bg {
        Some(bg) => Style::new().fg(fg).bg(bg),
        None => Style::new().fg(fg),
    };

    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_color: Option<(ratatui::style::Color, Option<ratatui::style::Color>)> = None;
    for (index, character) in text.chars().enumerate() {
        let Some(&color) = colors.get(index) else {
            break;
        };
        let pair = (color, backgrounds.and_then(|row| row.get(index)).copied());
        if let Some(previous) = run_color
            && previous != pair
        {
            spans.push(Span::styled(std::mem::take(&mut run), paint(previous)));
        }
        run_color = Some(pair);
        run.push(character);
    }
    if let Some(pair) = run_color {
        spans.push(Span::styled(run, paint(pair)));
    }
    spans
}

/// Visual column of a character index, with tabs expanded the way the editor
/// draws them.
fn visual_column(line: &str, char_col: usize) -> usize {
    line.chars().take(char_col).map(|c| if c == '\t' { tab_width() } else { 1 }).sum()
}

/// Merge `extra` into the styles covering visual columns [from, to), splitting
/// spans at the boundaries so the syntax colours survive underneath.
fn overlay_range(spans: Vec<Span<'static>>, from: usize, to: usize, extra: Style) -> Vec<Span<'static>> {
    if from >= to {
        return spans;
    }

    let mut result = Vec::with_capacity(spans.len() + 2);
    let mut column = 0;
    for span in spans {
        let text = span.content.into_owned();
        let length = text.chars().count();
        let span_end = column + length;

        if span_end <= from || column >= to {
            result.push(Span::styled(text, span.style));
        } else {
            let characters: Vec<char> = text.chars().collect();
            let head = from.saturating_sub(column).min(length);
            let tail = to.saturating_sub(column).min(length);
            if head > 0 {
                result.push(Span::styled(characters[..head].iter().collect::<String>(), span.style));
            }
            result.push(Span::styled(characters[head..tail].iter().collect::<String>(), span.style.patch(extra)));
            if tail < length {
                result.push(Span::styled(characters[tail..].iter().collect::<String>(), span.style));
            }
        }
        column = span_end;
    }
    result
}

/// Drop the first `columns` display columns of a line, for horizontal
/// scrolling. Paragraph::scroll would do it, but it takes a u16: on a
/// minified file, whose lines run past 65,535 columns, the offset wrapped
/// round and the view jumped back to the start. A wide character cut in two
/// leaves a space where its second half was.
fn skip_columns(spans: Vec<Span<'static>>, columns: usize) -> Vec<Span<'static>> {
    if columns == 0 {
        return spans;
    }
    let mut left = columns;
    let mut result = Vec::with_capacity(spans.len());
    for span in spans {
        if left == 0 {
            result.push(span);
            continue;
        }
        let mut kept = String::new();
        for character in span.content.chars() {
            if left == 0 {
                kept.push(character);
                continue;
            }
            let width = UnicodeWidthChar::width(character).unwrap_or(0);
            if width > left {
                kept.extend(std::iter::repeat_n(' ', width - left));
                left = 0;
            } else {
                left -= width;
            }
        }
        if !kept.is_empty() {
            result.push(Span::styled(kept, span.style));
        }
    }
    result
}

/// Draw the cursor cell, padding out to it when it sits past end-of-line.
fn place_cursor(mut spans: Vec<Span<'static>>, column: usize, style: Style) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    if column < total {
        return overlay_range(spans, column, column + 1, style);
    }
    if column > total {
        spans.push(Span::raw(" ".repeat(column - total)));
    }
    spans.push(Span::styled(" ", style));
    spans
}

fn render_editor(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &mut AppState) -> usize {
    let line_numbers = app_state.options.line_numbers;
    let (viewport_height, content_area) = if let Screen::Editor(editor_state) = &mut app_state.screen {
        let filename = shown_name(&editor_state.file_path);
        let modified = if editor_state.modified { " [Modified]" } else { "" };
        let title = format!(" Edit: {}{} ", filename, modified);

        let border_block = Block::default()
            .title(Line::from(Span::styled(title, style_title())).centered())
            .borders(Borders::ALL)
            .border_style(style_border());

        let inner_area = border_block.inner(area);
        f.render_widget(border_block, area);

        // Calculate line number gutter width. With the numbers turned off it is
        // zero wide, and the click mapping follows, since it reads the content
        // area as drawn.
        let total_lines = editor_state.lines.len();
        let line_num_width = if line_numbers { (total_lines.to_string().len() as u16).max(3) + 2 } else { 0 };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(line_num_width), Constraint::Min(0)])
            .split(inner_area);

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
                Line::from(Span::styled(
                    format!("{:>width$} ", line_num + 1, width = num_width),
                    style,
                ))
            })
            .collect();
        if line_numbers {
            let line_number_para = Paragraph::new(numbers)
                .style(Style::default().bg(crate::display::gutter()));
            f.render_widget(line_number_para, chunks[0]);
        }

        // Auto-scroll to keep cursor visible (disabled during mouse scrolling)
        if editor_state.auto_scroll {
            let visual_cursor_col: usize = editor_state.lines[editor_state.cursor_line]
                .chars()
                .take(editor_state.cursor_col)
                .map(|c| if c == '\t' { tab_width() } else { 1 })
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

        // Syntax colours first, then the selection over them, then the cursor.
        let has_highlighting = !editor_state.highlighted_lines.is_empty();
        let cursor_style = Style::default().fg(palette().selected_foreground).bg(palette().selected_background);
        let selection = editor_state.selection();
        let needle = app_state.find_shown.then(|| crate::find::Needle::new(&app_state.find_term)).flatten();
        let mut content_lines: Vec<Line> = Vec::with_capacity(end - start);

        for (idx, line) in editor_state.lines[start..end].iter().enumerate() {
            let actual_line_idx = start + idx;

            let mut spans: Vec<Span<'static>> =
                if has_highlighting && actual_line_idx < editor_state.highlighted_lines.len() {
                    editor_state.highlighted_lines[actual_line_idx]
                        .iter()
                        .map(|span| Span::styled(printable_line(&span.content), span.style))
                        .collect()
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
                let to = if actual_line_idx == last_line {
                    visual_column(line, last_col)
                } else {
                    visual_column(line, line.chars().count())
                };
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
    let text = printable_name(&input.text);
    let characters: Vec<char> = text.chars().collect();
    let cursor = input.cursor.min(characters.len());
    let before: String = characters[..cursor].iter().collect();
    let under: String = characters.get(cursor).map_or(" ".to_string(), char::to_string);
    let after: String = characters.get(cursor + 1..).map_or(String::new(), |rest| rest.iter().collect());
    let used = 2 + display_width(label) + display_width(&before) + display_width(&under) + display_width(&after) + 1 + 1;

    let line = vec![
        Span::styled("├─", style_border()),
        Span::styled(label.to_string(), style),
        Span::styled(before, style),
        Span::styled(under, style.add_modifier(Modifier::REVERSED)),
        Span::styled(after, style),
        Span::styled(" ", style),
        Span::styled("─".repeat((area.width as usize).saturating_sub(used)), style_border()),
        Span::styled("┤", style_border()),
    ];
    f.render_widget(Paragraph::new(Line::from(line)), area);
}

fn render_status_bar(f: &mut ratatui::Frame<'_>, area: Rect, text: String, style: Style) {
    let text_len = display_width(&text);
    let status_line = vec![
        Span::styled("├─", style_border()),
        Span::styled(text, style),
        Span::styled(
            "─".repeat((area.width as usize).saturating_sub(text_len).saturating_sub(3)),
            style_border(),
        ),
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

fn render_bottom_panel(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let status_style = style_title().bg(palette().selected_background);

    if let Some(Dialog::Prompt(kind, input)) = &app_state.dialog {
        let label = match kind {
            PromptKind::Find => " Find: ",
            PromptKind::GoToLine => " Go to line: ",
            PromptKind::Select => " Select: ",
            PromptKind::Deselect => " Deselect: ",
        };
        render_prompt_bar(f, area, label, input, status_style);
        return;
    }
    if !matches!(app_state.screen, Screen::Panels)
        && let Some(note) = &app_state.find_note
    {
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
    } else if let Some(viewer_state) = app_state.viewer() {
        {
            let filename = shown_name(&viewer_state.file_path);
            let line_seg = format!("Line {}/{}", viewer_state.scroll_offset + 1, viewer_state.total_lines);
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
            let item_size = |c: &crate::app::Item| -> u64 {
                if c.is_dir {
                    dir_sizes.get(&c.path_in(current_dir)).copied().unwrap_or(0)
                } else {
                    c.size_bytes
                }
            };
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

        let (left_style, right_style) = if app_state.is_left_active {
            (style_title(), style_dir_dark())
        } else {
            (style_dir_dark(), style_title())
        };

        // Disk usage sits at the far end of each panel's dash run, dropping to a
        // shorter form and then out entirely as the terminal narrows.
        let left_disk = disk_readout(app_state.disk_left, left_pad);
        let right_disk = disk_readout(app_state.disk_right, right_pad);
        let dashes = |pad: usize, disk: &Option<String>| {
            pad - disk.as_ref().map_or(0, |text| display_width(text) + 1)
        };

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

/// The F-key hints, dropped from the end when the terminal cannot hold them
/// whole - a half-drawn label reads worse than a missing one.
const FKEY_LABELS: [&str; 12] = [
    " F1 Help ",
    " F2 Rename ",
    " F3 View ",
    " F4 Edit ",
    " F5 Copy ",
    " F6 Move ",
    " F7 Create ",
    " F8 Delete ",
    " F9 Terminal ",
    " F10 Quit ",
    " F11 Options ",
    " F12 Preview ",
];

/// The two rows inside the bottom block, which the F-key labels leave empty.
/// They spell out what the columns cannot hold: the whole name however long,
/// the exact byte count rather than a rounded one, the second on the timestamp,
/// who owns it, and where a symlink points - which is shown nowhere else.
fn render_detail(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
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
    let cost = |parts: &[(&str, Style)]| -> usize {
        parts.iter().filter(|(text, _)| !text.is_empty()).map(|(text, _)| display_width(text) + gap.len()).sum()
    };
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
fn tail_of(text: &str, room: usize) -> String {
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

fn render_fkey_bar(f: &mut ratatui::Frame<'_>, area: Rect) {
    let mut block_bottom = Block::default()
        .borders(Borders::LEFT | Borders::BOTTOM | Borders::RIGHT)
        .border_style(style_border());

    // Two corners, then each label with a dash between. The last label's
    // trailing space can fall off the end unnoticed, hence the one spare column.
    let mut used = 2;
    for (index, label) in FKEY_LABELS.iter().enumerate() {
        let needed = used + display_width(label) + usize::from(index > 0);
        if needed > area.width as usize + 1 {
            break;
        }
        used = needed;
        block_bottom = block_bottom.title_bottom(Line::from(Span::styled(*label, style_title())).centered());
    }

    f.render_widget(block_bottom, area);
}

/// The area inside a popup's border that its body is laid out in.
fn popup_inner(area: Rect) -> Rect {
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
fn popup_body(f: &mut ratatui::Frame<'_>, area: Rect, lines: Vec<Line<'static>>) {
    let inner = popup_inner(area);
    let room = inner.height as usize;

    let blocks = fit_rows(lines.into_iter().map(|line| wrap_line(line, inner.width as usize)).collect(), room);
    let rows: usize = blocks.iter().map(Vec::len).sum();
    let airy = (rows + blocks.len()).saturating_sub(1);
    let mut body: Vec<Line> = Vec::with_capacity(airy.max(rows));
    for (index, block) in blocks.into_iter().enumerate() {
        if index > 0 && airy <= room {
            body.push(Line::from(""));
        }
        body.extend(block);
    }

    // Sit the block in the middle of whatever is left over.
    let padding = room.saturating_sub(body.len()) / 2;
    let mut out = vec![Line::from(""); padding];
    out.extend(body);
    f.render_widget(Paragraph::new(out).alignment(Alignment::Center), inner);
}

/// The rows a popup line takes at `width`. Only a line in one style is
/// wrapped, which is every long one a popup has; a line built from several -
/// a text field with its cursor - keeps to its own width already.
fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 || line.width() <= width || line.spans.len() != 1 {
        return vec![line];
    }
    let style = line.spans[0].style;
    wrap_text(&line.spans[0].content, width)
        .into_iter()
        .map(|row| Line { spans: vec![Span::styled(row, style)], style: line.style, alignment: line.alignment })
        .collect()
}

/// Text broken into rows of at most `width` columns, at the spaces between
/// words, and inside a word only when it is wider than a row by itself.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
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
fn fit_rows(mut blocks: Vec<Vec<Line<'static>>>, room: usize) -> Vec<Vec<Line<'static>>> {
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

fn render_error_popup(f: &mut ratatui::Frame<'_>, area: Rect, message: &str) {
    let popup_area = centered_rect(60, 20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Error ", style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    popup_body(f, popup_area, vec![Line::from(Span::styled(printable_name(message), style_title()))]);
}

fn render_help_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let help_lines = vec![
        "F1 - This help",
        "F2 - Rename folder/file",
        "F3 - View file (X text/hex/image)",
        "  F fit/fill, +/- zoom",
        "F4 - Edit file (Ctrl+S/F2 save)",
        "  Ctrl+F find, F3/Shift+F3 next/prev",
        "  Ctrl+G go to line",
        "  Ctrl+Z undo, Ctrl+Y redo, Ctrl+X/C/V",
        "  Shift+arrows select, Ctrl+A all",
        "F5 - Copy to other panel",
        "F6 - Move to other panel",
        "F7 - Create directory",
        "Shift+F4 - Create file",
        "F8 - Move to trash",
        "  Shift+F8 delete permanently",
        "F9 - Open terminal",
        "F10 - Quit",
        "F11 - Options (Alt+F1/F2 drives)",
        "F12 - Preview in other panel",
        "Ctrl+Left/Right - Dir to that panel",
        "Ctrl+T/Ctrl+W - New/close tab",
        "  Ctrl+PgUp/PgDn, Alt+1..9 switch",
        "Space - Select/deselect file",
        "+/- select/deselect by pattern, * invert",
        "  Alt+* invert with directories",
        "Ctrl+R - Reload both panels",
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
        .title(Line::from(Span::styled(" Help/About ", style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let inner = popup_area.inner(Margin { vertical: 2, horizontal: 2 });
    let max_len = help_lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let lines: Vec<Line> = help_lines.iter()
        .map(|&text| Line::from(Span::styled(format!("{:<width$}", text, width = max_len), style_title())))
        .collect();
    let help_para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(help_para, inner);
}

fn render_options_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState, editing: Option<&TextInput>) {
    let inner_width = 60.min(area.width as usize).saturating_sub(4);
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
    let popup_area = centered(area, inner_width + 4, list.len() + 6);
    let mut popup_block = Block::default()
        .title(Line::from(Span::styled(" Options ", style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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
        let typed = input.cursor_spans(cursor_style, cursor_style.add_modifier(Modifier::REVERSED));
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
        Line::from(vec![
            Span::styled(format!("{}{}", label, " ".repeat(gap)), style_dir()),
            Span::styled(format!("{} ", value), style_columns()),
        ])
    }
}

fn render_create_popup(f: &mut ratatui::Frame<'_>, area: Rect, is_dir: bool, input: &TextInput) {
    let popup_area = centered_rect(60, 20, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(if is_dir { " Create Directory " } else { " Create File " }, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // Show input with block cursor (REVERSED so it's visible against paragraph bg)
    let cursor_style = style_title().add_modifier(Modifier::REVERSED);
    // Padded out to the full width so the highlight reads as an input field.
    // Paragraph styles the spans rather than the row, so a bare line would
    // colour only the characters typed so far.
    let typed = input.cursor_spans(style_title(), cursor_style);
    let width = popup_inner(popup_area).width as usize;
    let typed_width: usize = typed.iter().map(|span| display_width(&span.content)).sum();
    let left = width.saturating_sub(typed_width) / 2;
    let mut spans = vec![Span::raw(" ".repeat(left))];
    spans.extend(typed);
    spans.push(Span::raw(" ".repeat(width.saturating_sub(left + typed_width))));
    let input_line = Line::from(spans).style(style_title().bg(palette().selected_background));
    popup_body(
        f,
        popup_area,
        vec![input_line, Line::from(Span::styled("Enter - Create    Esc - Cancel", style_columns()))],
    );
}

fn render_delete_popup(f: &mut ratatui::Frame<'_>, area: Rect, items: &[(PathBuf, bool)], to_trash: bool) {
    let count = items.len();
    let popup_area = centered_rect(60, 30, area);

    // Which kind of delete it is, said plainly: one can be undone and the
    // other cannot.
    let verb = if to_trash { "Move to trash" } else { "Delete" };
    let title = if count == 1 {
        let item_type = if items[0].1 { "directory" } else { "file" };
        format!(" {verb}: {item_type} ")
    } else {
        format!(" {verb}: {count} items ")
    };

    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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

/// Asks whether a copy or move may write over the names it found taken.
fn render_overwrite_popup(f: &mut ratatui::Frame<'_>, area: Rect, prompt: &crate::app::OverwritePrompt) {
    let count = prompt.taken.len();
    let popup_area = centered_rect(60, 30, area);
    let verb = if prompt.is_copy { "Copy" } else { "Move" };
    let popup_block = Block::default()
        .title(Line::from(Span::styled(format!(" {verb}: overwrite? "), style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    let name = |path: &std::path::PathBuf| shown_name(path);
    let mut lines = Vec::new();
    if count == 1 {
        lines.push(Line::from(Span::styled(format!("\"{}\" already exists", name(&prompt.taken[0])), style_title())));
    } else {
        lines.push(Line::from(Span::styled(format!("{count} items already exist"), style_title())));
        let names: Vec<String> = prompt.taken.iter().map(name).collect();
        lines.push(Line::from(Span::styled(names.join(", "), style_file())));
    }
    // What yes does to a directory is less obvious than what it does to a file.
    if prompt.taken.iter().any(|path| path.is_dir()) {
        lines.push(Line::from(Span::styled("Directories are merged, replacing what clashes inside", style_file())));
    }
    lines.push(Line::from(Span::styled("Y / Enter - Overwrite    N / Esc - Cancel", style_columns())));
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
fn render_transfer_popup(f: &mut ratatui::Frame<'_>, area: Rect, app_state: &AppState) {
    let Some(job) = &app_state.job else {
        return;
    };
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
        }
    };
    let popup_area = centered_rect(60, 30, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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
            format!(
                "{} of {} at {}/s    {hint}",
                format_size(job.done),
                format_size(total),
                format_size(rate as u64)
            )
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
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        popup_area.inner(Margin { vertical: 1, horizontal: 2 }),
    );
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
    };
    let popup_area = centered_rect(60, 30, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).wrap(ratatui::widgets::Wrap { trim: true }),
        popup_area.inner(Margin { vertical: 1, horizontal: 2 }),
    );
}

/// Unified copy/move popup. `is_copy` = true for F5 copy, false for F6 move.
fn render_copy_move_popup(f: &mut ratatui::Frame<'_>, area: Rect, items: &[(PathBuf, PathBuf, bool)], is_copy: bool) {
    let popup_area = centered_rect(70, 35, area);
    let verb = if is_copy { "Copy" } else { "Move" };
    let count = items.len();

    let title = if count == 1 {
        let item_type = if items[0].2 { "directory" } else { "file" };
        format!(" {} {} ", verb, item_type)
    } else {
        format!(" {} {} items ", verb, count)
    };

    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // Source info
    let source_msg = if count == 1 {
        let source_name = shown_name(&items[0].0);
        format!("{} \"{}\"", verb, source_name)
    } else {
        let names: Vec<String> = items.iter().map(|(src, _, _)| shown_name(src)).collect();
        format!("{} {} items: {}", verb, count, names.join(", "))
    };
    // Destination directory
    let dest_dir = items[0].1.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let dest_display = limit_path_string(&dest_dir, popup_area.width as usize - 10);
    popup_body(
        f,
        popup_area,
        vec![
            Line::from(Span::styled(source_msg, style_title())),
            Line::from(Span::styled(format!("to: {dest_display}"), style_file())),
            Line::from(Span::styled("Y / Enter - Yes    N / Esc - No", style_columns())),
        ],
    );
}

fn render_large_file_popup(f: &mut ratatui::Frame<'_>, area: Rect, large: &crate::app::LargeFile) {
    let name = shown_name(&large.path);
    let verb = if large.is_edit { "Edit" } else { "View" };

    let popup_area = centered_rect(60, 30, area);
    let title = if large.dimensions.is_some() { " Large Picture " } else { " Large File " };
    let popup_block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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

fn render_editor_save_popup(f: &mut ratatui::Frame<'_>, area: Rect) {
    let popup_area = centered_rect(60, 25, area);
    let popup_block = Block::default()
        .title(Line::from(Span::styled(" Unsaved Changes ", style_title())).centered())
        .borders(Borders::ALL)
        .style(style_border());

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A tab is clicked where it is drawn, and a long name is cut short.
    #[test]
    fn tabs_are_drawn_and_clicked_where_they_are() {
        assert_eq!(tab_label(Path::new("/")), "/");
        assert_eq!(tab_label(Path::new("/home/colt/src")), "src");
        assert_eq!(tab_label(Path::new("/tmp/a-rather-long-directory-name")), "a-rather-long-d…");

        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.dir_left = "/tmp/b".into();
        app_state.tabs_left = crate::app::Tabs::new(vec!["/a".into(), "/tmp/b".into()], 1);
        tab_strip(&mut app_state, true, Rect::new(0, 1, 40, 1));

        // " " then " a " then " b ".
        assert_eq!(app_state.tab_at(0, 1), None);
        assert_eq!(app_state.tab_at(1, 1), Some((true, StripHit::Item(0))));
        assert_eq!(app_state.tab_at(3, 1), Some((true, StripHit::Item(0))));
        assert_eq!(app_state.tab_at(4, 1), Some((true, StripHit::Item(1))));
        assert_eq!(app_state.tab_at(7, 1), None);
        assert_eq!(app_state.tab_at(4, 0), None);
    }

    /// Too many drives for the half they are in: a window onto them, with
    /// arrows that are there only while there is more that way, and every
    /// click mapped to what is drawn under it.
    #[test]
    fn a_crowded_drive_strip_scrolls() {
        use crate::fs_ops::{Mount, MountKind};
        let mut app_state = AppState::new();
        app_state.options.icon_style = IconStyle::Plain;
        app_state.dir_left = "/".into();
        app_state.mounts = (0..12)
            .map(|index| Mount { path: format!("/mnt/{index}").into(), label: format!("d{index}"), kind: MountKind::Disk })
            .collect();
        let area = Rect::new(0, 0, 30, 1);

        // Plain icons are five columns a slot: 30, less 4 for arrows, holds 5.
        drive_strip(&mut app_state, true, area);
        let strip = &app_state.drive_strip_left;
        assert_eq!((strip.first, strip.visible), (0, 5));
        assert_eq!(strip.hits.first(), Some(&StripHit::Item(0)));
        assert_eq!(strip.hits.last(), Some(&StripHit::Forward));
        assert_eq!(app_state.drive_at(1, 0), None); // no back arrow yet
        assert_eq!(app_state.drive_at(2, 0), Some((true, StripHit::Item(0))));
        assert_eq!(app_state.drive_at(27, 0), Some((true, StripHit::Forward)));

        // A page along, and the back arrow appears with the icons where they were.
        app_state.scroll_drive_strip(true, true);
        drive_strip(&mut app_state, true, area);
        assert_eq!(app_state.drive_at(0, 0), Some((true, StripHit::Back)));
        assert_eq!(app_state.drive_at(2, 0), Some((true, StripHit::Item(5))));

        // At the end, no forward arrow.
        app_state.scroll_drive_strip(true, true);
        drive_strip(&mut app_state, true, area);
        assert_eq!(app_state.drive_strip_left.first, 7);
        assert!(!app_state.drive_strip_left.hits.contains(&StripHit::Forward));

        // Room for all: no arrows and the old layout.
        drive_strip(&mut app_state, true, Rect::new(0, 0, 80, 1));
        assert_eq!(app_state.drive_strip_left.first, 0);
        assert_eq!(app_state.drive_at(1, 0), Some((true, StripHit::Item(0))));
    }

    /// A file name can hold anything but '/' and NUL - an escape sequence, a
    /// tab, a bell - and what a widget is handed goes to the terminal as it is.
    /// None of it may reach a cell: not in the row, the detail lines under it,
    /// the preview beside it, the status bar, or a popup that names the entry.
    #[test]
    fn a_hostile_name_never_reaches_the_terminal() {
        let dir = std::env::temp_dir().join(format!("fm84-hostile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let nasty = "a\u{1b}[41;97mHACKED\u{1b}[0m\tbell\u{7}.txt";
        std::fs::write(dir.join(nasty), "contents").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.show_preview = true;
        app_state.open_dir(true, dir.clone(), Some(nasty.as_ref()));
        app_state.refresh_cursor_detail();
        app_state.refresh_preview();
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();

        let mut clean = |app_state: &mut AppState, what: &str| {
            render_ui(&mut terminal, app_state);
            let dirty: Vec<String> = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.symbol().chars().any(char::is_control))
                .map(|cell| format!("{:?}", cell.symbol()))
                .collect();
            assert!(dirty.is_empty(), "{what} drew {dirty:?}");
        };

        clean(&mut app_state, "the panel");

        app_state.dialog = Some(Dialog::Delete { items: vec![(dir.join(nasty), false)], to_trash: true });
        clean(&mut app_state, "the delete popup");

        let mut input = TextInput::new();
        input.set(nasty.to_string());
        app_state.dialog = Some(Dialog::Rename(input));
        clean(&mut app_state, "the rename field");
        app_state.dialog = None;

        app_state.error = Some(format!("No such file or directory: {nasty}"));
        clean(&mut app_state, "the error popup");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The hexdump takes the widest layout its pane holds and never runs past
    /// it, at every width from one that fits nothing to one that fits the lot.
    #[test]
    fn hex_preview_fits_the_pane_it_is_given() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let mut seen = Vec::new();
        for width in 1..100u16 {
            let rows = hex_preview(&bytes, width, 4);
            let widest = rows.iter().map(|row| row.chars().count()).max().unwrap();
            // One column is the space every preview line opens with. The
            // narrowest layout is wider than the narrowest pane, which is the
            // one case where the text is left to be clipped.
            if width > 1 + crate::viewer::hex_row_width(4, 0) as u16 {
                assert!(widest < width as usize, "{width} columns held a {widest}-column row");
            }
            let bytes_shown: usize = rows.iter().map(|row| row.chars().filter(|c| *c == '|').count()).sum();
            assert_eq!(bytes_shown, 8, "{width}: every row keeps its gutter");
            seen.push((width, widest));
        }
        // Widest first, so a wider pane never shows less.
        assert!(seen.windows(2).all(|pair| pair[0].1 <= pair[1].1), "{seen:?}");
        assert_eq!(hex_preview(&bytes, 100, 4)[0].chars().count(), crate::viewer::hex_row_width(16, 4));
    }

    #[test]
    fn scrolling_sideways_goes_past_what_a_u16_holds() {
        let text = |spans: &[Span<'static>]| spans.iter().map(|span| span.content.as_ref()).collect::<String>();
        let spans = vec![Span::raw("ab"), Span::styled("cdef", style_title())];
        assert_eq!(text(&skip_columns(spans.clone(), 0)), "abcdef");
        assert_eq!(text(&skip_columns(spans.clone(), 3)), "def");
        // The style stays with what is left of its span.
        assert_eq!(skip_columns(spans.clone(), 3)[0].style, style_title());
        assert!(skip_columns(spans, 10).is_empty());

        // Half of a wide character is a space.
        assert_eq!(text(&skip_columns(vec![Span::raw("界x")], 1)), " x");

        // A minified line, scrolled further than 65,535 columns.
        let long = format!("{}END", "x".repeat(70_000));
        assert_eq!(text(&skip_columns(vec![Span::raw(long)], 70_000)), "END");
    }

    #[test]
    fn popup_text_wraps_at_spaces_and_keeps_the_last_line_in_view() {
        assert_eq!(wrap_text("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap_text("short", 20), ["short"]);
        // A word wider than a row is broken inside it.
        assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_text("", 4), [""]);
        // Wide characters count as the two columns they take.
        assert_eq!(wrap_text("界界界", 4), ["界界", "界"]);

        // Twenty rows of names, a destination and the keys, in seven rows:
        // the names give way, and the keys are still there.
        let names = (0..60).map(|n| format!("file{n:02}")).collect::<Vec<_>>().join(", ");
        let lines = vec![Line::from(names), Line::from("to: /tmp"), Line::from("Y / Enter - Yes")];
        let blocks = fit_rows(lines.into_iter().map(|line| wrap_line(line, 30)).collect(), 7);
        let rows: Vec<String> = blocks.iter().flatten().map(|line| line.to_string()).collect();
        assert_eq!(rows.len(), 7);
        assert!(rows[4].ends_with('…'), "{rows:?}");
        assert_eq!(rows[5..], ["to: /tmp", "Y / Enter - Yes"]);
    }

    #[test]
    fn centered_sits_in_the_middle_and_never_overflows() {
        let area = Rect::new(10, 5, 80, 20);
        assert_eq!(centered(area, 40, 20), Rect::new(30, 5, 40, 20));
        assert_eq!(centered(area, 80, 10), Rect::new(10, 10, 80, 10));
        // Larger than the area, as a filled image is: clipped to it, not shifted.
        assert_eq!(centered(area, 200, 90), area);
        // An odd leftover puts the extra column on the right.
        assert_eq!(centered(area, 79, 20).x, 10);
    }

    /// An editor open on a file of `lines` numbered lines, and a terminal to
    /// draw it on. The file is removed again when the test is done with it.
    fn editor_on(name: &str, lines: usize) -> (AppState, Terminal<ratatui::backend::TestBackend>, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("fm84-{name}-{}.txt", std::process::id()));
        let text: String = (1..=lines).map(|n| format!("{n}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_editor(path.clone()).unwrap();
        let terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        (app_state, terminal, path)
    }

    #[test]
    fn deleting_everything_from_the_bottom_of_a_long_file_draws() {
        let (mut app_state, mut terminal, path) = editor_on("select-all", 100);
        render_ui(&mut terminal, &mut app_state);
        // Ctrl+A scrolls to the end; Backspace then leaves a single line. The
        // scroll used to stay where Ctrl+A put it, and the next frame sliced
        // the lines with its start past its end.
        app_state.editor_select_all();
        app_state.editor_backspace();
        render_ui(&mut terminal, &mut app_state);
        let state = app_state.editor().unwrap();
        assert_eq!(state.lines, [""]);
        assert_eq!(state.scroll_offset, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_long_paste_leaves_the_cursor_on_screen() {
        let (mut app_state, mut terminal, path) = editor_on("paste", 3);
        render_ui(&mut terminal, &mut app_state);
        let pasted: String = (0..200).map(|n| format!("pasted {n}\n")).collect();
        app_state.editor_insert_text(&pasted);
        render_ui(&mut terminal, &mut app_state);
        let state = app_state.editor().unwrap();
        let height = app_state.editor_viewport_height;
        assert!(state.cursor_line >= state.scroll_offset && state.cursor_line < state.scroll_offset + height);
        std::fs::remove_file(path).unwrap();
    }
}
