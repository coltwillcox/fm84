use crate::fs_ops::{Mount, disk_usage, get_current_dir, list_mounts, load_directory_rows, nearest_existing_dir};
use crate::viewer::ViewerState;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

/// Reusable single-line text input with cursor.
pub struct TextInput {
    pub text: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self { text: String::new(), cursor: 0 }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set(&mut self, value: String) {
        self.cursor = value.chars().count();
        self.text = value;
    }

    fn byte_index(&self) -> usize {
        self.text.char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let len = self.text.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn insert(&mut self, c: char) {
        let idx = self.byte_index();
        self.text.insert(idx, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let byte_start = self.text.char_indices()
                .nth(self.cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let byte_end = self.byte_index();
            self.text.replace_range(byte_start..byte_end, "");
            self.cursor -= 1;
        }
    }

    pub fn delete_forward(&mut self) {
        let len = self.text.chars().count();
        if self.cursor < len {
            let byte_start = self.byte_index();
            let byte_end = self.text.char_indices()
                .nth(self.cursor + 1)
                .map(|(i, _)| i)
                .unwrap_or(self.text.len());
            self.text.replace_range(byte_start..byte_end, "");
        }
    }

    /// Returns styled spans with a block cursor at the cursor position.
    pub fn cursor_spans(&self, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        let byte_idx = self.byte_index();
        let before = self.text[..byte_idx].to_string();
        let rest = &self.text[byte_idx..];
        let mut chars = rest.chars();
        let cursor_char = chars.next().unwrap_or(' ');
        let after: String = chars.collect();

        vec![
            Span::styled(before, text_style),
            Span::styled(cursor_char.to_string(), cursor_style),
            Span::styled(after, text_style),
        ]
    }
}

pub struct AppState {
    pub is_error_displayed: bool,
    pub is_f1_displayed: bool,
    pub is_f11_displayed: bool,
    pub is_f12_displayed: bool,
    pub preview: Option<PreviewState>,
    /// Editor clipboard. Internal, so it works in a bare TTY too.
    pub clipboard: String,
    /// Mounts offered in the top strip, refreshed on the same tick as disk usage.
    pub mounts: Vec<Mount>,
    /// Which panel is choosing a drive, and which mount it has highlighted.
    pub drive_picker: Option<(bool, usize)>,
    pub is_f2_displayed: bool,
    pub is_f7_displayed: bool,
    /// The create dialog makes a directory (F7) or an empty file (Shift+F4).
    pub create_is_dir: bool,
    pub is_left_active: bool,
    pub dir_left: PathBuf,
    pub dir_right: PathBuf,
    pub page_size: u16,
    pub state_left: TableState,
    pub state_right: TableState,
    pub children_left: Vec<Item>,
    pub children_right: Vec<Item>,
    pub error_message: String,
    pub rename_input: TextInput,
    pub create_input: TextInput,
    pub is_f8_displayed: bool,
    pub delete_items: Vec<(String, bool)>,
    pub search_input: String,
    pub cached_clock: String,
    pub cached_separator_height: u16,
    pub cached_separator: String,
    pub is_f3_displayed: bool,
    pub viewer_state: Option<ViewerState>,
    pub viewer_viewport_height: usize,
    pub viewer_viewport_width: usize,
    pub is_f4_displayed: bool,
    pub editor_state: Option<EditorState>,
    pub editor_viewport_height: usize,
    pub is_f5_displayed: bool,
    pub copy_items: Vec<(PathBuf, PathBuf, bool)>,
    pub is_f6_displayed: bool,
    pub move_items: Vec<(PathBuf, PathBuf, bool)>,
    // Keyed by file name, not row index: a reload can re-sort the rows, and an
    // index would then point at a different file than the one the user picked.
    pub selected_left: HashSet<String>,
    pub selected_right: HashSet<String>,
    pub dir_sizes: HashMap<PathBuf, u64>,
    pub last_click_time: Option<Instant>,
    pub last_click_pos: (u16, u16),
    pub is_editor_save_prompt: bool,
    // Where the last frame actually drew things. Mouse handling reads these
    // instead of recomputing the layout from hardcoded row numbers.
    pub table_area_left: Rect,
    pub table_area_right: Rect,
    pub viewport_start_left: usize,
    pub viewport_start_right: usize,
    pub editor_content_area: Rect,
    // Directory mtimes as of the last load, so an external change can be spotted
    // without stat-ing every entry.
    pub dir_stamp_left: Option<SystemTime>,
    pub dir_stamp_right: Option<SystemTime>,
    pub last_refresh_check: Instant,
    // (used, total) bytes for each panel's filesystem.
    pub disk_left: Option<(u64, u64)>,
    pub disk_right: Option<(u64, u64)>,
    /// A file big enough to be worth asking about: (path, size, opening to edit).
    pub large_file: Option<(PathBuf, u64, bool)>,
}

#[derive(Clone)]
pub struct EditorState {
    pub file_path: PathBuf,
    pub lines: Vec<String>,
    pub highlighted_lines: Vec<Vec<Span<'static>>>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub modified: bool,
    pub auto_scroll: bool,
    /// Where a selection began. The selection runs from here to the cursor.
    pub selection_anchor: Option<(usize, usize)>,
    /// Edits that can be undone, oldest first.
    pub undo_stack: Vec<EditStep>,
    /// Undone edits, ready to be put back. Cleared by any fresh edit.
    pub redo_stack: Vec<EditStep>,
    /// Parser state entering each line, so an edit re-parses from that line
    /// instead of the whole file. Empty when highlighting is off.
    pub line_states: Vec<crate::viewer::LineState>,
    /// The terminator this file was written with, so saving doesn't rewrite
    /// every line of a CRLF file just because one character changed.
    pub line_ending: &'static str,
}

/// One undoable edit: the lines it replaced, and where the cursor was. The
/// number of lines that took their place is worked out at undo time from how
/// the buffer's length changed, so nothing has to be recorded afterwards.
#[derive(Clone)]
pub struct EditStep {
    first_line: usize,
    before: Vec<String>,
    total_lines_before: usize,
    cursor: (usize, usize),
}

/// What the opposite panel is showing while preview mode is on.
pub struct PreviewState {
    /// None for the parent entry, which has nothing to show but still holds
    /// the pane so it doesn't flicker back to a table as the cursor passes.
    pub path: Option<PathBuf>,
    pub label: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub name_full: String,
    pub name: String,
    pub extension: String,
    pub is_dir: bool,
    pub size: String,
    pub size_bytes: u64,
    pub modified: String,
    pub attributes: String,
}

impl AppState {
    pub fn new() -> Self {
        let mut state_left = TableState::default();
        state_left.select(Some(1));
        let mut state_right = TableState::default();
        state_right.select(Some(1));

        let (is_error_displayed, error_message, dir_root) = match get_current_dir() {
            Ok(root) => (false, String::new(), root),
            Err(e) => (true, e.to_string(), PathBuf::new()),
        };

        Self {
            is_error_displayed,
            is_f1_displayed: false,
            is_f11_displayed: false,
            is_f12_displayed: false,
            preview: None,
            clipboard: String::new(),
            mounts: Vec::new(),
            drive_picker: None,
            is_f2_displayed: false,
            is_f7_displayed: false,
            create_is_dir: true,
            is_left_active: true,
            dir_left: dir_root.clone(),
            dir_right: dir_root,
            page_size: 0,
            state_left,
            state_right,
            children_left: Vec::new(),
            children_right: Vec::new(),
            error_message,
            rename_input: TextInput::new(),
            create_input: TextInput::new(),
            is_f8_displayed: false,
            delete_items: Vec::new(),
            search_input: String::new(),
            cached_clock: String::new(),
            cached_separator_height: 0,
            cached_separator: String::new(),
            is_f3_displayed: false,
            viewer_state: None,
            viewer_viewport_height: 0,
            viewer_viewport_width: 0,
            is_f4_displayed: false,
            editor_state: None,
            editor_viewport_height: 0,
            is_f5_displayed: false,
            copy_items: Vec::new(),
            is_f6_displayed: false,
            move_items: Vec::new(),
            selected_left: HashSet::new(),
            selected_right: HashSet::new(),
            dir_sizes: HashMap::new(),
            last_click_time: None,
            last_click_pos: (0, 0),
            is_editor_save_prompt: false,
            table_area_left: Rect::default(),
            table_area_right: Rect::default(),
            viewport_start_left: 0,
            viewport_start_right: 0,
            editor_content_area: Rect::default(),
            dir_stamp_left: None,
            dir_stamp_right: None,
            last_refresh_check: Instant::now(),
            disk_left: None,
            disk_right: None,
            large_file: None,
        }
    }

    pub fn reset_rename(&mut self) {
        self.rename_input.clear();
        self.is_f2_displayed = false;
    }

    pub fn display_error(&mut self, message: String) {
        self.is_error_displayed = true;
        self.error_message = message;
    }

    pub fn reset_error(&mut self) {
        self.is_error_displayed = false;
        self.error_message.clear();
    }

    pub fn reset_create(&mut self) {
        self.is_f7_displayed = false;
        self.create_input.clear();
    }

    // Quick search methods
    pub fn search_add_char(&mut self, c: char) {
        self.search_input.push(c);
        self.jump_to_first_match();
    }

    pub fn search_backspace(&mut self) {
        self.search_input.pop();
        if !self.search_input.is_empty() {
            self.jump_to_first_match();
        }
    }

    pub fn search_clear(&mut self) {
        self.search_input.clear();
    }

    pub fn jump_to_first_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        if let Some(index) = children.iter().position(|item| item.name_full.to_lowercase().starts_with(&search_lower)) {
            state.select(Some(index));
        }
    }

    pub fn jump_to_next_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        let current = state.selected().unwrap_or(0);
        let len = children.len();

        // Search forward from current+1, wrapping around
        for offset in 1..=len {
            let i = (current + offset) % len;
            if children[i].name_full.to_lowercase().starts_with(&search_lower) {
                state.select(Some(i));
                return;
            }
        }
    }

