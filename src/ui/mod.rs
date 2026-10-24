//! Drawing: the screen laid out from top to bottom, the styles everything is
//! drawn in, and the cursor placed once a frame is done.

use crate::app::{AppState, AttributeKind, AttributesForm, Dialog, FindFiles, PromptKind, Screen, TextInput, TransferKind};
use crate::constants::*;
use crate::display::{palette, tab_width};
use crate::options::{Clock, DateFormat, IconStyle, OPTION_ROWS, Options, SortKey};
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

mod bars;
mod help;
mod options_popup;
mod panels;
mod popups;
mod screens;
mod spans;
#[cfg(test)]
mod tests;

pub use self::help::*;
use self::{bars::*, options_popup::*, panels::*, popups::*, screens::*, spans::*};

// The styles used throughout rendering, from whichever palette is showing.
fn style_border() -> Style {
    Style::new().fg(palette().border)
}
fn style_title() -> Style {
    Style::new().fg(palette().title)
}
fn style_columns() -> Style {
    Style::new().fg(palette().columns)
}
fn style_file() -> Style {
    Style::new().fg(palette().file)
}
fn style_dir() -> Style {
    Style::new().fg(palette().directory)
}
fn style_dir_dark() -> Style {
    Style::new().fg(palette().directory_dark)
}
// A field being typed into, wherever it is: the colours of a name being
// renamed in a panel, so every one reads the same and stands out from what is
// around it. The cell the cursor is on carries a mark, for show_cursor to
// put the terminal's cursor there - or, for Classic, to reverse it.
fn style_input() -> Style {
    Style::new().fg(palette().selected_foreground).bg(palette().rename_background).add_modifier(Modifier::BOLD)
}
fn style_input_cursor() -> Style {
    style_input().add_modifier(FIELD_CURSOR)
}

// Where the cursor is, marked on its cell as it is drawn: a field's, and the
// editor's. Each field draws its own text in its own place - centred in a
// popup, after a label, scrolled - and the mark finds the cell wherever that
// put it. Two modifiers nothing else draws with, both taken off again before
// the frame reaches the terminal, where they would make text blink.
const FIELD_CURSOR: Modifier = Modifier::SLOW_BLINK;
const EDITOR_CURSOR: Modifier = Modifier::RAPID_BLINK;
fn style_selection() -> Style {
    Style::new().bg(palette().selected_background_inactive)
}
// The other matches of a search. Marked rather than coloured, so it reads in
// every theme and leaves the syntax colours underneath alone.
fn style_match() -> Style {
    Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
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

    crate::display::set_classic_cursor(app_state.options.cursor == crate::options::CursorShape::Classic);
    let _ = terminal.draw(|f| {
        draw_frame(f, app_state);
        show_cursor(f, app_state);
    });
}

/// Show the cursor where the frame marked it. A field's wins over the
/// editor's, as a prompt over the editor is where the typing goes; and the
/// editor's shows only with nothing over it, as a question such as Save
/// changes? takes the keys while the editor stays in sight. As Classic
/// has it, each marked cell is drawn reversed instead and the terminal's own
/// cursor stays hidden - as it does under a running job or an error, which
/// take the keys from whatever is behind them.
fn show_cursor(f: &mut ratatui::Frame<'_>, app_state: &AppState) {
    let classic = app_state.options.cursor == crate::options::CursorShape::Classic;
    let buffer = f.buffer_mut();
    let (mut field, mut editor) = (None, None);
    for y in buffer.area.top()..buffer.area.bottom() {
        for x in buffer.area.left()..buffer.area.right() {
            let cell = &mut buffer[(x, y)];
            // Past the end of what is typed, the cell takes the colour of the
            // one after it - the rest of a field padded out to its width, or
            // whatever the field ends against - so the text does not look a
            // character longer than it is. Classic's block is the cursor
            // itself, and keeps the field's colour.
            if cell.modifier.contains(crate::app::PAST_END) {
                cell.modifier.remove(crate::app::PAST_END);
                if !classic && x + 1 < buffer.area.right() {
                    let after = buffer[(x + 1, y)].bg;
                    buffer[(x, y)].set_bg(after);
                }
            }
            // The last character typed, with the cursor past it: the
            // terminal's cursor goes in the cell after - however wide this
            // one is - on whatever the field ends against.
            let cell = &mut buffer[(x, y)];
            if cell.modifier.contains(crate::app::CURSOR_AFTER) {
                cell.modifier.remove(crate::app::CURSOR_AFTER);
                let width = display_width(cell.symbol()).max(1) as u16;
                field.get_or_insert(ratatui::layout::Position::new((x + width).min(buffer.area.right() - 1), y));
            }
            let cell = &mut buffer[(x, y)];
            if cell.modifier.contains(FIELD_CURSOR) {
                cell.modifier.remove(FIELD_CURSOR);
                field.get_or_insert(ratatui::layout::Position::new(x, y));
                if classic {
                    cell.modifier.insert(Modifier::REVERSED);
                }
            }
            if cell.modifier.contains(EDITOR_CURSOR) {
                cell.modifier.remove(EDITOR_CURSOR);
                editor.get_or_insert(ratatui::layout::Position::new(x, y));
                if classic {
                    cell.set_fg(palette().selected_foreground).set_bg(palette().selected_background);
                }
            }
        }
    }
    if !classic
        && app_state.job.is_none()
        && app_state.error.is_none()
        && let Some(position) = field.or(editor.filter(|_| app_state.dialog.is_none()))
    {
        f.set_cursor_position(position);
    }
}

