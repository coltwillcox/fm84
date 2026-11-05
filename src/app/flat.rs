//! Ctrl+B: every file under a panel's directory in one list, as Total
//! Commander's branch view - sorted by size, it finds what takes the space.

use super::*;

/// A panel listing more than one directory's entries, for the directory it
/// belongs to: under Ctrl+B every file below it, or what Find files found
/// there, fed to the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Flat {
    pub dir: PathBuf,
    /// What Find files fed it, by their paths from `dir`, and whether each
    /// is a directory. None for Ctrl+B's walk.
    pub feed: Option<Arc<Vec<(PathBuf, bool)>>>,
}

impl AppState {
    pub fn flat(&self, is_left: bool) -> Option<&Flat> {
        if is_left { self.flat_left.as_ref() } else { self.flat_right.as_ref() }
    }

    pub(super) fn flat_mut(&mut self, is_left: bool) -> &mut Option<Flat> {
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
                *self.flat_mut(is_left) = Some(Flat { dir: self.panel_dir(is_left), feed: None });
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
