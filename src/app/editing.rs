//! F4: the built-in editor.

use super::*;

/// What a save in the editor goes on to do once the file is written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterSave {
    /// Back to editing.
    Stay,
    /// Close the editor.
    Close,
    /// Close the editor, and fm84 with it: the save was F10's.
    Quit,
}

/// A file as it stood on disk: when it was last modified, and its size.
/// Together they tell a file something else has written apart from the one
/// the editor read, even on a filesystem that keeps time only to the second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiskStamp {
    pub modified: Option<SystemTime>,
    pub size: u64,
}

impl DiskStamp {
    /// The file at `path` as it stands now, through a link to what it names
    /// - which is what a save writes. None when there is nothing there.
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(DiskStamp {
            modified: metadata.modified().ok(),
            size: metadata.len(),
        })
    }
}

/// How a save from the editor came out, when it did not fail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Saved {
    Done,
    /// Not written: something else changed the file since the editor read
    /// it, and writing would throw that away. The stamps are the file as it
    /// was read, and as it is now.
    ChangedOnDisk {
        opened: Option<DiskStamp>,
        now: Option<DiskStamp>,
    },
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
    /// The file as it stood when it was last read or written here, so a save
    /// can tell when something else has written it since.
    pub disk_stamp: Option<DiskStamp>,
}

/// One undoable edit: the lines it replaced, and where the cursor was. The
/// number of lines that took their place is worked out at undo time from how
/// the buffer's length changed, so nothing has to be recorded afterwards.
#[derive(Clone)]
pub struct EditStep {
    pub(super) first_line: usize,
    pub(super) before: Vec<String>,
    pub(super) total_lines_before: usize,
    pub(super) cursor: (usize, usize),
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

    /// Settle the vertical scroll before a frame is drawn. It never runs past
    /// the last screenful - an edit that shortens the file, such as deleting a
    /// selection made with Ctrl+A, would otherwise leave it pointing past the
    /// end - and after a key, which sets auto_scroll, it brings the cursor into
    /// view: a paste, an undo or a deleted selection can each land it off
    /// screen. The mouse wheel clears auto_scroll, so reading elsewhere in the
    /// file does not snap back to the cursor.
    pub fn keep_in_view(&mut self, height: usize) {
        let height = height.max(1);
        if self.auto_scroll {
            if self.cursor_line < self.scroll_offset {
                self.scroll_offset = self.cursor_line;
            } else if self.cursor_line >= self.scroll_offset + height {
                self.scroll_offset = self.cursor_line + 1 - height;
            }
        }
        self.scroll_offset = self.scroll_offset.min(self.lines.len().saturating_sub(height));
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
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
}

#[cfg(test)]
impl AppState {
    /// Put the editor's cursor somewhere, with nothing selected.
    fn editor_state_cursor(&mut self, line: usize, col: usize) {
        if let Some(state) = self.editor_mut() {
            state.cursor_line = line;
            state.cursor_col = col;
            state.selection_anchor = None;
        }
    }
}

impl AppState {
    /// The file open in the editor, if that is what is on screen.
    pub fn editor(&self) -> Option<&EditorState> {
        match &self.screen {
            Screen::Editor(state) => Some(state),
            _ => None,
        }
    }

    pub fn editor_mut(&mut self) -> Option<&mut EditorState> {
        match &mut self.screen {
            Screen::Editor(state) => Some(state),
            _ => None,
        }
    }

    pub fn is_editing(&self) -> bool {
        matches!(self.screen, Screen::Editor(_))
    }

