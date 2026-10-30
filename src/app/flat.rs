//! Ctrl+B: every file under a panel's directory in one list, as Total
//! Commander's branch view - sorted by size, it finds what takes the space.

use super::*;

impl AppState {
    pub fn flat(&self, is_left: bool) -> Option<&PathBuf> {
        if is_left { self.flat_left.as_ref() } else { self.flat_right.as_ref() }
    }

    pub(super) fn flat_mut(&mut self, is_left: bool) -> &mut Option<PathBuf> {
        if is_left { &mut self.flat_left } else { &mut self.flat_right }
    }

    /// Ctrl+B: list every file under the active panel's directory, or go
    /// back to listing it as a directory. It belongs to that directory, and
    /// the panel lists as usual once it goes anywhere else. Back from it,
    /// the cursor lands on what holds the file it was on.
    pub fn toggle_flat(&mut self) {
        let is_left = self.is_left_active;
        let (children, state) = if is_left { (&self.children_left, &self.state_left) } else { (&self.children_right, &self.state_right) };
        let cursor = state.selected().and_then(|index| children.get(index)).map(|item| item.name_os.clone());
        let before = self.flat(is_left).cloned();
        let landing = match self.flat_mut(is_left).take() {
            Some(_) => cursor.and_then(|name| Path::new(&name).components().next().map(|first| first.as_os_str().to_os_string())),
            None => {
                *self.flat_mut(is_left) = Some(self.panel_dir(is_left));
                cursor
            }
        };
        self.reload_panel(is_left, landing.as_deref());
        // Still being read - a big tree - and Esc should leave it as it was.
        if let Some(pending) = self.listing_mut(is_left) {
            pending.restore_flat = Some(before);
        }
    }
}
