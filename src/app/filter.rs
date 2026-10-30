//! Ctrl+S: narrowing a panel to the entries a pattern matches, as it is typed.

use super::*;

/// A panel narrowed to what a pattern matches. It belongs to the directory it
/// was typed in, and goes once the panel is somewhere else.
pub struct PanelFilter {
    pub dir: PathBuf,
    pub text: String,
    glob: crate::glob::Glob,
    /// Every entry of the directory, for the pattern to be changed or dropped
    /// without reading it again.
    all: Vec<Item>,
}

impl PanelFilter {
    /// What of `items` the pattern keeps: the parent entry always, as the way
    /// out.
    fn keep(&self, items: &[Item]) -> Vec<Item> {
        items.iter().filter(|item| item.is_parent() || self.glob.matches(&item.name_full, item.is_dir)).cloned().collect()
    }
}

impl AppState {
    pub fn filter(&self, is_left: bool) -> Option<&PanelFilter> {
        if is_left { self.filter_left.as_ref() } else { self.filter_right.as_ref() }
    }

    fn filter_mut(&mut self, is_left: bool) -> &mut Option<PanelFilter> {
        if is_left { &mut self.filter_left } else { &mut self.filter_right }
    }

    /// Ctrl+S in a panel: ask for the pattern, starting from the one in
    /// place. Esc puts that one back.
    pub fn open_filter_prompt(&mut self) {
        let text = self.filter(self.is_left_active).map(|filter| filter.text.clone()).unwrap_or_default();
        let mut input = TextInput::new();
        input.set(text.clone());
        self.filter_before = text;
        self.dialog = Some(Dialog::Prompt(PromptKind::Filter, input));
    }

    /// Esc on the prompt: the pattern there was before it opened.
    pub fn cancel_filter_prompt(&mut self) {
        self.dialog = None;
        let before = std::mem::take(&mut self.filter_before);
        self.set_filter(self.is_left_active, &before);
    }

    /// Narrow the panel to what `text` matches, or show it all again for a
    /// pattern with nothing in it. The cursor stays on its entry while that
    /// is shown, and goes to the first match otherwise. What is selected but
    /// no longer shown is let go: a copy or delete takes the selection, and
    /// should not take what cannot be seen.
    pub fn set_filter(&mut self, is_left: bool, text: &str) {
        let (dir, children, state) = if is_left {
            (&self.dir_left, &self.children_left, &self.state_left)
        } else {
            (&self.dir_right, &self.children_right, &self.state_right)
        };
        let cursor = state.selected().and_then(|index| children.get(index)).map(|item| item.name_os.clone());
        let dir = dir.clone();
        let all = match self.filter_mut(is_left).take() {
            Some(filter) => filter.all,
            None => std::mem::take(if is_left { &mut self.children_left } else { &mut self.children_right }),
        };
        let items = match crate::glob::Glob::filter(text) {
            Some(glob) => {
                let filter = PanelFilter { dir, text: text.to_string(), glob, all };
                let items = filter.keep(&filter.all);
                *self.filter_mut(is_left) = Some(filter);
                items
            }
            None => all,
        };
        let index = cursor.and_then(|name| items.iter().position(|item| item.name_os == name)).or_else(|| items.iter().position(|item| !item.is_parent())).unwrap_or(0);
        let (children, state, selected) = if is_left {
            (&mut self.children_left, &mut self.state_left, &mut self.selected_left)
        } else {
            (&mut self.children_right, &mut self.state_right, &mut self.selected_right)
        };
        *children = items;
        state.select(Some(index));
        prune_selection(selected, children);
        self.search_clear();
        self.preview = None;
        self.cursor_detail = None;
    }

    /// A fresh reading of the directory `found`, narrowed by the panel's
    /// filter if it is still there; the filter goes if the panel has moved.
    pub(super) fn filter_listing(&mut self, is_left: bool, found: &Path, items: Vec<Item>) -> Vec<Item> {
        let slot = self.filter_mut(is_left);
        match slot {
            Some(filter) if filter.dir == found => {
                let kept = filter.keep(&items);
                filter.all = items;
                kept
            }
            _ => {
                *slot = None;
                items
            }
        }
    }

    /// The filter's typing, shown as it is typed.
    pub fn filter_typed(&mut self) {
        if let Some(Dialog::Prompt(PromptKind::Filter, input)) = &self.dialog {
            let text = input.text.clone();
            self.set_filter(self.is_left_active, &text);
        }
    }
}
