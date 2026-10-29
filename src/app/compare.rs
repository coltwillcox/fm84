//! Shift+F2 compares the panels, and = or Shift+F3 two files side by side.

use super::*;

/// Modified times this close count as the same. FAT and exFAT keep them only
/// to two seconds, so a file copied there and back would otherwise always
/// read as newer on one side.
const SAME_TIME: Duration = Duration::from_secs(2);

/// What Shift+F2 selects on each side, by name: an entry only that side has;
/// a file and a directory under one name, on both; and of a file both have,
/// the one modified later, or both when the times agree and the sizes do not.
/// A file without a modified time is judged by its size alone.
pub(super) fn differences(left: &[Item], right: &[Item]) -> (HashSet<OsString>, HashSet<OsString>) {
    fn entries(items: &[Item]) -> HashMap<OsString, &Item> {
        items.iter().filter(|item| !item.is_parent()).map(|item| (item.name_os.clone(), item)).collect()
    }
    let (left, right) = (entries(left), entries(right));
    let (mut on_left, mut on_right) = (HashSet::new(), HashSet::new());
    for (name, &item) in &left {
        let Some(&other) = right.get(name) else {
            on_left.insert(name.clone());
            continue;
        };
        let (newer, older) = match (item.is_dir, other.is_dir) {
            (true, true) => continue,
            (false, false) => match (item.modified_at, other.modified_at) {
                (Some(mine), Some(theirs)) if mine > theirs + SAME_TIME => (true, false),
                (Some(mine), Some(theirs)) if theirs > mine + SAME_TIME => (false, true),
                _ => {
                    let differ = item.size_bytes != other.size_bytes;
                    (differ, differ)
                }
            },
            _ => (true, true),
        };
        if newer {
            on_left.insert(name.clone());
        }
        if older {
            on_right.insert(name.clone());
        }
    }
    on_right.extend(right.into_keys().filter(|name| !left.contains_key(name)));
    (on_left, on_right)
}

impl AppState {
    /// Shift+F3 on two files: side by side when they are text that differs,
    /// and otherwise a word in the status bar about how they compare.
    pub fn compare_files(&mut self, left: PathBuf, right: PathBuf) {
        use crate::diff::Compared;
        match crate::diff::compare(&left, &right, self.options.large_file_bytes()) {
            Ok(Compared::Text(state)) => self.screen = Screen::Diff(*state),
            Ok(Compared::Identical) => self.find_note = Some("The files are identical".to_string()),
            Ok(Compared::LineEndings) => self.find_note = Some("The files differ only in line endings".to_string()),
            Ok(Compared::Binary { at, sizes_differ }) => {
                let sizes = if sizes_differ { ", and in size" } else { "" };
                self.find_note = Some(format!("The files differ from byte {}{}", crate::utils::grouped(at), sizes));
            }
            Err(message) => self.display_error(message),
        }
    }

    pub fn diff_mut(&mut self) -> Option<&mut crate::diff::DiffState> {
        match &mut self.screen {
            Screen::Diff(state) => Some(state),
            _ => None,
        }
    }

    pub fn close_diff(&mut self) {
        self.screen = Screen::Panels;
    }

    /// Scroll the two columns by `rows`, down or up, or sideways by
    /// `columns`, within what there is to see.
    pub fn diff_scroll(&mut self, rows: isize, columns: isize) {
        let height = self.diff_viewport_height;
        let width = self.diff_viewport_width;
        if let Some(state) = self.diff_mut() {
            let bottom = state.rows.len().saturating_sub(height);
            state.scroll_offset = state.scroll_offset.saturating_add_signed(rows).min(bottom);
            let right_edge = state.max_line_width.saturating_sub(width);
            state.horizontal_offset = state.horizontal_offset.saturating_add_signed(columns).min(right_edge);
        }
    }

    /// Shift+F2, as in Total Commander: select in each panel what differs from
    /// the other - what only it has, and of a file both have, the newer, or
    /// both when only the size tells them apart. Whatever was selected before
    /// is replaced, ready for F5 to copy across. Directories both have are
    /// not gone into. What the status bar says is how many differ.
    pub fn compare_dirs(&mut self) {
        if self.panel_busy(true) || self.panel_busy(false) {
            return;
        }
        if self.dir_left == self.dir_right {
            self.find_note = Some("Both panels show the same directory".to_string());
            return;
        }
        let (left, right) = differences(&self.children_left, &self.children_right);
        self.find_note = Some(if left.is_empty() && right.is_empty() {
            "The directories hold the same files".to_string()
        } else {
            format!("Differ: {} on the left, {} on the right", left.len(), right.len())
        });
        self.selected_left = left;
        self.selected_right = right;
    }
}
