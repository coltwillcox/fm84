//! Ctrl+D: the directories kept to go back to.

use super::*;

impl AppState {
    /// Ctrl+D: the bookmarks, read afresh so a hand edit shows, opened on
    /// the active panel's directory when it is one of them.
    pub fn open_bookmarks(&mut self) {
        let list = crate::options::load_bookmarks();
        let here = self.panel_dir(self.is_left_active);
        let index = list.iter().position(|path| *path == here).unwrap_or(0);
        self.dialog = Some(Dialog::Bookmarks { list, index });
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_bookmark_cursor(&mut self, step: isize) {
        if let Some(Dialog::Bookmarks { list, index }) = &mut self.dialog {
            *index = index.saturating_add_signed(step).min(list.len().saturating_sub(1));
        }
    }

    /// Insert or A: keep the active panel's directory, after the one
    /// highlighted. One already kept is only highlighted.
    pub fn add_bookmark(&mut self) {
        let here = self.panel_dir(self.is_left_active);
        let Some(Dialog::Bookmarks { list, index }) = &mut self.dialog else {
            return;
        };
        match list.iter().position(|path| *path == here) {
            Some(found) => *index = found,
            None => {
                let at = if list.is_empty() { 0 } else { *index + 1 };
                list.insert(at, here);
                *index = at;
                self.save_bookmarks();
            }
        }
    }

    /// Delete or D: forget the one highlighted.
    pub fn remove_bookmark(&mut self) {
        let Some(Dialog::Bookmarks { list, index }) = &mut self.dialog else {
            return;
        };
        if *index < list.len() {
            list.remove(*index);
            *index = (*index).min(list.len().saturating_sub(1));
            self.save_bookmarks();
        }
    }

    /// Shift+Up and Shift+Down: move the one highlighted up or down the list.
    pub fn shift_bookmark(&mut self, up: bool) {
        let Some(Dialog::Bookmarks { list, index }) = &mut self.dialog else {
            return;
        };
        let other = if up { index.checked_sub(1) } else { Some(*index + 1).filter(|&next| next < list.len()) };
        if let Some(other) = other {
            list.swap(*index, other);
            *index = other;
            self.save_bookmarks();
        }
    }

    /// Enter: take the active panel to the one highlighted. Gone since it was
    /// kept, it is said to be, as Ctrl+L says, and the panel stays put.
    pub fn go_to_bookmark(&mut self) {
        let Some(Dialog::Bookmarks { list, index }) = self.dialog.take() else {
            return;
        };
        if let Some(path) = list.into_iter().nth(index) {
            let is_left = self.is_left_active;
            self.request_listing(is_left, path, true, None, None, Travel::GoTo);
        }
    }

    fn save_bookmarks(&mut self) {
        if let Some(Dialog::Bookmarks { list, .. }) = &self.dialog
            && let Err(e) = crate::options::save_bookmarks(list)
        {
            self.display_error(format!("Cannot save bookmarks: {}", e));
        }
    }
}
