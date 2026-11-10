//! Alt+B: mixtapes - baskets with names, kept across runs - of files
//! gathered from anywhere, to copy, move or pack all at once from wherever
//! the panel is. Alt+B adds to the tape playing; Alt+Shift+B lists the
//! tapes, and shows one in the panel - fed, as Find files feeds it, under
//! the directory all of it is in - every entry chosen, ready for F5, F6 or
//! Alt+P.

use super::*;

/// Alt+Shift+B's list: the tape highlighted, and a name being typed.
pub struct MixtapeList {
    pub index: usize,
    pub naming: Option<Naming>,
}

/// A name being typed for a tape: a new one, or the one highlighted.
pub struct Naming {
    pub input: TextInput,
    pub renaming: bool,
}

impl AppState {
    /// Whether the active panel is showing the tape playing.
    pub fn showing_basket(&self) -> bool {
        self.flat(self.active_side).is_some_and(|flat| flat.kind == FeedKind::Basket)
    }

    /// Alt+B on `items`, what is chosen: onto the tape playing - a new one,
    /// with none yet - or, with the tape on show, off it.
    pub fn basket_toggle(&mut self, items: Vec<(PathBuf, bool)>) {
        if items.is_empty() {
            return;
        }
        if self.showing_basket() {
            if let Some(tape) = self.tapes.playing_mut() {
                tape.items.retain(|(path, _)| !items.iter().any(|(taken, _)| taken == path));
            }
            self.save_tapes();
            self.note_tape();
            return self.show_basket();
        }
        if self.refuse_in_archive(self.active_side) {
            return;
        }
        if self.tapes.playing().is_none() {
            let name = self.tapes.fresh_name(t!("Mixtape"));
            self.tapes.tapes.push(crate::options::Tape { name, items: Vec::new() });
            self.tapes.playing = self.tapes.tapes.len() - 1;
        }
        if let Some(tape) = self.tapes.playing_mut() {
            for item in items {
                if !tape.items.iter().any(|(path, _)| *path == item.0) {
                    tape.items.push(item);
                }
            }
        }
        self.save_tapes();
        self.note_tape();
    }

    /// What the tape playing is called and how much is on it, said at the
    /// foot of the screen.
    fn note_tape(&mut self) {
        if let Some(tape) = self.tapes.playing() {
            self.find_note = Some(t!("{tape}: {count}", tape = tape.name, count = tape.items.len()));
        }
    }

    /// Alt+Shift+B: the list of tapes, or the panel back to the directory it
    /// is in when it shows a tape already.
    pub fn open_basket(&mut self) {
        let side = self.active_side;
        if self.showing_basket() {
            self.panel_mut(side).flat = None;
            self.panel_mut(side).selected.clear();
            return self.reload_panel(side, None);
        }
        let index = self.tapes.playing.min(self.tapes.tapes.len().saturating_sub(1));
        self.dialog = Some(Dialog::Mixtapes(Box::new(MixtapeList { index, naming: None })));
    }

