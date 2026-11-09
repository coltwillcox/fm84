//! Drawing: the screen laid out from top to bottom, the styles everything is
//! drawn in, and the cursor placed once a frame is done.

use crate::app::{AppState, AttributeKind, AttributesForm, BranchList, Dialog, Drawn, FindFiles, PromptKind, Screen, Side, TextInput, TransferKind};
use crate::constants::*;
use crate::display::{palette, tab_width};
use crate::options::{Clock, DateFormat, IconStyle, OPTION_ROWS, Options, ScrollBarStyle, SortKey};
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
mod checksums;
mod help;
mod options_popup;
mod panels;
mod popups;
mod screens;
mod scroll_bars;
mod spans;
#[cfg(test)]
mod tests;

pub use self::help::*;
use self::{bars::*, checksums::*, options_popup::*, panels::*, popups::*, screens::*, scroll_bars::*, spans::*};

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
/// What a key is drawn in, beside what it does.
fn style_key() -> Style {
    style_columns().add_modifier(Modifier::BOLD)
}

/// Names of keys that stand alone, as the languages write them. Chords,
/// F-keys and letters are told by their shape instead.
const KEY_NAMES: [&str; 27] = [
    "Tab",
    "Esc",
    "Échap",
    "Enter",
    "Entrée",
    "Intro",
    "Invio",
    "Space",
    "Espace",
    "Espacio",
    "Espaço",
    "Spacja",
    "Spazio",
    "Szóköz",
    "Leertaste",
    "Insert",
    "Delete",
    "Del",
    "Entf",
    "Supr",
    "Suppr",
    "Canc",
    "Ins",
    "Home",
    "End",
    "PgUp",
    "PgDn",
];

/// Whether a word is a key: a chord, which has a `+` in it in any language
/// (Ctrl+F, Strg+F, +/-); an F-key, or several (F3/Shift+F3); a letter or a
/// sign alone (X, [N], =, *); or a key's name.
fn is_key(word: &str) -> bool {
    let mut characters = word.chars();
    let (first, second) = (characters.next(), characters.next());
    word.contains('+') || (first == Some('F') && second.is_some_and(|c| c.is_ascii_digit())) || (second.is_none() && first.is_some_and(|c| c.is_ascii_uppercase() || "=*".contains(c))) || KEY_NAMES.contains(&word)
}

/// A row of keys and what each does, as the popups give them: `Enter - Save
/// Esc - Cancel`, two spaces or more between one and the next. Before the
/// dash is the key. A part with no dash is a key only as far as its words
/// look like one - `Del Delete`, `→ ← Copy` - and otherwise is all words,
/// as `Type to narrow` is.
fn key_hints(text: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        // The part, and the spaces after it.
        let end = rest.find("  ").unwrap_or(rest.len());
        let gap = rest[end..].len() - rest[end..].trim_start_matches(' ').len();
        let part = &rest[..end];
        let key_len = match part.find(" - ") {
            Some(at) => at,
            None => {
                // Leading words that look like keys, with something after them.
                let mut taken = 0;
                for word in part.split_inclusive(' ') {
                    let bare = word.trim_end();
                    if !(is_key(bare) || (!bare.is_empty() && !bare.chars().any(char::is_alphanumeric))) {
                        break;
                    }
                    taken += word.len();
                }
                // Every word one - `Del Delete` - and the last is what it does.
                let taken = if taken >= part.len() { part.trim_end().rfind(' ').unwrap_or(0) } else { taken };
                part[..taken].trim_end().len()
            }
        };
        let (key, what) = part.split_at(key_len);
        if !key.is_empty() {
            spans.push(Span::styled(key.to_string(), style_key()));
        }
        if !what.is_empty() {
            spans.push(Span::styled(what.to_string(), style_title()));
        }
        if gap > 0 {
            spans.push(Span::raw(rest[end..end + gap].to_string()));
        }
        rest = &rest[end + gap..];
    }
    Line::from(spans)
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

/// Draw a frame. The frame only reads the state: what it drew, and where,
/// goes into a `Drawn` of its own, taken out of the state while it draws and
/// put back after for the keys and the mouse to read; and what it worked out
/// the state should take on - scrolls kept in range, strips laid out - comes
/// back as `Settled`, applied once it is done.
pub fn render_ui<B: Backend>(terminal: &mut Terminal<B>, app_state: &mut AppState) {
    let mut drawn = std::mem::take(&mut app_state.drawn);
    // Update cached clock
    let current_time = match app_state.options.clock {
        Clock::Hours24 => Local::now().format(" %H:%M:%S ").to_string(),
        Clock::Hours12 => Local::now().format(" %I:%M:%S %p ").to_string(),
        Clock::Off => String::new(),
    };
    if drawn.cached_clock != current_time {
        drawn.cached_clock = current_time;
    }

    crate::display::set_classic_cursor(app_state.options.cursor == crate::options::CursorShape::Classic);
    let mut settled = Settled::default();
    let _ = terminal.draw(|f| {
        settled = draw_frame(f, app_state, &mut drawn);
        show_cursor(f, app_state);
    });
    app_state.drawn = drawn;
    settled.apply(app_state);
}

