//! Alt+U: the space map as a screen of its own - the active panel's
//! entries as blocks as big as the room each takes, counted as
//! Alt+Shift+Enter counts them and growing as the counting goes. The
//! panel's cursor is the block chosen, so what was looked at is where the
//! panel is left.

use super::*;
use crate::treemap::{Direction, Tile};

impl AppState {
    /// Alt+U: the screen, every directory in the panel counted for it.
    pub fn open_treemap(&mut self) {
        self.size_all_dirs();
        self.screen = Screen::Treemap;
    }

    pub fn close_treemap(&mut self) {
        self.screen = Screen::Panels;
    }

    /// Each entry with something in it, by its place in the panel, and its
    /// bytes - a directory's as far as it has been counted.
    pub fn treemap_sizes(&self) -> Vec<(usize, u64)> {
        let side = self.active_side;
        let Some(map) = self.space_map(side) else {
            return Vec::new();
        };
        map.sizes.iter().copied().enumerate().filter(|&(_, size)| size > 0).collect()
    }

    /// Gone into another directory, its own are counted once it is listed.
    pub fn keep_treemap_sized(&mut self) {
        let side = self.active_side;
        if matches!(self.screen, Screen::Treemap) && !self.panel_busy(side) && self.panel(side).space_map.as_ref() != Some(&self.panel(side).dir) {
            self.size_all_dirs();
        }
    }

    /// Which of the blocks last drawn the cursor is on.
    fn treemap_at(&self) -> Option<usize> {
        let cursor = self.panel(self.active_side).state.selected()?;
        self.drawn.treemap.iter().position(|(tile, _)| *tile == Tile::Entry(cursor))
    }

    /// An arrow: the block that way, or the largest when the cursor is on
    /// none - `..`, or something too small to have one.
    pub fn treemap_move(&mut self, direction: Direction) {
        let to = match self.treemap_at() {
            Some(at) => crate::treemap::neighbour(&self.drawn.treemap, at, direction),
            None => self.drawn.treemap.iter().position(|(tile, _)| matches!(tile, Tile::Entry(_))),
        };
        self.treemap_choose(to);
    }

    /// The block at `index` of those drawn, as the panel's cursor. The rest
    /// of them together is no one entry to put it on.
    pub fn treemap_choose(&mut self, index: Option<usize>) {
        // The blocks are as last drawn, which a terminal too small to draw
        // the map in leaves as they were for another directory: one past
        // the entries this one has is not one of them.
        let count = self.panel(self.active_side).children.len();
        if let Some(&(Tile::Entry(entry), _)) = index.and_then(|index| self.drawn.treemap.get(index))
            && entry < count
        {
            self.panel_mut(self.active_side).state.select(Some(entry));
        }
    }

    /// Enter: into the directory chosen, mapped in its turn; on a file, back
    /// to the panel with the cursor on it.
    pub fn treemap_enter(&mut self) {
        let side = self.active_side;
        let panel = self.panel(side);
        let Some(item) = self.treemap_at().and(panel.state.selected()).and_then(|index| panel.children.get(index)) else {
            return;
        };
        if item.is_dir {
            let target = item.path_in(&panel.dir);
            self.open_dir(side, target, None);
        } else {
            self.close_treemap();
        }
    }

    /// Backspace: up a directory, the cursor on the one come out of.
    pub fn treemap_up(&mut self) {
        let side = self.active_side;
        let dir = self.panel(side).dir.clone();
        if let Some(parent) = dir.parent() {
            self.open_dir(side, parent.to_path_buf(), dir.file_name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blocks drawn for another directory - left as they were by a terminal
    /// too small to draw the map in - put no cursor past this one's entries.
    #[test]
    fn a_block_drawn_for_another_directory_is_not_chosen() {
        let dir = std::env::temp_dir().join(format!("fm84-treemap-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("only.txt"), "x").unwrap();
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.active_side = Side::Left;
        app_state.open_dir(Side::Left, dir.clone(), None);
        while app_state.panel_busy(Side::Left) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_listings();
        }
        let count = app_state.left.children.len();
        let before = app_state.left.state.selected();
        app_state.drawn.treemap = vec![(Tile::Entry(count + 3), Rect::new(0, 0, 10, 5))];
        app_state.treemap_choose(Some(0));
        assert_eq!(app_state.left.state.selected(), before);
        app_state.treemap_move(Direction::Right);
        assert!(app_state.left.state.selected().is_some_and(|index| index < count));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
