//! F11: moving through the options, changing them, and what a change sets off.

use super::*;

impl AppState {
    fn options_row(&self) -> OptionRow {
        OPTION_ROWS[self.options_cursor.min(OPTION_ROWS.len() - 1)]
    }

    /// Move the highlight a row, stopping at the first and the last.
    pub fn options_move(&mut self, down: bool) {
        self.options_cursor = if down { (self.options_cursor + 1).min(OPTION_ROWS.len() - 1) } else { self.options_cursor.saturating_sub(1) };
    }

    /// Step the highlighted option, or start typing into it if it is text.
    pub fn options_change(&mut self, forward: bool) {
        let row = self.options_row();
        if row.is_text() {
            let mut input = TextInput::new();
            input.set(self.options.text(row).to_string());
            self.dialog = Some(Dialog::Options { editing: Some(input) });
            return;
        }
        self.options.cycle(row, forward);
        self.options_changed(row);
    }

    pub fn options_commit_edit(&mut self) {
        let Some(Dialog::Options { editing }) = &mut self.dialog else {
            return;
        };
        let Some(input) = editing.take() else {
            return;
        };
        let row = self.options_row();
        self.options.set_text(row, input.text.trim().to_string());
        self.options_changed(row);
    }

    pub fn options_cancel_edit(&mut self) {
        if let Some(Dialog::Options { editing }) = &mut self.dialog {
            *editing = None;
        }
    }

    /// Sort both panels by `key`, from Ctrl+F3 to Ctrl+F6 or a click on a
    /// column heading. The key they are already sorted by turns the order
    /// round; another starts ascending. Kept as F11 would keep it.
    pub fn sort_panels_by(&mut self, key: SortKey) {
        if self.options.sort_key == key {
            self.options.sort_descending = !self.options.sort_descending;
        } else {
            self.options.sort_key = key;
            self.options.sort_descending = false;
        }
        self.options_changed(OptionRow::SortKey);
    }

    /// Keep a change: write it out, and reread the panels if it changes what
    /// they list. A failed write still leaves the change in place for this run.
    fn options_changed(&mut self, row: OptionRow) {
        crate::display::apply(&self.options);
        // The detail lines are held until the cursor moves, and the date
        // format is one of the things they are written in.
        self.cursor_detail = None;
        if row.affects_listing() {
            self.reload_panel(true, None);
            self.reload_panel(false, None);
        }
        if let Err(e) = self.options.save() {
            self.display_error(format!("Cannot save options: {}", e));
        }
    }
}
