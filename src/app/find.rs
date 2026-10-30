//! Find, go to line and replace, in the viewer and the editor; and the prompt
//! they are typed into.

use super::*;

impl AppState {
    /// The prompt and the marks go with the file they were for. The term is
    /// kept, to be offered again.
    pub(super) fn close_find(&mut self) {
        if matches!(self.dialog, Some(Dialog::Prompt(..))) {
            self.dialog = None;
        }
        self.find_shown = false;
        self.find_note = None;
    }

    /// Ask for something to find, or a line to go to. Find opens on the last
    /// term, so Enter alone looks for it again.
    pub fn open_prompt(&mut self, kind: PromptKind) {
        if !self.can_find() {
            return;
        }
        let mut input = TextInput::new();
        if kind == PromptKind::Find {
            input.set(self.find_term.clone());
        }
        self.dialog = Some(Dialog::Prompt(kind, input));
    }

    /// Only text can be searched or numbered: not a picture, and not the notice
    /// F4 raises on a binary file.
    pub fn can_find(&self) -> bool {
        match &self.screen {
            Screen::Editor(_) => true,
            Screen::Viewer(state) => !state.from_edit && state.mode != ViewMode::Image,
            Screen::Panels | Screen::Diff(_) => false,
        }
    }

    /// Enter on the prompt: look for what was typed, or go to the line.
    pub fn confirm_prompt(&mut self) {
        let Some(Dialog::Prompt(kind, input)) = self.dialog.take() else {
            return;
        };
        match kind {
            PromptKind::Find => {
                if input.text.is_empty() {
                    // Nothing to look for, so nothing to mark either.
                    self.find_shown = false;
                    return;
                }
                self.find_term = input.text;
                self.find(true, true);
            }
            PromptKind::GoToLine => match input.text.trim().parse::<usize>() {
                Ok(number) => self.go_to_line(number),
                Err(_) if input.text.trim().is_empty() => {}
                Err(_) => self.find_note = Some(format!("Not a line number: {}", input.text.trim())),
            },
            PromptKind::Select | PromptKind::Deselect => {
                let Some(glob) = crate::glob::Glob::new(&input.text) else {
                    return;
                };
                self.select_pattern = input.text;
                self.select_by(&glob, kind == PromptKind::Select);
            }
            // What to replace, and then what with. Nothing to replace is a
            // cancel; nothing to replace it with deletes it.
            PromptKind::Replace => {
                if input.text.is_empty() {
                    return;
                }
                self.find_term = input.text;
                let mut with = TextInput::new();
                with.set(self.replace_with.clone());
                self.dialog = Some(Dialog::Prompt(PromptKind::ReplaceWith, with));
            }
            PromptKind::ReplaceWith => {
                self.replace_with = input.text;
                self.start_replace();
            }
            PromptKind::GoTo => self.go_to_path(&input.text),
            PromptKind::Filter => {
                self.filter_before.clear();
                self.set_filter(self.is_left_active, &input.text);
            }
        }
    }

    /// Ctrl+R or F4 in the editor: ask what to replace, offering the text
    /// selected when it is all on one line, or else what Ctrl+F last looked
    /// for.
    pub fn open_replace(&mut self) {
        let Some(state) = self.editor() else {
            return;
        };
        let offered = state.selected_text().filter(|text| !text.contains('\n')).unwrap_or_else(|| self.find_term.clone());
        let mut input = TextInput::new();
        input.set(offered);
        self.dialog = Some(Dialog::Prompt(PromptKind::Replace, input));
    }

    /// Go to the first match from the cursor and ask about it.
    fn start_replace(&mut self) {
        let Some(needle) = crate::find::Needle::new(&self.find_term) else {
            return;
        };
        let Some(state) = self.editor() else {
            return;
        };
        let stop_at = state.selection().map_or((state.cursor_line, state.cursor_col), |(start, _)| start);
        self.find_shown = true;
        match self.editor_find(&needle, true, true) {
            Some(wrapped) => self.dialog = Some(Dialog::Replacing { stop_at, wrapped, replaced: 0 }),
            None => self.find_note = Some(format!("Not found: {}", self.find_term)),
        }
    }