/// The screen, top to bottom: the top bar, the path bar, the panels (or the
/// viewer, the editor, the diff), the status bar and the key bar. Popups are
/// placed by it too, over the panels and no taller than them.
fn screen_layout(area: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(1), Constraint::Percentage(100), Constraint::Length(1), Constraint::Length(3)])
        .split(area)
}

fn draw_frame(f: &mut ratatui::Frame<'_>, app_state: &mut AppState) {
    {
        let area = f.area();
        paint_background(f, area);

        // Guard against terminal too small to render
        if area.height < 10 || area.width < 30 {
            let msg = Paragraph::new("Terminal too small").alignment(Alignment::Center).style(style_title());
            let y = area.height / 2;
            if y < area.height {
                f.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
            }
            return;
        }

        let chunks_main = screen_layout(area);

        render_top_panel(f, chunks_main[0], app_state);
        let going_to = match &app_state.dialog {
            Some(Dialog::Prompt(PromptKind::GoTo, input)) => Some(input),
            _ => None,
        };
        render_path_bar(f, chunks_main[1], app_state, going_to);
        app_state.path_bar_area = chunks_main[1];
        match app_state.screen {
            Screen::Viewer(_) => {
                let (height, width, content_area) = render_viewer(f, chunks_main[2], app_state);
                app_state.viewer_viewport_height = height;
                app_state.viewer_viewport_width = width;
                app_state.viewer_content_area = content_area;
            }
            Screen::Editor(_) => app_state.editor_viewport_height = render_editor(f, chunks_main[2], app_state),
            Screen::Diff(_) => {
                let (height, width) = render_diff(f, chunks_main[2], app_state);
                app_state.diff_viewport_height = height;
                app_state.diff_viewport_width = width;
            }
            Screen::Panels => app_state.page_size = render_file_tables(f, chunks_main[2], app_state),
        }
        render_bottom_panel(f, chunks_main[3], app_state);
        render_fkey_bar(f, chunks_main[4], app_state);
        render_detail(f, chunks_main[4].inner(Margin { vertical: 0, horizontal: 2 }), app_state);

        // The rename field, the prompt and the drive picker are drawn where
        // they sit, in the panel, the status bar and the drive strip - and so
        // is the replace question, under the match it is asking about.
        let mut find_files_page = None;
        let mut attribute_rows = Vec::new();
        match &app_state.dialog {
            Some(Dialog::Help) => {
                app_state.help_room = render_help_popup(f, area, app_state.help_scroll);
                // Kept to what the room allows, so a screen that has grown
                // does not leave the keys scrolling through lines past the end.
                app_state.help_scroll = app_state.help_scroll.min(help_lines().len().saturating_sub(app_state.help_room));
            }
            Some(Dialog::Options { editing }) => render_options_popup(f, area, app_state, editing.as_ref()),
            Some(Dialog::Create { is_dir, input }) => render_create_popup(f, area, *is_dir, input),
            Some(Dialog::Delete { items, to_trash }) => render_delete_popup(f, area, items, *to_trash),
            Some(Dialog::Transfer { sources, is_copy, input, .. }) => render_destination_popup(f, area, sources, if *is_copy { "Copy" } else { "Move" }, "to", input),
            Some(Dialog::Symlink { sources, input, .. }) => render_destination_popup(f, area, sources, "Symlink", "at", input),
            Some(Dialog::LargeFile(large)) => render_large_file_popup(f, area, large),
            Some(Dialog::SaveChanges { .. }) => render_editor_save_popup(f, area),
            Some(Dialog::DiskChanged { opened, now, .. }) => render_disk_changed_popup(f, area, app_state, *opened, *now),
            Some(Dialog::FindFiles(find)) => match find.as_ref() {
                FindFiles::Asking { name, containing, on_containing } => {
                    let start = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
                    render_find_files_form(f, area, start, name, containing, *on_containing);
                }
                FindFiles::Found(search) => find_files_page = Some(render_find_files_results(f, area, search)),
            },
            Some(Dialog::Attributes(form)) => attribute_rows = render_attributes_popup(f, area, form),
            Some(Dialog::Bookmarks { list, index }) => render_bookmarks_popup(f, area, list, *index, &app_state.panel_dir(app_state.is_left_active)),
            Some(Dialog::Rename(_) | Dialog::Prompt(..) | Dialog::DrivePicker { .. } | Dialog::Replacing { .. }) | None => {}
        }
        if let Some(page) = find_files_page {
            app_state.find_files_page = page;
        }
        app_state.attribute_rows = attribute_rows;
        // Over everything: a job, which holds the keys while it runs, or an
        // error, which holds them until it is put away.
        if app_state.job.is_some() {
            render_transfer_popup(f, area, app_state);
        } else if let Some(message) = &app_state.error {
            render_error_popup(f, area, message);
        }
    }
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

/// A `width` x `height` area in the middle of `area`, shrunk to fit inside it.
fn centered(area: Rect, width: usize, height: usize) -> Rect {
    let width = width.min(area.width as usize) as u16;
    let height = height.min(area.height as usize) as u16;
    Rect::new(area.x + (area.width - width) / 2, area.y + (area.height - height) / 2, width, height)
}