/// What a frame worked out that the state takes on after it - a scroll kept
/// to what the room allows, now the room is known. Drawing only reads the
/// state; this is how what it found goes back.
#[derive(Default)]
struct Settled {
    /// F1's list, kept to its room, so a screen that has grown does not
    /// leave the keys scrolling through lines past the end.
    help_scroll: Option<usize>,
    /// The open popup's list - checksums, a sync plan or a multi-rename:
    /// the rows it had room for, and its scroll kept to them.
    popup_list: Option<(usize, usize)>,
    /// The diff's scroll, kept so the last row stays at the bottom.
    diff_scroll: Option<usize>,
    /// The editor's scroll down and across, kept with the cursor in view.
    editor_scroll: Option<(usize, usize)>,
    /// The drive strips and the tab strips, left then right, as laid out:
    /// which items they show, and where each was drawn.
    strips: Option<([Strip; 2], [Strip; 2])>,
}

impl Settled {
    fn apply(self, app_state: &mut AppState) {
        if let Some(scroll) = self.help_scroll {
            app_state.help_scroll = scroll;
        }
        let list = match &mut app_state.dialog {
            Some(Dialog::Checksums(view)) => Some((&mut view.room, &mut view.scroll)),
            Some(Dialog::Sync(plan)) => Some((&mut plan.room, &mut plan.scroll)),
            Some(Dialog::DirEdit(plan)) => Some((&mut plan.room, &mut plan.scroll)),
            Some(Dialog::MultiRename(rename)) => Some((&mut rename.room, &mut rename.scroll)),
            _ => None,
        };
        if let (Some((room, scroll)), Some((to_room, to_scroll))) = (self.popup_list, list) {
            (*to_room, *to_scroll) = (room, scroll);
        }
        if let (Some(scroll), Screen::Diff(state)) = (self.diff_scroll, &mut app_state.screen) {
            state.scroll_offset = scroll;
        }
        if let (Some((down, across)), Screen::Editor(state)) = (self.editor_scroll, &mut app_state.screen) {
            (state.scroll_offset, state.horizontal_offset) = (down, across);
        }
        if let Some((drives, [left, right])) = self.strips {
            app_state.drive_strips = drives;
            (app_state.left.tabs.strip, app_state.right.tabs.strip) = (left, right);
        }
    }
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
        && app_state.workers.job.is_none()
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

fn draw_frame(f: &mut ratatui::Frame<'_>, app_state: &AppState, drawn: &mut Drawn) -> Settled {
    let mut settled = Settled::default();
    {
        let area = f.area();
        paint_background(f, area);

        // Guard against terminal too small to render
        if area.height < 10 || area.width < 30 {
            let msg = Paragraph::new(t!("Terminal too small")).alignment(Alignment::Center).style(style_title());
            let y = area.height / 2;
            if y < area.height {
                f.render_widget(msg, Rect::new(area.x, area.y + y, area.width, 1));
            }
            return settled;
        }

        let chunks_main = screen_layout(area);

        settled.strips = Some(render_top_panel(f, chunks_main[0], app_state, drawn));
        let going_to = match &app_state.dialog {
            Some(Dialog::Prompt(PromptKind::GoTo, input)) => Some(input),
            _ => None,
        };
        drawn.branch_hits = render_path_bar(f, chunks_main[1], app_state, going_to);
        drawn.path_bar_area = chunks_main[1];
        // Only the viewer and the editor have bars, and they put theirs back.
        drawn.scroll_bars.clear();
        match app_state.screen {
            Screen::Viewer(_) => {
                let (height, width, content_area, bars) = render_viewer(f, chunks_main[2], app_state);
                drawn.viewer_viewport_height = height;
                drawn.viewer_viewport_width = width;
                drawn.viewer_content_area = content_area;
                drawn.scroll_bars = bars;
            }
            Screen::Editor(_) => (drawn.editor_viewport_height, settled.editor_scroll) = render_editor(f, chunks_main[2], app_state, drawn),
            Screen::Diff(_) => {
                let (height, width, scroll) = render_diff(f, chunks_main[2], app_state);
                drawn.diff_viewport_height = height;
                drawn.diff_viewport_width = width;
                settled.diff_scroll = Some(scroll);
            }
            Screen::Panels => drawn.page_size = render_file_tables(f, chunks_main[2], app_state, drawn),
        }
        render_bottom_panel(f, chunks_main[3], app_state);
        render_fkey_bar(f, chunks_main[4], app_state, drawn);
        render_detail(f, chunks_main[4].inner(Margin { vertical: 0, horizontal: 2 }), app_state);

        // The rename field, the prompt and the drive picker are drawn where
        // they sit, in the panel, the status bar and the drive strip - and so
        // is the replace question, under the match it is asking about.
        let mut find_files_page = None;
        let mut attribute_rows = Vec::new();
        let mut branch_rows = Vec::new();
        match &app_state.dialog {
            Some(Dialog::Help) => {
                drawn.help_room = render_help_popup(f, area, app_state.help_scroll);
                settled.help_scroll = Some(app_state.help_scroll.min(help_lines().len().saturating_sub(drawn.help_room)));
            }
            Some(Dialog::Options { editing }) => render_options_popup(f, area, app_state, editing.as_ref()),
            Some(Dialog::Create { is_dir, input }) => render_create_popup(f, area, *is_dir, input),
            Some(Dialog::Delete { items, to_trash }) => render_delete_popup(f, area, items, *to_trash),
            Some(Dialog::Transfer { sources, is_copy, input, .. }) => render_destination_popup(f, area, sources, if *is_copy { Destination::Copy } else { Destination::Move }, input, None),
            Some(Dialog::Symlink { sources, input, hard, .. }) => {
                let (kind, other) = if *hard { (Destination::HardLink, t!("Tab - Symlink")) } else { (Destination::Symlink, t!("Tab - Hard link")) };
                render_destination_popup(f, area, sources, kind, input, Some(other));
            }
            Some(Dialog::Extract { archive, input }) => render_destination_popup(f, area, &[(archive.clone(), false)], Destination::Extract, input, None),
            Some(Dialog::Checksums(view)) => settled.popup_list = Some(render_checksums_popup(f, area, view)),
            Some(Dialog::DirEdit(plan)) => settled.popup_list = Some(render_dir_edit_popup(f, area, plan)),
            Some(Dialog::Sync(plan)) => settled.popup_list = Some(render_sync_popup(f, area, plan, app_state.options.date_format)),
            Some(Dialog::MultiRename(rename)) => settled.popup_list = Some(render_multi_rename_popup(f, area, rename)),
            Some(Dialog::Pack { sources, input }) => render_destination_popup(f, area, sources, Destination::Pack, input, Some(t!("Tab - Format"))),
            Some(Dialog::LargeFile(large)) => render_large_file_popup(f, area, large),
            Some(Dialog::SaveChanges { .. }) => render_editor_save_popup(f, area),
            Some(Dialog::DiskChanged { opened, now, .. }) => render_disk_changed_popup(f, area, app_state, *opened, *now),
            Some(Dialog::FindFiles(find)) => match find.as_ref() {
                FindFiles::Asking(form) => {
                    let start = &app_state.panel(app_state.active_side).dir;
                    render_find_files_form(f, area, start, form);
                }
                FindFiles::Found(search) => find_files_page = Some(render_find_files_results(f, area, search)),
            },
            Some(Dialog::Attributes(form)) => attribute_rows = render_attributes_popup(f, area, form),
            Some(Dialog::Branches(list)) => branch_rows = render_branches_popup(f, area, list),
            Some(Dialog::Bookmarks { list, index }) => render_bookmarks_popup(f, area, list, *index, &app_state.panel_dir(app_state.active_side)),
            Some(Dialog::History(list)) => render_history_popup(f, area, list, &app_state.panel_dir(app_state.active_side)),
            Some(Dialog::UserMenu(menu)) => render_user_menu_popup(f, area, menu),
            Some(Dialog::OpenWith(menu)) => render_open_with_popup(f, area, menu),
            Some(Dialog::Rename(_) | Dialog::Prompt(..) | Dialog::DrivePicker { .. } | Dialog::Replacing { .. }) | None => {}
        }
        if let Some(page) = find_files_page {
            drawn.find_files_page = page;
        }
        drawn.attribute_rows = attribute_rows;
        drawn.branch_rows = branch_rows;
        // Over everything: a job, which holds the keys while it runs, or an
        // error, which holds them until it is put away.
        if app_state.workers.job.is_some() {
            render_transfer_popup(f, area, app_state);
        } else if let Some(message) = &app_state.error {
            render_error_popup(f, area, message);
        }
    }
    settled
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