    /// Y or N on the match asked about: replace it, or leave it, and go on to
    /// the next - or, back where it started, stop. True while there is
    /// another to ask about.
    pub fn replace_step(&mut self, replace: bool) -> bool {
        let Some(Dialog::Replacing { mut stop_at, mut wrapped, mut replaced }) = self.dialog.take() else {
            return false;
        };
        let Some(needle) = crate::find::Needle::new(&self.find_term) else {
            return false;
        };
        if replace && let Some(((line, start), (_, end))) = self.editor().and_then(EditorState::selection) {
            let with = self.replace_with.clone();
            self.replace_selection(&with);
            replaced += 1;
            // Gone round and back on the line it started on, text before the
            // starting point moves the starting point along with it.
            if wrapped && line == stop_at.0 && start < stop_at.1 {
                stop_at.1 = (stop_at.1 + with.chars().count()).saturating_sub(end - start);
            }
        }
        // On from the cursor, which is past the match: never into the text
        // just put in, so replacing "a" with "aa" cannot go on for ever.
        let next = self.editor_find(&needle, true, false);
        let at = self.editor().and_then(EditorState::selection).map(|(start, _)| start);
        // Done once round the end and back to where it started - or round the
        // end a second time, which is how it gets there when the last match
        // before the start was replaced and none is left after it: a search
        // that went on would find the text it put in, and never end.
        let finished = |went_round: bool, at: (usize, usize)| (wrapped && went_round) || ((wrapped || went_round) && at >= stop_at);
        match (next, at) {
            (Some(went_round), Some(at)) if !finished(went_round, at) => {
                wrapped |= went_round;
                self.dialog = Some(Dialog::Replacing { stop_at, wrapped, replaced });
                true
            }
            _ => {
                self.finish_replace(replaced);
                false
            }
        }
    }

    /// A on the match asked about: replace it and every one after it, round
    /// to where it started, as a single step for Ctrl+Z to take back.
    pub fn replace_all(&mut self) {
        let Some(state) = self.editor_mut() else {
            return;
        };
        let before = state.lines.clone();
        let cursor = (state.cursor_line, state.cursor_col);
        // Each replacement records its own step; those are put aside, and
        // one for the lot recorded in their place.
        let history = std::mem::take(&mut state.undo_stack);
        // Highlighting is held off while it replaces - redone after each, a
        // line with three matches was parsed three times over, and 60,000
        // replacements took over a second - and done once at the end, over
        // the lines that changed.
        let line_states = std::mem::take(&mut state.line_states);
        while self.replace_step(true) {}
        if let Some(state) = self.editor_mut() {
            state.line_states = line_states;
            let changed = state.lines != before;
            if changed && !state.line_states.is_empty() {
                // A replacement holds no line break, so the lines stay where
                // they were; should they not have, it is the whole file.
                let differs = |index: &usize| state.lines.get(*index) != before.get(*index);
                let (first, last) = if state.lines.len() == before.len() {
                    let last_index = state.lines.len().saturating_sub(1);
                    ((0..state.lines.len()).find(differs).unwrap_or(0), (0..state.lines.len()).rev().find(differs).unwrap_or(last_index))
                } else {
                    (0, state.lines.len().saturating_sub(1))
                };
                let extension = state.file_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                crate::viewer::highlight_through(&state.lines, &extension, first, last, &mut state.highlighted_lines, &mut state.line_states);
            }
            state.undo_stack = history;
            if changed {
                state.undo_stack.push(EditStep {
                    first_line: 0,
                    total_lines_before: before.len(),
                    before,
                    cursor,
                });
                if state.undo_stack.len() > crate::constants::UNDO_LIMIT {
                    state.undo_stack.remove(0);
                }
            }
        }
    }

    /// Esc on the match asked about, or no more to ask about: say how many
    /// were replaced.
    pub fn finish_replace(&mut self, replaced: usize) {
        if let Some(Dialog::Replacing { .. }) = self.dialog {
            self.dialog = None;
        }
        self.find_note = Some(match replaced {
            0 => "Nothing replaced".to_string(),
            1 => "Replaced 1".to_string(),
            many => format!("Replaced {many}"),
        });
    }

