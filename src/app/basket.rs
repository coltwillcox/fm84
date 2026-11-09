//! Alt+B: a basket of files gathered from anywhere, to copy, move or pack
//! all at once from wherever the panel is. Alt+Shift+B shows it in the
//! panel - fed, as Find files feeds it, under the directory all of it is
//! in - every entry chosen, ready for F5, F6 or Alt+P.

use super::*;

impl AppState {
    /// Whether the active panel is showing the basket.
    pub fn showing_basket(&self) -> bool {
        self.flat(self.active_side).is_some_and(|flat| flat.basket)
    }

    /// Alt+B on `items`, what is chosen: into the basket - or, with the
    /// basket on show, out of it.
    pub fn basket_toggle(&mut self, items: Vec<(PathBuf, bool)>) {
        if items.is_empty() {
            return;
        }
        if self.showing_basket() {
            self.basket.retain(|(path, _)| !items.iter().any(|(taken, _)| taken == path));
            self.find_note = Some(t!("Basket: {count}", count = self.basket.len()));
            return self.show_basket();
        }
        if self.refuse_in_archive(self.active_side) {
            return;
        }
        for item in items {
            if !self.basket.iter().any(|(path, _)| *path == item.0) {
                self.basket.push(item);
            }
        }
        self.find_note = Some(t!("Basket: {count}", count = self.basket.len()));
    }

    /// Alt+Shift+B: the basket in the active panel, or the panel back to
    /// the directory it is in when it shows the basket already.
    pub fn open_basket(&mut self) {
        let side = self.active_side;
        if self.showing_basket() {
            self.panel_mut(side).flat = None;
            self.panel_mut(side).selected.clear();
            return self.reload_panel(side, None);
        }
        if self.basket.is_empty() {
            self.find_note = Some(t!("The basket is empty").to_string());
            return;
        }
        self.show_basket();
    }

    /// The basket fed to the active panel as it now is, what is no longer
    /// there let go of, and all of it selected. Empty, the panel goes back
    /// to the directory it was under.
    fn show_basket(&mut self) {
        let side = self.active_side;
        self.basket.retain(|(path, _)| path_exists(path));
        let Some(base) = common_dir(self.basket.iter().map(|(path, _)| path.as_path())) else {
            if self.basket.is_empty() {
                self.panel_mut(side).flat = None;
                self.panel_mut(side).selected.clear();
                self.reload_panel(side, None);
            } else {
                self.display_error(t!("The basket holds files on more than one drive").to_string());
            }
            return;
        };
        let feed: Vec<(PathBuf, bool)> = self.basket.iter().filter_map(|(path, is_dir)| path.strip_prefix(&base).ok().map(|relative| (relative.to_path_buf(), *is_dir))).collect();
        let chosen: HashSet<OsString> = feed.iter().map(|(relative, _)| relative.clone().into_os_string()).collect();
        let first = feed.first().map(|(relative, _)| relative.clone().into_os_string());
        let panel = self.panel_mut(side);
        panel.flat = Some(Flat {
            dir: base.clone(),
            feed: Some(Arc::new(feed)),
            basket: true,
        });
        panel.selected = chosen.clone();
        if self.panel_dir(side) == base {
            self.reload_panel(side, first.as_deref());
        } else {
            self.open_dir(side, base, first.as_deref());
            // Going there chooses afresh; what is chosen is all of it.
            match &mut self.panel_mut(side).listing {
                Some(pending) => pending.then_select = Some(chosen),
                None => {
                    let panel = self.panel_mut(side);
                    panel.selected = chosen;
                    prune_selection(&mut panel.selected, &panel.children);
                }
            }
        }
    }
}

/// The deepest directory every one of `paths` is under - never one of them
/// itself, which the feed could not then list. None for none, or for paths
/// on drives of their own, which no one directory holds.
fn common_dir<'a>(paths: impl Iterator<Item = &'a Path>) -> Option<PathBuf> {
    let mut common: Option<PathBuf> = None;
    for path in paths {
        let parent = path.parent()?;
        common = Some(match common {
            None => parent.to_path_buf(),
            Some(known) => known.components().zip(parent.components()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect(),
        });
    }
    common.filter(|dir| dir.has_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_basket_sits_under_the_deepest_directory_it_all_shares() {
        let common = |paths: &[&str]| common_dir(paths.iter().map(Path::new));
        assert_eq!(common(&["/a/b/x", "/a/b/c/y"]), Some(PathBuf::from("/a/b")));
        assert_eq!(common(&["/a/b/x", "/a/bc/y"]), Some(PathBuf::from("/a")));
        // A directory in it is listed from the one it is in.
        assert_eq!(common(&["/a/b"]), Some(PathBuf::from("/a")));
        assert_eq!(common(&["/x", "/y/z"]), Some(PathBuf::from("/")));
        assert_eq!(common(&[]), None);
    }
}