    pub fn open_editor(&mut self, file_path: PathBuf) -> Result<(), String> {
        use crate::viewer::{highlight_all, is_binary_file};

        let file_size = std::fs::metadata(&file_path).map_err(|e| e.to_string())?.len();
        // Taken before the file is read: a write between the two then makes
        // the save ask needlessly, rather than go over it without a word.
        let disk_stamp = DiskStamp::of(&file_path);

        if is_binary_file(&file_path).unwrap_or(false) {
            self.open_viewer(file_path)?;
            if let Screen::Viewer(state) = &mut self.screen {
                // Editing is refused outright; F3 is where a binary gets read.
                state.from_edit = true;
                state.mode = ViewMode::Text;
                state.bytes = Vec::new();
                state.image = None;
                state.image_lines = Vec::new();
                state.image_colors = Vec::new();
                state.content_lines = Vec::new();
                state.total_lines = 1;
            }
            return Ok(());
        }

        // The binary check reads only the head of the file, so a byte that is
        // not UTF-8 can still be waiting further in. Opened lossily, saving
        // would write a replacement character over every one of them, so the
        // file is refused - with a reason, rather than the decoder's own.
        let content = String::from_utf8(std::fs::read(&file_path).map_err(|e| e.to_string())?).map_err(|_| format!("Cannot edit {}: it is not UTF-8 text. F3 shows it as it is.", file_path.display()))?;
        let line_ending = detect_line_ending(&content);
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if content.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // The initial parse measures about 0.9s per megabyte, which is what the
        // limit in F11 caps; editing stays fast afterwards regardless of length.
        let limit = self.options.highlight_limit_bytes();
        let (highlighted_lines, line_states) = if limit > 0 && file_size <= limit {
            highlight_all(&lines, extension)
        } else {
            // Rendering already falls back to plain text when these are empty.
            (Vec::new(), Vec::new())
        };

        self.screen = Screen::Editor(EditorState {
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
            disk_stamp,
        });
        Ok(())
    }

    /// R on the changed-on-disk question: read the file again as it now is,
    /// dropping the edits, and keep the cursor on the line it was on, as far
    /// as the file still reaches.
    pub fn editor_reload(&mut self) -> Result<(), String> {
        let Some((path, line)) = self.editor().map(|state| (state.file_path.clone(), state.cursor_line)) else {
            return Ok(());
        };
        self.open_editor(path)?;
        if let Some(state) = self.editor_mut() {
            state.cursor_line = line.min(state.lines.len().saturating_sub(1));
        }
        Ok(())
    }

    pub fn close_editor(&mut self) {
        self.screen = Screen::Panels;
        self.close_find();
    }

    /// Re-highlight the file from `from` downward. Cheap: the cached state lets
    /// it resume mid-file and stop again as soon as the parse converges.
    pub fn editor_rehighlight_from(&mut self, from: usize) {
        self.editor_rehighlight_through(from, from);
    }

    /// Re-highlight after an edit that changed every line from `from` to
    /// `through`. The parse may stop once it meets the cache, but not before
    /// `through`: a line below one the cache agrees with can still be new
    /// text, and its old spans would go on showing what it said before.
    pub(super) fn editor_rehighlight_through(&mut self, from: usize, through: usize) {
        if let Screen::Editor(state) = &mut self.screen {
            if state.line_states.is_empty() {
                return; // highlighting disabled for this file
            }
            let extension = state.file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            crate::viewer::highlight_through(&state.lines, extension, from, through, &mut state.highlighted_lines, &mut state.line_states);
        }
    }

    pub fn editor_scroll_up(&mut self) {
        if let Screen::Editor(state) = &mut self.screen
            && state.scroll_offset > 0
        {
            state.scroll_offset -= 1;
            state.auto_scroll = false;
        }
    }

    pub fn editor_scroll_down(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            let max = state.lines.len().saturating_sub(self.editor_viewport_height);
            if state.scroll_offset < max {
                state.scroll_offset += 1;
                state.auto_scroll = false;
            }
        }
    }