    /// Put `text` in place of the selection, as one step to undo. Typed or
    /// pasted text does this already; nothing at all, which deletes the
    /// selection, is the case they leave out.
    fn replace_selection(&mut self, text: &str) {
        if !text.is_empty() {
            self.editor_insert_text(text);
            return;
        }
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    /// Move to the next match of find_term, or the previous one, selecting it.
    /// `fresh` is a new search from the prompt, which may find the match the
    /// cursor is already on; F3 and Shift+F3 go on past it.
    pub fn find(&mut self, forward: bool, fresh: bool) {
        let Some(needle) = crate::find::Needle::new(&self.find_term) else {
            // F3 with nothing looked for yet asks what to look for.
            self.open_prompt(PromptKind::Find);
            return;
        };
        if !self.can_find() {
            return;
        }
        self.find_shown = true;

        let found = if self.is_editing() { self.editor_find(&needle, forward, fresh) } else { self.viewer_find(&needle, forward, fresh) };
        self.find_note = match found {
            None => Some(format!("Not found: {}", self.find_term)),
            Some(true) => Some(if forward { "Wrapped to the top" } else { "Wrapped to the bottom" }.to_string()),
            Some(false) => None,
        };
    }

    /// Some(wrapped) when a match was found and selected.
    fn editor_find(&mut self, needle: &crate::find::Needle, forward: bool, fresh: bool) -> Option<bool> {
        let height = self.editor_viewport_height.max(1);
        let state = self.editor_mut()?;
        let cursor = (state.cursor_line, state.cursor_col);
        let start = state.selection().map_or(cursor, |(start, _)| start);
        // A match is selected from its start to the cursor at its end, so on
        // from the cursor is past it and back from its start is before it.
        let from = if forward && !fresh { cursor } else { start };

        let lines = &state.lines;
        let found = crate::find::search(needle, lines.len(), |index| lines[index].clone(), from, forward)?;
        state.selection_anchor = Some((found.line, found.start));
        state.cursor_line = found.line;
        state.cursor_col = found.end;
        state.auto_scroll = true;
        if found.line < state.scroll_offset || found.line >= state.scroll_offset + height {
            state.scroll_offset = found.line.saturating_sub(height / 3);
        }
        Some(found.wrapped)
    }

    fn viewer_find(&mut self, needle: &crate::find::Needle, forward: bool, fresh: bool) -> Option<bool> {
        let (height, width) = (self.viewer_viewport_height.max(1), self.viewer_viewport_width.max(1));
        let state = self.viewer_mut()?;
        // From the match on screen when there is one, and otherwise from the
        // top of what is showing.
        let from = match state.selected_range() {
            Some((_, end)) if forward && !fresh => end,
            Some((start, _)) => start,
            None => {
                let (line, from, _) = state.row(state.scroll_offset);
                (line, from)
            }
        };

        let found = crate::find::search(needle, state.line_count(), |index| state.line_text(index), from, forward)?;
        state.selection = Some(((found.line, found.start), (found.line, found.end)));
        // Scrolled by rows, which are the lines themselves unless wrapping.
        let row = state.row_of(found.line, found.start);
        if row < state.scroll_offset || row >= state.scroll_offset + height {
            let max = state.total_lines.saturating_sub(height);
            state.scroll_offset = row.saturating_sub(height / 3).min(max);
        }
        // Sideways in columns, which is how it scrolls: a match after wide
        // characters is further along than its place in the line.
        if !state.wrapping() {
            let text = state.line_text(found.line);
            let (start, end) = (crate::utils::display_column(&text, found.start), crate::utils::display_column(&text, found.end));
            if start < state.horizontal_offset || end > state.horizontal_offset + width {
                state.horizontal_offset = start.saturating_sub(width / 4);
            }
        }
        Some(found.wrapped)
    }

    /// Ctrl+G: put the cursor on the start of a line, counted from 1. A number
    /// past the end goes to the last line. In the viewer the line goes to the
    /// top, as far as the end of the file allows.
    pub fn go_to_line(&mut self, number: usize) {
        if self.is_editing() {
            let height = self.editor_viewport_height.max(1);
            if let Screen::Editor(state) = &mut self.screen {
                let line = number.clamp(1, state.lines.len().max(1)) - 1;
                state.selection_anchor = None;
                state.cursor_line = line;
                state.cursor_col = 0;
                state.auto_scroll = true;
                if line < state.scroll_offset || line >= state.scroll_offset + height {
                    state.scroll_offset = line.saturating_sub(height / 3);
                }
            }
        } else if let Screen::Viewer(state) = &mut self.screen {
            let line = number.clamp(1, state.line_count().max(1)) - 1;
            state.scroll_offset = state.row_of(line, 0).min(state.total_lines.saturating_sub(self.viewer_viewport_height));
        }
    }
}