    fn mixtape_list(&mut self) -> Option<&mut MixtapeList> {
        match &mut self.dialog {
            Some(Dialog::Mixtapes(list)) => Some(list),
            _ => None,
        }
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_mixtape_cursor(&mut self, step: isize) {
        let last = self.tapes.tapes.len().saturating_sub(1);
        if let Some(list) = self.mixtape_list() {
            list.index = list.index.saturating_add_signed(step).min(last);
        }
    }

    /// Space: the tape highlighted is the one playing, for Alt+B to add to.
    pub fn play_mixtape(&mut self) {
        let Some(index) = self.mixtape_list().map(|list| list.index) else { return };
        if index < self.tapes.tapes.len() {
            self.tapes.playing = index;
            self.save_tapes();
        }
    }

    /// Enter: the tape highlighted plays, and shows in the active panel.
    pub fn show_mixtape(&mut self) {
        let Some(Dialog::Mixtapes(list)) = self.dialog.take() else { return };
        let Some(tape) = self.tapes.tapes.get(list.index) else { return };
        if tape.items.is_empty() {
            self.find_note = Some(t!("{tape} is empty", tape = tape.name));
        }
        self.tapes.playing = list.index;
        self.save_tapes();
        if !self.tapes.tapes[list.index].items.is_empty() {
            self.show_basket();
        }
    }

    /// N or Insert, a new tape to name; R or F2, the one highlighted renamed.
    pub fn name_mixtape(&mut self, renaming: bool) {
        let current = self.mixtape_list().map(|list| list.index).and_then(|index| self.tapes.tapes.get(index)).map(|tape| tape.name.clone());
        if renaming && current.is_none() {
            return;
        }
        let mut input = TextInput::new();
        if renaming {
            input.set(current.unwrap_or_default());
        }
        if let Some(list) = self.mixtape_list() {
            list.naming = Some(Naming { input, renaming });
        }
    }

    /// Enter on the name: the new tape, after the one highlighted and
    /// playing - or the one highlighted called by it. A name another tape
    /// has already is refused, as is none at all.
    pub fn commit_mixtape_name(&mut self) {
        let Some((index, naming)) = self.mixtape_list().and_then(|list| list.naming.take().map(|naming| (list.index, naming))) else {
            return;
        };
        let name = naming.input.text.trim().to_string();
        if name.is_empty() {
            return;
        }
        if self.tapes.is_taken(&name, naming.renaming.then_some(index)) {
            if let Some(list) = self.mixtape_list() {
                list.naming = Some(naming);
            }
            return self.display_error(t!("There is a mixtape called {name} already", name));
        }
        if naming.renaming {
            self.tapes.tapes[index].name = name;
        } else {
            let at = if self.tapes.tapes.is_empty() { 0 } else { index + 1 };
            self.tapes.tapes.insert(at, crate::options::Tape { name, items: Vec::new() });
            self.tapes.playing = at;
            if let Some(list) = self.mixtape_list() {
                list.index = at;
            }
        }
        self.save_tapes();
    }

    /// Esc on the name: back to the list as it was.
    pub fn cancel_mixtape_name(&mut self) {
        if let Some(list) = self.mixtape_list() {
            list.naming = None;
        }
    }

    /// D or Delete: the tape highlighted forgotten - the tape, never the
    /// files on it. The one playing stays playing, wherever it moves to.
    pub fn remove_mixtape(&mut self) {
        let Some(index) = self.mixtape_list().map(|list| list.index) else { return };
        if index >= self.tapes.tapes.len() {
            return;
        }
        self.tapes.tapes.remove(index);
        if self.tapes.playing > index {
            self.tapes.playing -= 1;
        }
        self.tapes.playing = self.tapes.playing.min(self.tapes.tapes.len().saturating_sub(1));
        let last = self.tapes.tapes.len().saturating_sub(1);
        if let Some(list) = self.mixtape_list() {
            list.index = list.index.min(last);
        }
        self.save_tapes();
    }

    /// The tapes kept for next time. A failed write is said, as the
    /// bookmarks' is: what was gathered would otherwise be lost unseen.
    pub fn save_tapes(&mut self) {
        if let Err(e) = self.tapes.save() {
            self.display_error(t!("Cannot save the mixtapes: {e}", e));
        }
    }

    /// The tape playing fed to the active panel as it now is, what is no
    /// longer there let go of, and all of it selected. Empty, the panel goes
    /// back to the directory it was under.
    fn show_basket(&mut self) {
        let side = self.active_side;
        let Some(tape) = self.tapes.playing_mut() else { return };
        let before = tape.items.len();
        tape.items.retain(|(path, _)| path_exists(path));
        let (gone, items, name) = (tape.items.len() != before, tape.items.clone(), tape.name.clone());
        if gone {
            self.save_tapes();
        }
        let Some(base) = common_dir(items.iter().map(|(path, _)| path.as_path())) else {
            if items.is_empty() {
                self.panel_mut(side).flat = None;
                self.panel_mut(side).selected.clear();
                self.reload_panel(side, None);
            } else {
                self.display_error(t!("{tape} holds files on more than one drive", tape = name));
            }
            return;
        };
        let feed: Vec<(PathBuf, bool)> = items.iter().filter_map(|(path, is_dir)| path.strip_prefix(&base).ok().map(|relative| (relative.to_path_buf(), *is_dir))).collect();
        let chosen: HashSet<OsString> = feed.iter().map(|(relative, _)| relative.clone().into_os_string()).collect();
        let first = feed.first().map(|(relative, _)| relative.clone().into_os_string());
        let panel = self.panel_mut(side);
        panel.flat = Some(Flat {
            dir: base.clone(),
            feed: Some(Arc::new(feed)),
            kind: FeedKind::Basket,
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
