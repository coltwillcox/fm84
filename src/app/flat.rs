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
    /// The feed is the mixtape playing, shown from Alt+Shift+B, which Alt+B
    /// takes entries out of.
    pub basket: bool,
}

impl AppState {
    /// What Find files fed either panel, under the names `moves` gave it -
    /// and what was inside a directory renamed, under its new path. The
    /// list holds paths, and at the next reread one no longer there is
    /// taken to be gone.
    pub fn feed_follow(&mut self, moves: &[(PathBuf, PathBuf)]) {
        // The mixtapes follow their files, and what is inside a directory on
        // one - every tape, not only the one playing.
        let mut followed = false;
        for (path, _) in self.tapes.tapes.iter_mut().flat_map(|tape| &mut tape.items) {
            if let Some(moved) = moves.iter().find_map(|(from, to)| path.strip_prefix(from).ok().map(|below| if below.as_os_str().is_empty() { to.clone() } else { to.join(below) })) {
                *path = moved;
                followed = true;
            }
        }
        if followed {
            self.save_tapes();
        }
        for flat in [&mut self.left.flat, &mut self.right.flat].into_iter().flatten() {
            let Some(feed) = &flat.feed else { continue };
            let dir = &flat.dir;
            let followed: Vec<(PathBuf, bool)> = feed
                .iter()
                .map(|(relative, is_dir)| {
                    let at = dir.join(relative);
                    let moved = moves.iter().find_map(|(from, to)| at.strip_prefix(from).ok().map(|below| if below.as_os_str().is_empty() { to.clone() } else { to.join(below) }));
                    let relative = moved.and_then(|moved| moved.strip_prefix(dir).ok().map(Path::to_path_buf)).unwrap_or_else(|| relative.clone());
                    (relative, *is_dir)
                })
                .collect();
            flat.feed = Some(Arc::new(followed));
        }
    }

    pub fn flat(&self, side: Side) -> Option<&Flat> {
        self.panel(side).flat.as_ref()
    }

    /// Ctrl+B: list every file under the active panel's directory, or go
    /// back to listing it as a directory. It belongs to that directory, and
    /// the panel lists as usual once it goes anywhere else. Back from it,
    /// the cursor lands on what holds the file it was on.
    pub fn toggle_flat(&mut self) {
        let side = self.active_side;
        let panel = self.panel(side);
        let (children, state) = (&panel.children, &panel.state);
        let cursor = state.selected().and_then(|index| children.get(index)).map(|item| item.name_os.clone());
        let before = self.flat(side).cloned();
        let landing = match self.panel_mut(side).flat.take() {
            Some(_) => cursor.and_then(|name| Path::new(&name).components().next().map(|first| first.as_os_str().to_os_string())),
            None => {
                self.panel_mut(side).flat = Some(Flat {
                    dir: self.panel_dir(side),
                    feed: None,
                    basket: false,
                });
                cursor
            }
        };
        self.reload_panel(side, landing.as_deref());
        // Still being read - a big tree - and Esc should leave it as it was.
        if let Some(pending) = &mut self.panel_mut(side).listing {
            pending.restore_flat = Some(before);
        }
    }
}
