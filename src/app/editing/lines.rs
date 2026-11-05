//! The editor's work on whole lines: the indent carried on to a new line,
//! the selected lines indented or brought back out, and commented or not.

use super::*;

/// What a line comment starts with in a file of this extension, or of this
/// name when it has none. `#` for what is not known: most configuration and
/// scripts.
fn comment_for(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "cs" | "java" | "kt" | "kts" | "scala" | "go" | "swift" | "dart" | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "php" | "zig" | "v" | "groovy" | "gradle" | "proto" | "jsonc" | "glsl"
        | "hlsl" | "wgsl" | "sol" => "//",
        "lua" | "sql" | "hs" | "elm" | "ada" | "adb" | "ads" | "vhd" | "vhdl" => "--",
        "lisp" | "lsp" | "el" | "clj" | "cljs" | "cljc" | "edn" | "scm" | "ss" | "rkt" | "ini" | "asm" | "s" => ";",
        "tex" | "sty" | "cls" | "erl" | "hrl" | "m" | "mat" => "%",
        "vim" => "\"",
        "bat" | "cmd" => "REM",
        "f" | "f90" | "f95" | "f03" => "!",
        _ => "#",
    }
}

/// The spaces and tabs a line starts with.
fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

impl AppState {
    /// Enter: the line broken at the cursor, the new one starting with the
    /// indent of the one it came from - as far as the cursor, so Enter in
    /// the middle of an indent does not make it deeper. What follows the
    /// cursor keeps its own spaces after it.
    pub fn editor_enter(&mut self) {
        let line = self.editor().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.delete_selection();
        let mut from = None;
        if let Screen::Editor(state) = &mut self.screen {
            from = Some(state.cursor_line);
            let line = &mut state.lines[state.cursor_line];
            let byte_idx = char_to_byte(line, state.cursor_col);
            let indent = indent_of(&line[..byte_idx]).to_string();
            let new_line = format!("{indent}{}", &line[byte_idx..]);
            line.truncate(byte_idx);
            // What is left behind, when it is only the indent, keeps none of
            // it: no line of nothing but spaces.
            if !indent.is_empty() && line.len() == indent.len() {
                line.clear();
            }
            state.cursor_line += 1;
            state.lines.insert(state.cursor_line, new_line);
            state.cursor_col = indent.chars().count();
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

    /// The lines a line command works on: those the selection touches - not
    /// the last, when the selection only reaches its start - or the cursor's.
    fn selected_lines(&self) -> Option<(usize, usize)> {
        let state = self.editor()?;
        Some(match state.selection() {
            Some(((first, _), (last, 0))) if last > first => (first, last - 1),
            Some(((first, _), (last, _))) => (first, last),
            None => (state.cursor_line, state.cursor_line),
        })
    }

    /// Whether a selection runs over more than one line, for Tab to indent
    /// it rather than type over it.
    pub fn editor_spans_lines(&self) -> bool {
        self.editor().and_then(EditorState::selection).is_some_and(|((first, _), (last, _))| first != last)
    }

    /// One indent: a tab in a file indented with tabs, and otherwise as many
    /// spaces as F11's tab width.
    fn indent_unit(&self) -> String {
        let tabs = self.editor().is_some_and(|state| {
            let mut indents = state.lines.iter().map(|line| indent_of(line)).filter(|indent| !indent.is_empty());
            indents.next().is_some_and(|indent| indent.starts_with('\t'))
        });
        if tabs { "\t".to_string() } else { " ".repeat(self.options.tab_width.max(1)) }
    }

    /// Tab on a selection of lines, and Shift+Tab anywhere: each line
    /// indented one step further, or brought one step back out - a tab, or
    /// up to a tab width of spaces. Lines with nothing on them are left be.
    /// The lines stay selected, whole, for another press; one step to undo.
    pub fn editor_indent(&mut self, deeper: bool) {
        let Some((first, last)) = self.selected_lines() else {
            return;
        };
        let unit = self.indent_unit();
        let width = self.options.tab_width.max(1);
        let had_selection = self.editor().and_then(EditorState::selection).is_some();
        self.push_undo(first..=last);
        if let Screen::Editor(state) = &mut self.screen {
            let mut cursor_shift: isize = 0;
            for index in first..=last {
                let line = &mut state.lines[index];
                if line.trim().is_empty() {
                    continue;
                }
                let change = if deeper {
                    line.insert_str(0, &unit);
                    unit.chars().count() as isize
                } else {
                    let removed = if line.starts_with('\t') { 1 } else { line.chars().take(width).take_while(|&c| c == ' ').count() };
                    line.replace_range(..removed, "");
                    -(removed as isize)
                };
                if index == state.cursor_line {
                    cursor_shift = change;
                }
            }
            if had_selection {
                state.selection_anchor = Some((first, 0));
                state.cursor_line = last;
                state.cursor_col = state.lines[last].chars().count();
            } else {
                state.cursor_col = state.cursor_col.saturating_add_signed(cursor_shift).min(state.lines[state.cursor_line].chars().count());
            }
            state.modified = true;
        }
        self.editor_rehighlight_through(first, last);
        self.finish_edit();
    }

    /// Ctrl+/: the selected lines, or the cursor's, made comments - or, when
    /// every one with anything on it already is, made code again. The mark
    /// goes in at the shallowest indent among them, so a block stays lined
    /// up, with a space after it; taking it out takes that space too.
    pub fn editor_toggle_comment(&mut self) {
        let Some((first, last)) = self.selected_lines() else {
            return;
        };
        let Some(mark) = self.editor().map(|state| comment_for(&state.file_path)) else {
            return;
        };
        let had_selection = self.editor().and_then(EditorState::selection).is_some();
        self.push_undo(first..=last);
        if let Screen::Editor(state) = &mut self.screen {
            let filled: Vec<usize> = (first..=last).filter(|&index| !state.lines[index].trim().is_empty()).collect();
            let commented = !filled.is_empty() && filled.iter().all(|&index| state.lines[index].trim_start().starts_with(mark));
            let column = filled.iter().map(|&index| indent_of(&state.lines[index]).len()).min().unwrap_or(0);
            let mut cursor_shift: isize = 0;
            for &index in &filled {
                let line = &mut state.lines[index];
                let change = if commented {
                    let at = indent_of(line).len();
                    let end = at + mark.len();
                    let end = if line[end..].starts_with(' ') { end + 1 } else { end };
                    line.replace_range(at..end, "");
                    -((end - at) as isize)
                } else {
                    // At the shallowest indent, which is spaces and tabs - a
                    // byte offset that is a character one too.
                    line.insert_str(column, &format!("{mark} "));
                    (mark.len() + 1) as isize
                };
                if index == state.cursor_line {
                    cursor_shift = change;
                }
            }
            if had_selection {
                state.selection_anchor = Some((first, 0));
                state.cursor_line = last;
                state.cursor_col = state.lines[last].chars().count();
            } else {
                state.cursor_col = state.cursor_col.saturating_add_signed(cursor_shift).min(state.lines[state.cursor_line].chars().count());
            }
            state.modified = true;
        }
        self.editor_rehighlight_through(first, last);
        self.finish_edit();
    }
}
