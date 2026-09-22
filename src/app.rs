use crate::fs_ops::get_current_dir;
use crate::viewer::ViewerState;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

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
    pub is_f2_displayed: bool,
    pub is_f7_displayed: bool,
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
    /// Parser state entering each line, so an edit re-parses from that line
    /// instead of the whole file. Empty when highlighting is off.
    pub line_states: Vec<crate::viewer::LineState>,
    /// The terminator this file was written with, so saving doesn't rewrite
    /// every line of a CRLF file just because one character changed.
    pub line_ending: &'static str,
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
            is_f2_displayed: false,
            is_f7_displayed: false,
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

    pub fn viewer_scroll_right(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            // Stop once the longest line's end reaches the right edge.
            let max = state.max_line_width.saturating_sub(self.viewer_viewport_width);
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
                state.from_edit = true;
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
            line_ending,
            line_states,
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

/// Convert a char index to a byte index in a string.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
