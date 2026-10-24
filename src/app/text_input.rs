//! A line of text being typed, and the cursor in it: the rename field, the
//! prompts, the dialogs' fields.

use super::*;

/// Reusable single-line text input with cursor.
pub struct TextInput {
    pub text: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self { text: String::new(), cursor: 0 }
    }

    pub fn set(&mut self, value: String) {
        self.cursor = value.chars().count();
        self.text = value;
    }

    fn byte_index(&self) -> usize {
        self.text.char_indices().nth(self.cursor).map(|(i, _)| i).unwrap_or(self.text.len())
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

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    /// Put the cursor on the character drawn `column` cells in, or after
    /// the last when the column is past it - for a click on a field drawn
    /// from its start, as one whose text fits is.
    pub fn cursor_to_column(&mut self, column: usize) {
        let mut used = 0;
        self.cursor = self
            .text
            .chars()
            .take_while(|&character| {
                used += crate::utils::display_width(&character.to_string()).max(1);
                used <= column
            })
            .count();
    }

    pub fn insert(&mut self, c: char) {
        let idx = self.byte_index();
        self.text.insert(idx, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let byte_start = self.text.char_indices().nth(self.cursor - 1).map(|(i, _)| i).unwrap_or(0);
            let byte_end = self.byte_index();
            self.text.replace_range(byte_start..byte_end, "");
            self.cursor -= 1;
        }
    }

    pub fn delete_forward(&mut self) {
        let len = self.text.chars().count();
        if self.cursor < len {
            let byte_start = self.byte_index();
            let byte_end = self.text.char_indices().nth(self.cursor + 1).map(|(i, _)| i).unwrap_or(self.text.len());
            self.text.replace_range(byte_start..byte_end, "");
        }
    }

    /// The spans of a field with the cursor in it: what is before it, the
    /// character it is on, and what is after. Past the end, the terminal's
    /// cursor sits after the last character, which is marked to say so - a
    /// cell of its own would show as a space the text does not have. Classic
    /// draws its cursor as a cell, and needs one there; so does a field with
    /// nothing in it, for the cursor to be found at all.
    fn spans_around(before: &str, under: Option<char>, after: &str, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        let printable = |text: &str| crate::utils::printable_name(text);
        match under {
            Some(character) => vec![Span::styled(printable(before), text_style), Span::styled(printable(&character.to_string()), cursor_style), Span::styled(printable(after), text_style)],
            None if crate::display::classic_cursor() => vec![Span::styled(printable(before), text_style), Span::styled(" ", cursor_style)],
            None => match before.char_indices().last() {
                Some((last, _)) => vec![Span::styled(printable(&before[..last]), text_style), Span::styled(printable(&before[last..]), text_style.add_modifier(CURSOR_AFTER))],
                None => vec![Span::styled(" ", cursor_style.add_modifier(PAST_END))],
            },
        }
    }

    /// Styled spans with a block cursor, windowed to `width` columns so the
    /// cursor stays in view. A name longer than the field it is typed into
    /// would otherwise run past the edge and be clipped there, taking the
    /// cursor with it: you would be typing somewhere you could not see.
    pub fn cursor_spans_within(&self, width: usize, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        if width == 0 {
            return Vec::new();
        }

        let cell = |character: char| crate::utils::display_width(&character.to_string()).max(1);
        let characters: Vec<char> = self.text.chars().collect();
        let cursor = self.cursor.min(characters.len());
        let under = characters.get(cursor).copied();
        let at_cursor = under.unwrap_or(' ');

        // A cursor past the end still needs a cell of its own to sit in.
        let whole: usize = characters.iter().map(|&c| cell(c)).sum::<usize>() + 1;

        // Everything fits: start at the beginning. Otherwise scroll so the
        // cursor is at the right-hand edge, which is where typing keeps it.
        let mut first = 0;
        if whole > width {
            let mut used = cell(at_cursor);
            first = cursor;
            while first > 0 && used + cell(characters[first - 1]) <= width {
                first -= 1;
                used += cell(characters[first]);
            }
        }

        let before: String = characters[first..cursor].iter().collect();
        let mut used = crate::utils::display_width(&before) + cell(at_cursor);
        let mut after = String::new();
        for &character in characters.iter().skip(cursor + 1) {
            let this = cell(character);
            if used + this > width {
                break;
            }
            used += this;
            after.push(character);
        }

        // A rename field is opened on the name that is there, and a name can
        // hold anything; the width is unchanged, a control character standing
        // in one column as the '.' that replaces it does.
        Self::spans_around(&before, under, &after, text_style, cursor_style)
    }

    /// Styled spans with a block cursor at the cursor position, for a field
    /// wide enough to hold whatever is typed into it.
    pub fn cursor_spans(&self, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        let byte_idx = self.byte_index();
        let before = self.text[..byte_idx].to_string();
        let rest = &self.text[byte_idx..];
        let mut chars = rest.chars();
        let under = chars.next();
        let after: String = chars.collect();

        Self::spans_around(&before, under, &after, text_style, cursor_style)
    }
}

/// Marks the cell a cursor sits in past the end of what is typed - in an
/// empty field, the only place it can go - for the UI to give it the colour
/// of whatever follows the field. A modifier nothing else draws with; the UI
/// takes it off again.
pub const PAST_END: Modifier = Modifier::HIDDEN;

/// Marks the last character typed, with the cursor past the end: the
/// terminal's cursor goes straight after it. Another modifier nothing else
/// draws with, taken off again by the UI.
pub const CURSOR_AFTER: Modifier = Modifier::CROSSED_OUT;