    pub fn editor_scroll_left(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
            state.auto_scroll = false;
        }
    }

    pub fn editor_scroll_right(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            state.horizontal_offset += 1;
            state.auto_scroll = false;
        }
    }

    /// Remember the lines `range` covers before they are replaced. Called once
    /// per user action, with a range spanning everything that action touches.
    pub(super) fn push_undo(&mut self, range: std::ops::RangeInclusive<usize>) {
        if let Screen::Editor(state) = &mut self.screen {
            let last = (*range.end()).min(state.lines.len().saturating_sub(1));
            let first = (*range.start()).min(last);

            state.undo_stack.push(EditStep {
                first_line: first,
                before: state.lines[first..=last].to_vec(),
                total_lines_before: state.lines.len(),
                cursor: (state.cursor_line, state.cursor_col),
            });
        }
    }

    /// Settle the step push_undo recorded, once the action has run. One that
    /// changed nothing - Backspace at the very start, Delete at the very end,
    /// Ctrl+X with nothing selected - is dropped, so it neither costs an undo
    /// that does nothing nor throws away the redo history. One that did change
    /// something forks the history: editing after undoing loses the old branch.
    pub(super) fn finish_edit(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            let Some(step) = state.undo_stack.last() else {
                return;
            };
            let unchanged = state.lines.len() == step.total_lines_before && state.lines[step.first_line..step.first_line + step.before.len()] == step.before[..];
            if unchanged {
                state.undo_stack.pop();
                return;
            }
            state.redo_stack.clear();
            if state.undo_stack.len() > crate::constants::UNDO_LIMIT {
                state.undo_stack.remove(0);
            }
        }
    }

    /// The range a user action is about to touch: the selection when there is
    /// one, otherwise the given fallback.
    pub(super) fn edit_range(&self, fallback: std::ops::RangeInclusive<usize>) -> std::ops::RangeInclusive<usize> {
        match self.editor().and_then(|state| state.selection()) {
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
        let mut changed = None;

        if let Screen::Editor(state) = &mut self.screen {
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

                // Every line put back is changed, not only the first of them.
                let through = step.first_line + step.before.len().saturating_sub(1);
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

                changed = Some((step.first_line, through));
            }
        }

        if let Some((from, through)) = changed {
            self.editor_rehighlight_through(from, through);
        }
    }

    /// Remove the selected range, leaving the cursor where the selection began.
    /// Returns the line to re-highlight from, or None if nothing was selected.
    /// Shared by cut, paste, typing, Backspace and Delete.
    pub(super) fn delete_selection(&mut self) -> Option<usize> {
        let state = self.editor_mut()?;
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
        if let Screen::Editor(state) = &mut self.screen {
            let last_line = state.lines.len().saturating_sub(1);
            state.selection_anchor = Some((0, 0));
            state.cursor_line = last_line;
            state.cursor_col = state.lines[last_line].chars().count();
            state.scroll_offset = last_line.saturating_sub(height.saturating_sub(1));
        }
    }

    pub fn editor_copy(&mut self) {
        if let Some(text) = self.editor().and_then(|state| state.selected_text()) {
            // Offer it to the terminal as well, so it reaches the system
            // clipboard where OSC 52 is supported.
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
    }

    pub fn editor_cut(&mut self) {
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.editor_copy();
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
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
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));

        // A pasted Windows clipboard arrives with CRLF; the buffer holds lines.
        let text = text.replace("\r\n", "\n");

        // A selection is replaced by what is pasted over it.
        let mut rehighlight = self.delete_selection().map(|from| (from, from));

        if let Screen::Editor(state) = &mut self.screen {
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
            // Every line the paste wrote, down to the one the cursor ends on.
            let from = rehighlight.map_or(start_line, |(from, _)| from.min(start_line));
            rehighlight = Some((from, state.cursor_line));
        }

        if let Some((from, through)) = rehighlight {
            self.editor_rehighlight_through(from, through);
        }
        self.finish_edit();
    }

    /// Called before a cursor move: Shift extends the selection from where the
    /// cursor was, anything else drops it.
    pub fn editor_prepare_move(&mut self, extend: bool) {
        if let Screen::Editor(state) = &mut self.screen {
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
        if let Screen::Editor(state) = &mut self.screen
            && state.cursor_line > 0
        {
            state.cursor_line -= 1;
            state.clamp_col();
            if state.cursor_line < state.scroll_offset {
                state.scroll_offset = state.cursor_line;
            }
        }
    }

    pub fn editor_cursor_down(&mut self) {
        if let Screen::Editor(state) = &mut self.screen
            && state.cursor_line < state.lines.len().saturating_sub(1)
        {
            state.cursor_line += 1;
            state.clamp_col();
            if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
            }
        }
    }

    pub fn editor_cursor_left(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
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
        if let Screen::Editor(state) = &mut self.screen {
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
        if let Screen::Editor(state) = &mut self.screen {
            state.cursor_col = 0;
        }
    }

    pub fn editor_end(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            state.cursor_col = state.lines[state.cursor_line].chars().count();
        }
    }

    pub fn editor_page_up(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            let page = self.editor_viewport_height.saturating_sub(1);
            state.cursor_line = state.cursor_line.saturating_sub(page);
            state.scroll_offset = state.scroll_offset.saturating_sub(page);
            state.clamp_col();
        }
    }

    pub fn editor_page_down(&mut self) {
        if let Screen::Editor(state) = &mut self.screen {
            let page = self.editor_viewport_height.saturating_sub(1);
            let max_line = state.lines.len().saturating_sub(1);
            state.cursor_line = (state.cursor_line + page).min(max_line);
            let max_scroll = state.lines.len().saturating_sub(self.editor_viewport_height);
            state.scroll_offset = (state.scroll_offset + page).min(max_scroll);
            state.clamp_col();
        }
    }

    pub fn editor_insert_char(&mut self, c: char) {
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        // Typing over a selection replaces it.
        self.delete_selection();
        let mut from = None;
        if let Screen::Editor(state) = &mut self.screen {
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
        self.finish_edit();
    }

    pub fn editor_backspace(&mut self) {
        let fallback = match self.editor() {
            // Joining with the line above puts that line in range too.
            Some(state) if state.cursor_col == 0 => state.cursor_line.saturating_sub(1)..=state.cursor_line,
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        // With a selection, Backspace removes that rather than a character.
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            self.finish_edit();
            return;
        }
        let mut from = None;
        if let Screen::Editor(state) = &mut self.screen {
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
        self.finish_edit();
    }

    pub fn editor_delete(&mut self) {
        let fallback = match self.editor() {
            // At end of line the next line is pulled up, so include it.
            Some(state) if state.cursor_col >= state.lines[state.cursor_line].chars().count() => state.cursor_line..=state.cursor_line + 1,
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            self.finish_edit();
            return;
        }
        let mut from = None;
        if let Screen::Editor(state) = &mut self.screen {
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
        self.finish_edit();
    }

    pub fn editor_enter(&mut self) {
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.delete_selection();
        let mut from = None;
        if let Screen::Editor(state) = &mut self.screen {
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
        self.finish_edit();
    }

    /// Save the file in the editor - unless something else has written it
    /// since it was read here, when nothing is written and the caller asks
    /// first. A file gone altogether counts: saving would bring back what
    /// someone meant to delete.
    pub fn editor_save(&mut self) -> Result<Saved, String> {
        if let Some(state) = self.editor() {
            let now = DiskStamp::of(&state.file_path);
            if now != state.disk_stamp {
                return Ok(Saved::ChangedOnDisk { opened: state.disk_stamp, now });
            }
        }
        self.editor_save_over()?;
        Ok(Saved::Done)
    }

    /// Save the file in the editor, whatever is on disk.
    pub fn editor_save_over(&mut self) -> Result<(), String> {
        if let Screen::Editor(state) = &mut self.screen {
            let content = state.lines.join(state.line_ending);
            crate::fs_ops::save_file(&state.file_path, content.as_bytes()).map_err(|e| e.to_string())?;
            state.modified = false;
            // What is there now is this, and the next save should not take
            // it for somebody else's.
            state.disk_stamp = DiskStamp::of(&state.file_path);
            // A save written in place leaves its directory's mtime alone - only
            // adding, removing or renaming entries moves that - so the refresh
            // that watches it never notices, and the panels would go on showing
            // the size from before the edit. Either of them may be showing the
            // file, and a link to it may be in a directory neither is watching.
            self.reload_panel(true, None);
            self.reload_panel(false, None);
        }
        Ok(())
    }

    pub fn editor_is_modified(&self) -> bool {
        self.editor().is_some_and(|s| s.modified)
    }
}

#[cfg(test)]
mod tests;