    pub fn jump_to_prev_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        let current = state.selected().unwrap_or(0);
        let len = children.len();

        // Search backward from current-1, wrapping around
        for offset in 1..=len {
            let i = (current + len - offset) % len;
            if children[i].name_full.to_lowercase().starts_with(&search_lower) {
                state.select(Some(i));
                return;
            }
        }
    }

    /// Returns (&children, &mut state) for the active panel.
    fn active_panel_mut(&mut self) -> (&[Item], &mut TableState) {
        if self.is_left_active {
            (&self.children_left, &mut self.state_left)
        } else {
            (&self.children_right, &mut self.state_right)
        }
    }

    pub fn reset_delete(&mut self) {
        self.is_f8_displayed = false;
        self.delete_items.clear();
    }

    /// Open a file for viewing or editing, asking first when it is large enough
    /// that loading it will stall for a noticeable while.
    pub fn request_open(&mut self, file_path: PathBuf, is_edit: bool) {
        let size = match std::fs::metadata(&file_path) {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                self.display_error(e.to_string());
                return;
            }
        };

        if size > crate::constants::LARGE_FILE_SIZE {
            self.large_file = Some((file_path, size, is_edit));
        } else {
            self.open_file(file_path, is_edit);
        }
    }

    /// Answer to the large-file prompt: load it after all.
    pub fn confirm_large_file(&mut self) {
        if let Some((file_path, _, is_edit)) = self.large_file.take() {
            self.open_file(file_path, is_edit);
        }
    }

    pub fn reset_large_file(&mut self) {
        self.large_file = None;
    }

    fn open_file(&mut self, file_path: PathBuf, is_edit: bool) {
        let result = if is_edit { self.open_editor(file_path) } else { self.open_viewer(file_path) };
        if let Err(e) = result {
            self.display_error(e);
        }
    }

    pub fn open_viewer(&mut self, file_path: PathBuf) -> Result<(), String> {
        use crate::viewer::load_file_content;
        let state = load_file_content(&file_path).map_err(|e| e.to_string())?;
        self.viewer_state = Some(state);
        self.is_f3_displayed = true;
        Ok(())
    }

    pub fn close_viewer(&mut self) {
        self.is_f3_displayed = false;
        self.viewer_state = None;
    }

    pub fn viewer_scroll_down(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + 1).min(max);
        }
    }

    pub fn viewer_scroll_up(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.scroll_offset.saturating_sub(1);
        }
    }

    pub fn viewer_page_down(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + self.viewer_viewport_height).min(max);
        }
    }

    pub fn viewer_page_up(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.scroll_offset.saturating_sub(self.viewer_viewport_height);
        }
    }

    pub fn viewer_home(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = 0;
            state.horizontal_offset = 0;
        }
    }

    pub fn viewer_scroll_left(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
        }
    }

    /// Switch the viewer between text and a hexdump. Reads the raw bytes the
    /// first time they are needed, so a text file only pays for them on demand.
    pub fn viewer_toggle_hex(&mut self) {
        // Nothing to toggle on the refusal notice F4 puts up.
        if self.viewer_state.as_ref().is_some_and(|state| state.from_edit) {
            return;
        }

        let mut error = None;

        if let Some(state) = &mut self.viewer_state {
            if !state.hex && state.bytes.is_empty() && state.file_size > 0 {
                match std::fs::read(&state.file_path) {
                    Ok(bytes) => state.bytes = bytes,
                    Err(e) => error = Some(e.to_string()),
                }
            }

            if error.is_none() {
                // Leaving hex on a file that was never decoded: build the text
                // lossily rather than leave the pane blank.
                if state.hex && state.content_lines.is_empty() {
                    state.content_lines = String::from_utf8_lossy(&state.bytes).lines().map(str::to_string).collect();
                    if state.content_lines.is_empty() {
                        state.content_lines.push(String::new());
                    }
                    state.max_line_width = state
                        .content_lines
                        .iter()
                        .map(|line| crate::utils::line_display_width(line))
                        .max()
                        .unwrap_or(0);
                }

                state.hex = !state.hex;
                state.total_lines = if state.hex {
                    crate::viewer::hex_line_count(&state.bytes)
                } else {
                    state.content_lines.len().max(1)
                };
                state.scroll_offset = state.scroll_offset.min(state.total_lines.saturating_sub(1));
                state.horizontal_offset = 0;
            }
        }

        if let Some(e) = error {
            self.display_error(e);
        }
    }

    pub fn viewer_scroll_right(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            // Stop once the longest line's end reaches the right edge.
            let longest = if state.hex { crate::constants::HEX_LINE_WIDTH } else { state.max_line_width };
            let max = longest.saturating_sub(self.viewer_viewport_width);
            state.horizontal_offset = (state.horizontal_offset + 1).min(max);
        }
    }

    pub fn viewer_end(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.total_lines.saturating_sub(self.viewer_viewport_height);
        }
    }

    pub fn open_editor(&mut self, file_path: PathBuf) -> Result<(), String> {
        use crate::constants::MAX_HIGHLIGHT_SIZE;
        use crate::viewer::{highlight_all, is_binary_file};

        let file_size = std::fs::metadata(&file_path).map_err(|e| e.to_string())?.len();

        if is_binary_file(&file_path).unwrap_or(false) {
            self.open_viewer(file_path)?;
            if let Some(state) = &mut self.viewer_state {
                // Editing is refused outright; F3 is where a binary gets read.
                state.from_edit = true;
                state.hex = false;
                state.bytes = Vec::new();
                state.content_lines = Vec::new();
                state.total_lines = 1;
            }
            return Ok(());
        }

        let content = std::fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
        let line_ending = detect_line_ending(&content);
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if content.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let (highlighted_lines, line_states) = if file_size <= MAX_HIGHLIGHT_SIZE {
            highlight_all(&lines, extension)
        } else {
            // Rendering already falls back to plain text when these are empty.
            (Vec::new(), Vec::new())
        };

        self.editor_state = Some(EditorState {
            file_path,
            lines,
            highlighted_lines,
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            horizontal_offset: 0,
            modified: false,
            auto_scroll: true,
            selection_anchor: None,
            line_ending,
            line_states,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        });
        self.is_f4_displayed = true;
        Ok(())
    }

    pub fn close_editor(&mut self) {
        self.is_f4_displayed = false;
        self.editor_state = None;
    }

    /// Re-highlight the file from `from` downward. Cheap: the cached state lets
    /// it resume mid-file and stop again as soon as the parse converges.
    pub fn editor_rehighlight_from(&mut self, from: usize) {
        if let Some(state) = &mut self.editor_state {
            if state.line_states.is_empty() {
                return; // highlighting disabled for this file
            }
            let extension = state.file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            crate::viewer::highlight_from(
                &state.lines,
                extension,
                from,
                &mut state.highlighted_lines,
                &mut state.line_states,
            );
        }
    }

    pub fn editor_scroll_up(&mut self) {
        if let Some(state) = &mut self.editor_state {
            if state.scroll_offset > 0 {
                state.scroll_offset -= 1;
                state.auto_scroll = false;
            }
        }
    }

    pub fn editor_scroll_down(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let max = state.lines.len().saturating_sub(self.editor_viewport_height);
            if state.scroll_offset < max {
                state.scroll_offset += 1;
                state.auto_scroll = false;
            }
        }
    }

    pub fn editor_scroll_left(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
            state.auto_scroll = false;
        }
    }

    pub fn editor_scroll_right(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.horizontal_offset += 1;
            state.auto_scroll = false;
        }
    }

    /// Remember the lines `range` covers before they are replaced. Called once
    /// per user action, with a range spanning everything that action touches.
    fn push_undo(&mut self, range: std::ops::RangeInclusive<usize>) {
        if let Some(state) = &mut self.editor_state {
            let last = (*range.end()).min(state.lines.len().saturating_sub(1));
            let first = (*range.start()).min(last);

            state.undo_stack.push(EditStep {
                first_line: first,
                before: state.lines[first..=last].to_vec(),
                total_lines_before: state.lines.len(),
                cursor: (state.cursor_line, state.cursor_col),
            });

            // Editing after undoing forks the history; the old branch is gone.
            state.redo_stack.clear();

            if state.undo_stack.len() > crate::constants::UNDO_LIMIT {
                state.undo_stack.remove(0);
            }
        }
    }

    /// The range a user action is about to touch: the selection when there is
    /// one, otherwise the given fallback.
    fn edit_range(&self, fallback: std::ops::RangeInclusive<usize>) -> std::ops::RangeInclusive<usize> {
        match self.editor_state.as_ref().and_then(|state| state.selection()) {
            Some(((first, _), (last, _))) => first..=last,
            None => fallback,
        }
    }

    pub fn editor_undo(&mut self) {
        self.step_history(true);
    }

    pub fn editor_redo(&mut self) {
        self.step_history(false);
    }

    /// Move one step through the edit history. Undo and redo are the same
    /// operation in opposite directions: apply a step, and push the step that
    /// would put things back onto the other stack.
    fn step_history(&mut self, undoing: bool) {
        let mut from = None;

        if let Some(state) = &mut self.editor_state {
            let stack = if undoing { &mut state.undo_stack } else { &mut state.redo_stack };
            if let Some(step) = stack.pop() {
                // However many lines replaced the originals, the buffer's change
                // in length tells us how many to take back out.
                let removed = step.total_lines_before - step.before.len();
                let replaced = state.lines.len().saturating_sub(removed);
                let end = (step.first_line + replaced).min(state.lines.len());

                let inverse = EditStep {
                    first_line: step.first_line,
                    before: state.lines[step.first_line..end].to_vec(),
                    total_lines_before: state.lines.len(),
                    cursor: (state.cursor_line, state.cursor_col),
                };

                state.lines.splice(step.first_line..end, step.before);
                state.cursor_line = step.cursor.0.min(state.lines.len().saturating_sub(1));
                state.cursor_col = step.cursor.1.min(state.lines[state.cursor_line].chars().count());
                state.selection_anchor = None;
                state.modified = true;

                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }

                let other = if undoing { &mut state.redo_stack } else { &mut state.undo_stack };
                other.push(inverse);
                if other.len() > crate::constants::UNDO_LIMIT {
                    other.remove(0);
                }

                from = Some(step.first_line);
            }
        }

        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    /// Remove the selected range, leaving the cursor where the selection began.
    /// Returns the line to re-highlight from, or None if nothing was selected.
    /// Shared by cut, paste, typing, Backspace and Delete.
    fn delete_selection(&mut self) -> Option<usize> {
        let state = self.editor_state.as_mut()?;
        let ((first_line, first_col), (last_line, last_col)) = state.selection()?;

        let head = char_slice(&state.lines[first_line], 0, first_col);
        let last = &state.lines[last_line];
        let tail = char_slice(last, last_col, last.chars().count());

        state.lines.splice(first_line..=last_line, [format!("{head}{tail}")]);
        state.cursor_line = first_line;
        state.cursor_col = first_col;
        state.selection_anchor = None;
        state.modified = true;
        Some(first_line)
    }

    pub fn editor_select_all(&mut self) {
        let height = self.editor_viewport_height;
        if let Some(state) = &mut self.editor_state {
            let last_line = state.lines.len().saturating_sub(1);
            state.selection_anchor = Some((0, 0));
            state.cursor_line = last_line;
            state.cursor_col = state.lines[last_line].chars().count();
            state.scroll_offset = last_line.saturating_sub(height.saturating_sub(1));
        }
    }

    pub fn editor_copy(&mut self) {
        if let Some(text) = self.editor_state.as_ref().and_then(|state| state.selected_text()) {
            // Offer it to the terminal as well, so it reaches the system
            // clipboard where OSC 52 is supported.
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
    }

    pub fn editor_cut(&mut self) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.editor_copy();
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
        }
    }

    pub fn editor_paste(&mut self) {
        let text = self.clipboard.clone();
        self.editor_insert_text(&text);
    }

    /// Insert text at the cursor, replacing any selection. Serves both the
    /// internal clipboard and a bracketed paste arriving from the terminal.
    pub fn editor_insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));

        // A pasted Windows clipboard arrives with CRLF; the buffer holds lines.
        let text = text.replace("\r\n", "\n");

        // A selection is replaced by what is pasted over it.
        let mut rehighlight = self.delete_selection();

        if let Some(state) = &mut self.editor_state {
            let line = state.lines[state.cursor_line].clone();
            let head = char_slice(&line, 0, state.cursor_col);
            let tail = char_slice(&line, state.cursor_col, line.chars().count());
            let pieces: Vec<&str> = text.split('\n').collect();
            let start_line = state.cursor_line;

            if let [only] = pieces[..] {
                state.lines[start_line] = format!("{head}{only}{tail}");
                state.cursor_col += only.chars().count();
            } else {
                let last = pieces.len() - 1;
                let mut replacement = Vec::with_capacity(pieces.len());
                replacement.push(format!("{head}{}", pieces[0]));
                replacement.extend(pieces[1..last].iter().map(|piece| (*piece).to_string()));
                replacement.push(format!("{}{tail}", pieces[last]));

                state.lines.splice(start_line..=start_line, replacement);
                state.cursor_line = start_line + last;
                state.cursor_col = pieces[last].chars().count();
            }

            state.modified = true;
            state.selection_anchor = None;
            rehighlight = Some(rehighlight.unwrap_or(start_line).min(start_line));
        }

        if let Some(from) = rehighlight {
            self.editor_rehighlight_from(from);
        }
    }

    /// Called before a cursor move: Shift extends the selection from where the
    /// cursor was, anything else drops it.
    pub fn editor_prepare_move(&mut self, extend: bool) {
        if let Some(state) = &mut self.editor_state {
            if extend {
                if state.selection_anchor.is_none() {
                    state.selection_anchor = Some((state.cursor_line, state.cursor_col));
                }
            } else {
                state.selection_anchor = None;
            }
        }
    }

    pub fn editor_cursor_up(&mut self) {
        if let Some(state) = &mut self.editor_state {
            if state.cursor_line > 0 {
                state.cursor_line -= 1;
                state.clamp_col();
                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }
            }
        }
    }

    pub fn editor_cursor_down(&mut self) {
        if let Some(state) = &mut self.editor_state {
            if state.cursor_line < state.lines.len().saturating_sub(1) {
                state.cursor_line += 1;
                state.clamp_col();
                if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                    state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
                }
            }
        }
    }

    pub fn editor_cursor_left(&mut self) {
        if let Some(state) = &mut self.editor_state {
            if state.cursor_col > 0 {
                state.cursor_col -= 1;
            } else if state.cursor_line > 0 {
                state.cursor_line -= 1;
                state.cursor_col = state.lines[state.cursor_line].chars().count();
                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }
            }
        }
    }

    pub fn editor_cursor_right(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let line_len = state.lines[state.cursor_line].chars().count();
            if state.cursor_col < line_len {
                state.cursor_col += 1;
            } else if state.cursor_line < state.lines.len().saturating_sub(1) {
                state.cursor_line += 1;
                state.cursor_col = 0;
                if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                    state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
                }
            }
        }
    }

    pub fn editor_home(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.cursor_col = 0;
        }
    }

    pub fn editor_end(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.cursor_col = state.lines[state.cursor_line].chars().count();
        }
    }

    pub fn editor_page_up(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let page = self.editor_viewport_height.saturating_sub(1);
            state.cursor_line = state.cursor_line.saturating_sub(page);
            state.scroll_offset = state.scroll_offset.saturating_sub(page);
            state.clamp_col();
        }
    }

    pub fn editor_page_down(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let page = self.editor_viewport_height.saturating_sub(1);
            let max_line = state.lines.len().saturating_sub(1);
            state.cursor_line = (state.cursor_line + page).min(max_line);
            let max_scroll = state.lines.len().saturating_sub(self.editor_viewport_height);
            state.scroll_offset = (state.scroll_offset + page).min(max_scroll);
            state.clamp_col();
        }
    }

    pub fn editor_insert_char(&mut self, c: char) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        // Typing over a selection replaces it.
        self.delete_selection();
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            let line = &mut state.lines[state.cursor_line];
            let byte_idx = char_to_byte(line, state.cursor_col);
            line.insert(byte_idx, c);
            state.cursor_col += 1;
            state.modified = true;
            from = Some(state.cursor_line);
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    pub fn editor_backspace(&mut self) {
        let fallback = match self.editor_state.as_ref() {
            // Joining with the line above puts that line in range too.
            Some(state) if state.cursor_col == 0 => state.cursor_line.saturating_sub(1)..=state.cursor_line,
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        // With a selection, Backspace removes that rather than a character.
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            return;
        }
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            if state.cursor_col > 0 {
                let line = &mut state.lines[state.cursor_line];
                let byte_start = char_to_byte(line, state.cursor_col - 1);
                let byte_end = char_to_byte(line, state.cursor_col);
                line.replace_range(byte_start..byte_end, "");
                state.cursor_col -= 1;
                state.modified = true;
                from = Some(state.cursor_line);
            } else if state.cursor_line > 0 {
                let current_line = state.lines.remove(state.cursor_line);
                state.cursor_line -= 1;
                state.cursor_col = state.lines[state.cursor_line].chars().count();
                state.lines[state.cursor_line].push_str(&current_line);
                state.modified = true;
                from = Some(state.cursor_line);
                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    pub fn editor_delete(&mut self) {
        let fallback = match self.editor_state.as_ref() {
            // At end of line the next line is pulled up, so include it.
            Some(state) if state.cursor_col >= state.lines[state.cursor_line].chars().count() => {
                state.cursor_line..=state.cursor_line + 1
            }
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            return;
        }
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            let line_len = state.lines[state.cursor_line].chars().count();
            if state.cursor_col < line_len {
                let line = &mut state.lines[state.cursor_line];
                let byte_start = char_to_byte(line, state.cursor_col);
                let byte_end = char_to_byte(line, state.cursor_col + 1);
                line.replace_range(byte_start..byte_end, "");
                state.modified = true;
                from = Some(state.cursor_line);
            } else if state.cursor_line < state.lines.len().saturating_sub(1) {
                let next_line = state.lines.remove(state.cursor_line + 1);
                state.lines[state.cursor_line].push_str(&next_line);
                state.modified = true;
                from = Some(state.cursor_line);
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    pub fn editor_enter(&mut self) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.delete_selection();
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            from = Some(state.cursor_line);
            let line = &mut state.lines[state.cursor_line];
            let byte_idx = char_to_byte(line, state.cursor_col);
            let new_line = line[byte_idx..].to_string();
            line.truncate(byte_idx);
            state.cursor_line += 1;
            state.lines.insert(state.cursor_line, new_line);
            state.cursor_col = 0;
            state.modified = true;
            if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    pub fn editor_save(&mut self) -> Result<(), String> {
        if let Some(state) = &mut self.editor_state {
            let content = state.lines.join(state.line_ending);
            std::fs::write(&state.file_path, content).map_err(|e| e.to_string())?;
            state.modified = false;
        }
        Ok(())
    }

    pub fn editor_is_modified(&self) -> bool {
        self.editor_state.as_ref().is_some_and(|s| s.modified)
    }

    pub fn reset_copy(&mut self) {
        self.is_f5_displayed = false;
        self.copy_items.clear();
    }

    pub fn reset_move(&mut self) {
        self.is_f6_displayed = false;
        self.move_items.clear();
    }

    pub fn toggle_selection(&mut self) {
        self.toggle_selection_inner(true);
    }

    pub fn toggle_selection_no_size(&mut self) {
        self.toggle_selection_inner(false);
    }

    fn toggle_selection_inner(&mut self, calculate_size: bool) {
        use crate::fs_ops::calculate_dir_size;

        let mut error_msg: Option<String> = None;
        let mut dir_size_result: Option<(PathBuf, u64)> = None;

        {
            let (state, children, selected_set, current_dir) = if self.is_left_active {
                (&mut self.state_left, &self.children_left, &mut self.selected_left, &self.dir_left)
            } else {
                (&mut self.state_right, &self.children_right, &mut self.selected_right, &self.dir_right)
            };

            if let Some(index) = state.selected() {
                if index < children.len() && children[index].name != ".." {
                    let item = &children[index];

                    if !selected_set.remove(&item.name_full) {
                        selected_set.insert(item.name_full.clone());

                        if calculate_size && item.is_dir {
                            let full_path = current_dir.join(&item.name_full);
                            match calculate_dir_size(&full_path) {
                                Ok(size) => dir_size_result = Some((full_path, size)),
                                Err(e) => error_msg = Some(format!("Cannot calculate size: {}", e)),
                            }
                        }
                    }
                }

                // Move to next item
                let len = children.len();
                if len > 0 {
                    state.select(Some((index + 1).min(len - 1)));
                }
            }
        }

        if let Some((path, size)) = dir_size_result {
            self.dir_sizes.insert(path, size);
        }
        if let Some(msg) = error_msg {
            self.display_error(msg);
        }
    }

    /// True while a dialog, prompt, viewer or editor owns the screen.
    pub fn is_modal_open(&self) -> bool {
        self.is_error_displayed
            || self.is_f1_displayed
            || self.is_f11_displayed
            || self.is_f2_displayed
            || self.is_f3_displayed
            || self.is_f4_displayed
            || self.is_f5_displayed
            || self.is_f6_displayed
            || self.is_f7_displayed
            || self.is_f8_displayed
            || self.is_editor_save_prompt
            || self.large_file.is_some()
    }

    /// Remember a directory's mtime so a later change to it stands out.
    pub fn record_dir_stamp(&mut self, is_left: bool) {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        let stamp = std::fs::metadata(dir).and_then(|metadata| metadata.modified()).ok();
        if is_left {
            self.dir_stamp_left = stamp;
        } else {
            self.dir_stamp_right = stamp;
        }
        self.record_disk_usage(is_left);
    }

    /// Read the panel filesystem's used/total. Off the render path: statvfs is
    /// a syscall and blocks outright on an unresponsive network mount.
    pub fn record_disk_usage(&mut self, is_left: bool) {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        let usage = disk_usage(dir);
        if is_left {
            self.disk_left = usage;
        } else {
            self.disk_right = usage;
        }
    }

    /// Point a panel at `dir` and read it. The single way a panel's directory
    /// changes - navigation, and anything else that jumps somewhere. `select`
    /// names the entry to land on, otherwise the cursor goes to the top.
    pub fn open_dir(&mut self, is_left: bool, dir: PathBuf, select: Option<&str>) {
        if is_left {
            self.dir_left = dir;
            self.selected_left.clear();
            self.state_left.select(Some(0));
        } else {
            self.dir_right = dir;
            self.selected_right.clear();
            self.state_right.select(Some(0));
        }
        self.search_clear();
        // Handles a vanished target too, by climbing to the nearest parent.
        self.reload_panel(is_left, select);
    }

    /// Reread one panel from disk. `prefer` names the entry to land on - the
    /// file just renamed or created. Otherwise the cursor keeps the *file* it
    /// was on rather than the row, since entries appearing or vanishing above
    /// shift every index below them; if that file is gone, the row is kept.
    pub fn reload_panel(&mut self, is_left: bool, prefer: Option<&str>) {
        let dir = if is_left { self.dir_left.clone() } else { self.dir_right.clone() };

        // The directory may have been removed underneath us; climb to the
        // nearest ancestor that still exists rather than sitting on an error.
        let (dir, relocated) = match nearest_existing_dir(&dir) {
            Some(found) if found == dir => (dir, false),
            Some(found) => (found, true),
            None => {
                self.display_error(format!("No such directory: {}", dir.display()));
                return;
            }
        };

        if relocated {
            // Cursor, search and selection all referred to entries that are gone.
            if is_left {
                self.dir_left = dir.clone();
                self.selected_left.clear();
                self.state_left.select(Some(0));
            } else {
                self.dir_right = dir.clone();
                self.selected_right.clear();
                self.state_right.select(Some(0));
            }
            self.search_clear();
        }

        let (children, state) = if is_left {
            (&self.children_left, &self.state_left)
        } else {
            (&self.children_right, &self.state_right)
        };
        let previous_index = state.selected().unwrap_or(0);
        let wanted = if relocated {
            None
        } else {
            prefer.map(str::to_string).or_else(|| {
                children.get(previous_index).map(|item| item.name_full.clone())
            })
        };

        match load_directory_rows(&dir) {
            Ok(items) => {
                let index = wanted
                    .and_then(|name| items.iter().position(|item| item.name_full == name))
                    .unwrap_or(previous_index)
                    .min(items.len().saturating_sub(1));

                if is_left {
                    self.children_left = items;
                    self.state_left.select(Some(index));
                } else {
                    self.children_right = items;
                    self.state_right.select(Some(index));
                }
                self.record_dir_stamp(is_left);
            }
            Err(e) => self.display_error(e.to_string()),
        }
    }

    /// The mount a panel is sitting on: the longest one its directory is under.
    pub fn current_mount(&self, is_left: bool) -> Option<usize> {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| dir.starts_with(&mount.path))
            .max_by_key(|(_, mount)| mount.path.as_os_str().len())
            .map(|(index, _)| index)
    }

    /// Start choosing a drive for a panel, highlighting the one it is on.
    pub fn open_drive_picker(&mut self, is_left: bool) {
        // Re-read so a stick plugged in a moment ago shows up.
        self.mounts = list_mounts();
        if self.mounts.is_empty() {
            return;
        }
        let current = self.current_mount(is_left).unwrap_or(0);
        self.drive_picker = Some((is_left, current));
    }

    pub fn move_drive_picker(&mut self, forward: bool) {
        if let Some((is_left, index)) = self.drive_picker {
            let count = self.mounts.len();
            let next = if forward { (index + 1) % count } else { (index + count - 1) % count };
            self.drive_picker = Some((is_left, next));
        }
    }

    /// Take the highlighted drive; the panel jumps to that mount point.
    pub fn confirm_drive_picker(&mut self) {
        if let Some((is_left, index)) = self.drive_picker.take() {
            if let Some(mount) = self.mounts.get(index) {
                let path = mount.path.clone();
                self.open_dir(is_left, path, None);
            }
        }
    }

    /// Reread any panel whose directory changed underneath us. Called once per
    /// frame; the interval keeps it to a couple of stat calls a second.
    pub fn refresh_stale_panels(&mut self) {
        // Reloading under an open dialog would move things out from under the user.
        if self.is_modal_open() || self.last_refresh_check.elapsed() < crate::constants::REFRESH_INTERVAL {
            return;
        }
        self.last_refresh_check = Instant::now();

        // A drive appearing or going away should show up in the strip.
        self.mounts = list_mounts();

        for is_left in [true, false] {
            // Free space moves without the directory changing - a copy anywhere
            // else on the same filesystem shifts it - so this updates every tick.
            self.record_disk_usage(is_left);

            let dir = if is_left { &self.dir_left } else { &self.dir_right };
            let stamp = std::fs::metadata(dir).and_then(|metadata| metadata.modified()).ok();
            let known = if is_left { self.dir_stamp_left } else { self.dir_stamp_right };
            if stamp != known {
                self.reload_panel(is_left, None);
            }
        }
    }

    /// Full path of the entry under the cursor, unless that is the parent entry.
    fn cursor_target(&self) -> Option<(PathBuf, String, bool)> {
        let (children, state, dir) = if self.is_left_active {
            (&self.children_left, &self.state_left, &self.dir_left)
        } else {
            (&self.children_right, &self.state_right, &self.dir_right)
        };
        let item = children.get(state.selected()?)?;
        if item.name == ".." {
            return None;
        }
        Some((dir.join(&item.name_full), item.name_full.clone(), item.is_dir))
    }

    /// Keep the preview pointed at whatever the cursor is on. Reads only when
    /// the target actually changed, so holding an arrow key stays cheap.
    pub fn refresh_preview(&mut self) {
        if !self.is_f12_displayed {
            self.preview = None;
            return;
        }

        let target = self.cursor_target();
        let path = target.as_ref().map(|(path, _, _)| path.clone());
        if self.preview.as_ref().is_some_and(|preview| preview.path == path) {
            return;
        }

        let (label, lines) = match &target {
            None => (String::new(), Vec::new()),
            Some((path, name, true)) => {
                let lines = match std::fs::read_dir(path) {
                    Ok(entries) => vec![format!("{} items", entries.count())],
                    Err(e) => vec![e.to_string()],
                };
                (name.clone(), lines)
            }
            Some((path, name, false)) => (
                name.clone(),
                crate::viewer::load_preview(path, crate::constants::PREVIEW_MAX_BYTES, crate::constants::PREVIEW_MAX_LINES),
            ),
        };

        self.preview = Some(PreviewState { path, label, lines });
    }

    pub fn clear_all_selections(&mut self) {
        self.selected_left.clear();
        self.selected_right.clear();
    }

    pub fn clear_active_selections(&mut self) {
        if self.is_left_active {
            self.selected_left.clear();
        } else {
            self.selected_right.clear();
        }
    }
}

impl EditorState {
    /// The selection as ((line, col), (line, col)) in document order, or None
    /// when nothing is selected.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.selection_anchor?;
        let cursor = (self.cursor_line, self.cursor_col);
        if anchor == cursor {
            return None;
        }
        Some(if anchor < cursor { (anchor, cursor) } else { (cursor, anchor) })
    }

    /// The selected text, with '\n' between lines whatever the file uses.
    pub fn selected_text(&self) -> Option<String> {
        let ((first_line, first_col), (last_line, last_col)) = self.selection()?;

        if first_line == last_line {
            return Some(char_slice(&self.lines[first_line], first_col, last_col));
        }

        let first = &self.lines[first_line];
        let mut text = char_slice(first, first_col, first.chars().count());
        for line in &self.lines[first_line + 1..last_line] {
            text.push('\n');
            text.push_str(line);
        }
        text.push('\n');
        text.push_str(&char_slice(&self.lines[last_line], 0, last_col));
        Some(text)
    }

    /// Clamp cursor_col to current line length.
    fn clamp_col(&mut self) {
        let line_len = self.lines[self.cursor_line].chars().count();
        self.cursor_col = self.cursor_col.min(line_len);
    }
}

/// Pick the terminator to save a file with. A mixed file normalises to whichever
/// style already dominates; a file with no newline at all gets "\n".
fn detect_line_ending(content: &str) -> &'static str {
    let crlf = content.matches("\r\n").count();
    let lf = content.matches('\n').count() - crlf;
    if crlf > lf { "\r\n" } else { "\n" }
}

/// A character range of a line, as a new String.
fn char_slice(line: &str, from: usize, to: usize) -> String {
    line[char_to_byte(line, from)..char_to_byte(line, to)].to_string()
}

/// Convert a char index to a byte index in a string.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
