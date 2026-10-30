//! Alt+Down: the directories the panels have been in, across every tab, to
//! go back to any of them at once rather than a step at a time.

use super::*;

/// The popup: every directory visited, most recent first, narrowed to those
/// whose path holds each word typed.
pub struct HistoryList {
    pub input: TextInput,
    all: Vec<PathBuf>,
    /// What `input` lets through, in the same order.
    pub shown: Vec<PathBuf>,
    pub index: usize,
}

impl HistoryList {
    /// Narrow to what has every word typed somewhere in its path, smart case
    /// as Find is: all in lower case matches either case.
    fn narrow(&mut self) {
        let fold = !self.input.text.chars().any(char::is_uppercase);
        let folded = |text: &str| -> String { if fold { text.chars().map(crate::find::fold_char).collect() } else { text.to_string() } };
        let words: Vec<String> = self.input.text.split_whitespace().map(folded).collect();
        self.shown = self.all.iter().filter(|path| words.iter().all(|word| folded(&path.to_string_lossy()).contains(word.as_str()))).cloned().collect();
        self.index = 0;
    }
}

impl AppState {
    /// Note that a panel has gone to `found` from `left`. Newest first, each
    /// directory once. The one left is kept too, after it, if it is not
    /// there yet - where the panels started, which nothing went to.
    pub(super) fn record_visit(&mut self, left: &Path, found: &Path) {
        self.visited.retain(|path| path != found);
        self.visited.insert(0, found.to_path_buf());
        if !self.visited.iter().any(|path| path == left) {
            self.visited.insert(1, left.to_path_buf());
        }
        self.visited.truncate(crate::constants::VISITED_MAX);
    }

    /// Alt+Down: the directories visited, opened on the newest that is not
    /// where the active panel already is.
    pub fn open_history(&mut self) {
        let here = self.panel_dir(self.is_left_active);
        let mut all = self.visited.clone();
        if all.is_empty() {
            all.push(here.clone());
        }
        let index = all.iter().position(|path| *path != here).unwrap_or(0);
        self.dialog = Some(Dialog::History(Box::new(HistoryList {
            input: TextInput::new(),
            shown: all.clone(),
            all,
            index,
        })));
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_history_cursor(&mut self, step: isize) {
        if let Some(Dialog::History(list)) = &mut self.dialog {
            list.index = list.index.saturating_add_signed(step).min(list.shown.len().saturating_sub(1));
        }
    }

    /// The words typed, which the list is narrowed to.
    pub fn history_typed(&mut self) {
        if let Some(Dialog::History(list)) = &mut self.dialog {
            list.narrow();
        }
    }

    /// Enter: take the active panel to the one highlighted. One gone since is
    /// said to be, as Ctrl+L says, and the panel stays put.
    pub fn go_to_history(&mut self) {
        let Some(Dialog::History(list)) = self.dialog.take() else {
            return;
        };
        if let Some(path) = list.shown.into_iter().nth(list.index) {
            let is_left = self.is_left_active;
            self.request_listing(is_left, path, true, None, None, Travel::GoTo);
        }
    }
}
