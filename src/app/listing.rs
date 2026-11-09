//! Reading a panel's directory - off the interface thread, so a mount that has
//! stopped answering cannot freeze it - and where each tab has been.

use super::*;

/// A directory a tab has been in, and the entry the cursor was on as it left,
/// to land on again on the way back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub dir: PathBuf,
    pub cursor: Option<OsString>,
}

/// How a panel's move to another directory counts in its tab's history.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Travel {
    /// Not at all: a reread, or a tab switched to.
    Stay,
    /// A step forward, which drops what Alt+Right could have gone back to.
    Go,
    /// Ctrl+L, to a path that has to be there as typed: a directory, or a
    /// file to land on in its own. Unlike anywhere else, one that is gone
    /// is not climbed out of; it was a mistyping, and is said to be one.
    GoTo,
    /// Alt+Left and Alt+Right.
    Back,
    Forward,
}

/// A panel's directory being read on a thread, with what to do once it is.
pub struct PendingListing {
    /// The directory asked for.
    pub dir: PathBuf,
    /// Going there, rather than reading again where the panel already is.
    pub(super) navigate: bool,
    /// The entry to land on.
    pub(super) select: Option<OsString>,
    /// Set when it is a tab being switched to.
    pub(super) tab: Option<TabSwitch>,
    pub(super) travel: Travel,
    pub(super) answer: Receiver<Listed>,
    /// Stops a flat walk once the listing is not wanted.
    pub(super) _stop: StopOnDrop,
    /// Ctrl+B as it was before this listing was asked for by it, to put
    /// back if the listing never comes.
    pub(super) restore_flat: Option<Option<Flat>>,
    /// What to select once it is listed, in place of what was: a feed
    /// gone to, all of it chosen.
    pub(super) then_select: Option<HashSet<OsString>>,
}

/// Set as it goes. A listing dropped unused - Esc, or another in its place -
/// has a walk under Ctrl+B stop rather than go on through a whole disk for
/// nobody. Once taken in, the walk is over anyway.
#[derive(Default)]
pub(super) struct StopOnDrop(pub(super) Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
impl PendingListing {
    /// A panel on its way to `dir`, held there for a test: nothing is coming.
    pub fn going_to(dir: PathBuf) -> Self {
        let (_, answer) = mpsc::channel();
        PendingListing {
            dir,
            navigate: true,
            select: None,
            tab: None,
            travel: Travel::Go,
            answer,
            _stop: StopOnDrop::default(),
            restore_flat: None,
            then_select: None,
        }
    }
}

/// What a listing thread comes back with.
pub(super) struct Listed {
    /// Where it read: the directory asked for, or the nearest parent of it
    /// still there. None if not even the root could be reached.
    pub(super) dir: Option<PathBuf>,
    pub(super) items: std::io::Result<Vec<Item>>,
    /// The git branch it is on, if it is in a repository.
    pub(super) branch: Option<String>,
    /// The directory's modified time, taken before it was read, so a change
    /// made during the read is still seen as one.
    pub(super) stamp: Option<SystemTime>,
    pub(super) disk: Option<(u64, u64)>,
    /// The archive the directory runs through, when it is inside one.
    pub(super) archive: Option<PathBuf>,
    /// A flat listing that stopped at FLAT_MAX files.
    pub(super) truncated: bool,
}

/// How a flat panel is listed: Ctrl+B's walk, which `cancel` stops, or
/// what Find files fed it, by their paths from the directory.
pub(super) enum FlatRead<'a> {
    Walk(&'a AtomicBool),
    Feed(&'a [(PathBuf, bool)]),
}

/// Everything a panel needs from the disk, read on the listing thread.
/// `flat`, for a panel under Ctrl+B, has every file under `dir` listed, and
/// stops the walk once set - or, with what Find files fed the panel, those
/// entries alone - but only for `dir` itself: one climbed out of to a parent
/// is listed as any directory is.
pub(super) fn read_listing(dir: &Path, options: &Options, flat: Option<FlatRead<'_>>) -> Listed {
    // Ahead of climbing to a directory that is there, which would climb
    // straight out of an archive.
    if let Some(located) = crate::archive::locate(dir) {
        return read_archive_listing(dir, located, options, flat.is_some());
    }
    let Some(found) = nearest_existing_dir(dir) else {
        return Listed {
            dir: None,
            items: Err(std::io::ErrorKind::NotFound.into()),
            branch: None,
            stamp: None,
            disk: None,
            archive: None,
            truncated: false,
        };
    };
    let stamp = std::fs::metadata(&found).and_then(|metadata| metadata.modified()).ok();
    let mut truncated = false;
    let items = match flat.filter(|_| found == dir) {
        Some(FlatRead::Feed(feed)) => Ok(crate::fs_ops::load_feed_rows(&found, feed, options)),
        Some(FlatRead::Walk(cancel)) => crate::fs_ops::load_flat_rows(&found, options, cancel).map(|(items, cut)| {
            truncated = cut;
            items
        }),
        None => load_directory_rows(&found, options),
    };
    let disk = disk_usage(&found);
    let branch = crate::fs_ops::git_branch(&found);
    Listed {
        dir: Some(found),
        items,
        branch,
        stamp,
        disk,
        archive: None,
        truncated,
    }
}

/// A directory inside an archive. One the archive no longer has is climbed
/// out of as one gone from the disk is, as far as the archive's root; a file
/// in it stops the climb at the directory it is in, for Ctrl+L to land on.
/// What the disk says - its space, the branch, the time to watch - is of the
/// archive file and the directory it is in.
fn read_archive_listing(dir: &Path, located: crate::archive::Located, options: &Options, flat: bool) -> Listed {
    let archive = located.archive;
    let outside = archive.parent().unwrap_or(&archive).to_path_buf();
    let (found, items) = match crate::archive::index(&archive) {
        Ok(index) => {
            let mut inner = located.inner.as_path();
            while !index.is_dir(inner) {
                inner = inner.parent().unwrap_or(Path::new(""));
            }
            // Not joined when empty: that would end the path in a separator,
            // and a file's path with one on the end does not stat.
            let found = if inner.as_os_str().is_empty() { archive.clone() } else { archive.join(inner) };
            let rows = if flat && found == dir {
                crate::fs_ops::archive_flat_rows(&index, inner, options)
            } else {
                crate::fs_ops::archive_rows(&index, inner, options)
            };
            (found, Ok(rows))
        }
        // Said as what it is: an archive fm84 cannot read, not a missing
        // directory.
        Err(e) => (dir.to_path_buf(), Err(std::io::Error::new(e.kind(), t!("not readable as an archive ({e})", e)))),
    };
    let stamp = std::fs::metadata(&archive).and_then(|metadata| metadata.modified()).ok();
    Listed {
        dir: Some(found),
        items,
        branch: crate::fs_ops::git_branch(&outside),
        stamp,
        disk: disk_usage(&outside),
        archive: Some(archive),
        truncated: false,
    }
}

/// Drop selections whose file is no longer there.
///
/// A selection is keyed by name so it can follow its file across a re-sort, but
/// that leaves the name behind when the file itself goes. Nothing shows it: the
/// count and the operations all walk the files and ask whether each is
/// selected, so a name with no file behind it is silently inert - until
/// something takes that name again, at which point a file the user never picked
/// is selected, and joins the next copy, move or delete. Worse, those read the
/// selection *instead of* the cursor whenever it is not empty, so one such name
/// quietly redirects the whole operation.
pub(super) fn prune_selection(selected: &mut HashSet<OsString>, items: &[Item]) {
    let names: HashSet<&OsString> = items.iter().map(|item| &item.name_os).collect();
    selected.retain(|name| names.contains(name));
}

impl AppState {
    /// The directory a panel is in - or, while it is on its way somewhere
    /// slow to read, the one it is going to, which is the one meant. Asked of
    /// by what reaches into the panel from outside: a copy to it, or a search
    /// from it.
    pub fn panel_dir(&self, side: Side) -> PathBuf {
        match &self.panel(side).listing {
            Some(pending) if pending.navigate => pending.dir.clone(),
            _ => self.panel(side).dir.clone(),
        }
    }

    /// True while a panel's directory is still being read, past the moment
    /// the UI waits. Until it lands the panel shows what it did before, and
    /// only Tab and Esc mean anything there.
    pub fn panel_busy(&self, side: Side) -> bool {
        self.panel(side).listing.is_some()
    }

    /// Point a panel at `dir` and read it. The single way a panel's directory
    /// changes - navigation, and anything else that jumps somewhere. `select`
    /// names the entry to land on, otherwise the cursor goes to the top.
    /// False when the directory was refused and the panel stayed put; true
    /// when it went there, or is still on its way.
    pub fn open_dir(&mut self, side: Side, dir: PathBuf, select: Option<&std::ffi::OsStr>) -> bool {
        self.request_listing(side, dir, true, select.map(std::ffi::OsStr::to_os_string), None, Travel::Go).unwrap_or(true)
    }

    /// Ctrl+Left and Ctrl+Right, as in Total Commander: the arrow names the
    /// panel, and it is sent to the directory under the cursor in the active
    /// one. On `..` that is the parent, landing on the directory left; on a
    /// file it is the directory the file is in, landing on the file. The focus
    /// stays where it is. Pointed at the active panel itself, it simply goes in.
    pub fn open_in_panel(&mut self, side: Side) {
        let panel = self.panel(self.active_side);
        let (children, state, dir) = (&panel.children, &panel.state, &panel.dir);
        let item = state.selected().and_then(|index| children.get(index));
        let (target, select) = match item {
            Some(item) if item.is_parent() => match dir.parent() {
                Some(parent) => (parent.to_path_buf(), dir.file_name().map(std::ffi::OsStr::to_os_string)),
                None => return,
            },
            Some(item) if item.is_dir => (item.path_in(dir), None),
            Some(item) => (dir.clone(), Some(item.name_os.clone())),
            None => (dir.clone(), None),
        };
        self.open_dir(side, target, select.as_deref());
    }

    /// Reread one panel from disk. `prefer` names the entry to land on - the
    /// file just renamed or created. Otherwise the cursor keeps the *file* it
    /// was on rather than the row, since entries appearing or vanishing above
    /// shift every index below them; if that file is gone, the row is kept.
    /// A panel on its way somewhere else is left to get there: what it lands
    /// on is read fresh anyway.
    pub fn reload_panel(&mut self, side: Side, prefer: Option<&std::ffi::OsStr>) {
        if self.panel(side).listing.as_ref().is_some_and(|pending| pending.navigate) {
            return;
        }
        let dir = self.panel(side).dir.clone();
        self.request_listing(side, dir, false, prefer.map(std::ffi::OsStr::to_os_string), None, Travel::Stay);
    }

    /// Read a panel's directory on a thread of its own, and take it in at
    /// once if it comes back inside the moment the UI waits - which on a disk
    /// that answers it always does, so nothing looks any different. Otherwise
    /// the panel goes on showing what it did, and poll_listings takes the
    /// listing in when it lands. Some(false) when it was refused there and
    /// then, None while it is still being read. A listing already on its way
    /// for this panel is dropped for this one.
    pub(super) fn request_listing(&mut self, side: Side, dir: PathBuf, navigate: bool, select: Option<OsString>, tab: Option<TabSwitch>, travel: Travel) -> Option<bool> {
        let (sender, answer) = mpsc::channel();
        let (worker_dir, options) = (dir.clone(), self.options.clone());
        // Flat only for the directory it belongs to, and fed or walked.
        let flat = self.flat(side).filter(|flat| flat.dir == dir).map(|flat| flat.feed.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let flat = flat.as_ref().map(|feed| match feed {
                Some(feed) => FlatRead::Feed(feed),
                None => FlatRead::Walk(&worker_cancel),
            });
            let _ = sender.send(read_listing(&worker_dir, &options, flat));
        });
        let pending = PendingListing {
            dir,
            navigate,
            select,
            tab,
            travel,
            answer,
            _stop: StopOnDrop(cancel),
            restore_flat: None,
            then_select: None,
        };
        match pending.answer.recv_timeout(self.workers.listing_wait) {
            Ok(listed) => {
                // One asked for earlier and still on its way read the
                // directory as it was before this one did. Left waiting, it
                // would land on top of this and take back what it shows.
                self.panel_mut(side).listing = None;
                Some(self.apply_listing(side, pending, listed))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.panel_mut(side).listing = Some(pending);
                None
            }
            // Gone without a word, which only happens if it panicked.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.display_error(t!("Cannot read {path}", path = pending.dir.display()));
                self.undo_tab(side, pending.tab);
                Some(false)
            }
        }
    }

    /// Take in whichever listings have landed since the last frame.
    pub fn poll_listings(&mut self) {
        for side in Side::BOTH {
            let Some(pending) = self.panel_mut(side).listing.take() else {
                continue;
            };
            match pending.answer.try_recv() {
                Ok(listed) => {
                    self.apply_listing(side, pending, listed);
                }
                Err(TryRecvError::Empty) => self.panel_mut(side).listing = Some(pending),
                Err(TryRecvError::Disconnected) => {
                    self.display_error(t!("Cannot read {path}", path = pending.dir.display()));
                    if let Some(flat) = pending.restore_flat.clone() {
                        self.panel_mut(side).flat = flat;
                    }
                    self.undo_tab(side, pending.tab);
                }
            }
        }
    }

    /// Esc on a panel still being read: stop waiting for it. The panel stays
    /// where it was, and a tab it was switching to is switched back. Whatever
    /// the thread finds is never asked for.
    pub fn cancel_listing(&mut self, side: Side) {
        if let Some(pending) = self.panel_mut(side).listing.take() {
            if let Some(flat) = pending.restore_flat.clone() {
                self.panel_mut(side).flat = flat;
            }
            self.undo_tab(side, pending.tab);
        }
    }

    /// Put a panel to a listing that has come back. False when it was a
    /// directory that is there but could not be read, which is refused - /root,
    /// to anyone else - and the panel stays where it was. Moved there anyway,
    /// the panel went on listing the directory it left under the name of one it
    /// could not show, and every operation built its paths in the wrong place.
    /// One that is gone is climbed out of, to the nearest parent still there.
    fn apply_listing(&mut self, side: Side, pending: PendingListing, listed: Listed) -> bool {
        let Some(found) = listed.dir else {
            self.display_error(t!("No such directory: {path}", path = pending.dir.display()));
            self.undo_tab(side, pending.tab);
            self.forget_travel(side, pending.travel);
            return false;
        };
        if pending.navigate
            && found == pending.dir
            && let Err(e) = &listed.items
        {
            self.display_error(t!("Cannot open {path}: {e}", path = pending.dir.display(), e));
            self.undo_tab(side, pending.tab);
            self.forget_travel(side, pending.travel);
            return false;
        }
        // Ctrl+L to a file lands on it, in the directory it is in. Anything
        // else that is not there as typed stays where it is, and says so.
        let to_file = pending.travel == Travel::GoTo && found != pending.dir && pending.dir.parent() == Some(found.as_path()) && listed.items.as_ref().is_ok_and(|items| items.iter().any(|item| Some(item.name_os.as_os_str()) == pending.dir.file_name()));
        if pending.travel == Travel::GoTo && found != pending.dir && !to_file {
            self.display_error(t!("No such file or directory: {path}", path = pending.dir.display()));
            return false;
        }
        let leaving = self.panel_as_tab(side);
        // Flat for the directory Ctrl+B was pressed in, and no other.
        if self.flat(side).is_some_and(|flat| flat.dir != found) {
            self.panel_mut(side).flat = None;
        }

        // Gone from under it, and climbed out of to `found`.
        let relocated = found != pending.dir && !to_file;
        if pending.navigate || relocated {
            // Cursor, search and selection all referred to entries elsewhere.
            if side.is_left() {
                self.left.dir = found.clone();
                self.left.selected.clear();
                self.left.state.select(Some(0));
            } else {
                self.right.dir = found.clone();
                self.right.selected.clear();
                self.right.state.select(Some(0));
            }
            self.search_clear();
            if leaving.dir != found {
                self.forget_sizes_in(&leaving.dir);
                self.record_visit(&leaving.dir, &found);
            }
        }

        let panel = self.panel(side);
        let (children, state) = (&panel.children, &panel.state);
        let previous_index = state.selected().unwrap_or(0);
        let wanted = if to_file {
            pending.dir.file_name().map(std::ffi::OsStr::to_os_string)
        } else if relocated {
            None
        } else if pending.navigate {
            pending.select
        } else {
            // By the name the filesystem holds: two that are not valid UTF-8
            // can read the same, and the cursor would land on the wrong one.
            pending.select.or_else(|| children.get(previous_index).map(|item| item.name_os.clone()))
        };

        match listed.items {
            Ok(items) => {
                let items = self.filter_listing(side, &found, items);
                let selected = &mut self.panel_mut(side).selected;
                if let Some(chosen) = pending.then_select {
                    *selected = chosen;
                }
                prune_selection(selected, &items);
                let index = wanted.and_then(|name| items.iter().position(|item| item.name_os == name)).unwrap_or(previous_index).min(items.len().saturating_sub(1));
                if side.is_left() {
                    self.left.children = items;
                    self.left.state.select(Some(index));
                } else {
                    self.right.children = items;
                    self.right.state.select(Some(index));
                }
            }
            // Gone unreadable while the panel is in it. Said once: the watcher
            // only speaks up again when the directory changes.
            Err(e) => self.display_error(e.to_string()),
        }
        if listed.truncated {
            self.find_note = Some(t!("Only the first {count} files are listed", count = crate::constants::FLAT_MAX));
        }

        // What is watched for a change: the archive file, for a directory
        // inside one, which has no time of its own.
        let probe = listed.archive.clone().unwrap_or_else(|| found.clone());
        self.request_git_marks(side, &found, listed.branch.is_some() && listed.archive.is_none());
        self.request_info(side, listed.archive.is_some());
        if side.is_left() {
            self.left.disk = listed.disk;
            self.left.branch = listed.branch;
            self.left.archive = listed.archive;
        } else {
            self.right.disk = listed.disk;
            self.right.branch = listed.branch;
            self.right.archive = listed.archive;
        }
        let watch = &self.panel(side).watch;
        if watch.as_ref().is_none_or(|watch| watch.dir != found) {
            let watch = crate::background::watch_dir(found.clone(), probe, listed.stamp, listed.disk, self.workers.watch_interval, self.workers.watch_sender.clone());
            self.panel_mut(side).watch = Some(watch);
        }

        // A tab brings back what was selected in it - when the panel got to
        // where the tab was, and not to some parent of it.
        if let Some(tab) = pending.tab {
            let panel = self.panel_mut(side);
            let (children, selected) = (&panel.children, &mut panel.selected);
            if relocated {
                selected.clear();
            } else {
                *selected = tab.selected;
                prune_selection(selected, children);
            }
        }

        // Both of these keep what they gathered until the cursor moves to some
        // other entry, which is what makes them cheap to hold. A reread is the
        // other way the entry under the cursor changes - it was saved in the
        // editor, written over by a copy, or changed by something else
        // entirely - and neither would notice that on its own. Dropping them
        // here has them gathered again before the next frame, since the loop
        // refreshes both on its way to drawing one.
        self.preview = None;
        self.cursor_detail = None;
        if pending.navigate {
            let stepped = matches!(pending.travel, Travel::Back | Travel::Forward);
            let moved = self.record_travel(side, pending.travel, leaving, &found);
            // A step onto a directory gone since, climbed out of to where the
            // panel already was, did nothing anyone could see. Go on to the
            // one after it instead.
            if stepped && !moved {
                self.travel(pending.travel == Travel::Back);
            }
        }
        true
    }

    /// Keep a move that landed in the tab's history. `leaving` is the panel
    /// as it was before it moved, and `found` where it ended up. True when
    /// that is somewhere else.
    fn record_travel(&mut self, side: Side, travel: Travel, leaving: Tab, found: &Path) -> bool {
        let moved = leaving.dir != found;
        let visit = Visit { dir: leaving.dir, cursor: leaving.cursor };
        let tabs = &mut self.panel_mut(side).tabs;
        let tab = &mut tabs.list[tabs.active];
        match travel {
            Travel::Stay => return moved,
            Travel::Go | Travel::GoTo if moved => {
                tab.back.push(visit);
                tab.forward.clear();
            }
            Travel::Go | Travel::GoTo => {}
            // The visit stepped to is taken off, even when it turned out to be
            // where the panel already was - its directory gone, and climbed
            // out of to here - or the key would go there again and again.
            Travel::Back => {
                tab.back.pop();
                if moved {
                    tab.forward.push(visit);
                }
            }
            Travel::Forward => {
                tab.forward.pop();
                if moved {
                    tab.back.push(visit);
                }
            }
        }
        if tab.back.len() > crate::constants::HISTORY_MAX {
            tab.back.remove(0);
        }
        moved
    }

    /// A step back or forward to a directory that could not be shown: it is
    /// dropped, so the next press goes on past it.
    fn forget_travel(&mut self, side: Side, travel: Travel) {
        let tabs = &mut self.panel_mut(side).tabs;
        let tab = &mut tabs.list[tabs.active];
        match travel {
            Travel::Back => {
                tab.back.pop();
            }
            Travel::Forward => {
                tab.forward.pop();
            }
            _ => {}
        }
    }

    /// Alt+Left and Alt+Right: back to the directory the active panel's tab
    /// was in before, or forward again, landing on the entry the cursor was on
    /// there. The history is taken in step only as the panel gets there.
    pub fn travel(&mut self, back: bool) {
        let side = self.active_side;
        if self.panel_busy(side) {
            return;
        }
        let tabs = &mut self.panel_mut(side).tabs;
        let tab = &tabs.list[tabs.active];
        let Some(visit) = (if back { tab.back.last() } else { tab.forward.last() }).cloned() else {
            return;
        };
        let travel = if back { Travel::Back } else { Travel::Forward };
        self.request_listing(side, visit.dir, true, visit.cursor, None, travel);
    }

    /// Ctrl+L: edit the active panel's path where it is shown, as it stands.
    pub fn open_go_to(&mut self) {
        let mut text = self.panel_dir(self.active_side).display().to_string();
        if !text.ends_with(std::path::is_separator) {
            text.push(std::path::MAIN_SEPARATOR);
        }
        let mut input = TextInput::new();
        input.set(text);
        self.dialog = Some(Dialog::Prompt(PromptKind::GoTo, input));
    }

    /// Enter on Ctrl+L's prompt: go to the path typed, a directory or a file
    /// to land on. `~` and a path from the panel are read as F5 reads them.
    pub(super) fn go_to_path(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let side = self.active_side;
        let path = crate::fs_ops::tidy_path(&crate::fs_ops::typed_path(text, &self.panel_dir(side)));
        self.request_listing(side, path, true, None, None, Travel::GoTo);
    }

    /// Tab on Ctrl+L's prompt: fill out the name being typed. Read on a
    /// thread, waited for only as long as a listing is: on a mount that does
    /// not answer, Tab does nothing rather than hang.
    pub fn complete_go_to(&mut self) {
        let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &self.dialog else {
            return;
        };
        let (text, base, hidden) = (input.text.clone(), self.panel_dir(self.active_side), self.options.show_hidden);
        let (sender, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(crate::fs_ops::complete_path(&text, &base, hidden));
        });
        if let Ok(Some(completed)) = answer.recv_timeout(self.workers.listing_wait)
            && let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &mut self.dialog
        {
            input.set(completed);
        }
    }

    /// Take in what the watchers have seen since the last frame.
    pub fn poll_watchers(&mut self) {
        while let Ok(event) = self.workers.watch_events.try_recv() {
            match event {
                // To whichever panel shows the directory - both, when they are
                // in the same one.
                WatchEvent::Changed { dir } => {
                    for panel in [&mut self.left, &mut self.right] {
                        if panel.dir == dir {
                            panel.stale = true;
                        }
                    }
                }
                // Free space moves without the directory changing - a copy
                // anywhere else on the same filesystem shifts it.
                WatchEvent::Disk { dir, usage } => {
                    for panel in [&mut self.left, &mut self.right] {
                        if panel.dir == dir {
                            panel.disk = usage;
                        }
                    }
                }
                WatchEvent::Mounts(mounts) => self.mounts = mounts,
            }
        }
    }

    /// Reread any panel whose directory the watcher saw change. Not under an
    /// open dialog, which would move things out from under the user, nor while
    /// the panel is still being read: what lands then is fresh anyway.
    pub fn refresh_stale_panels(&mut self) {
        if self.is_modal_open() {
            return;
        }
        for side in Side::BOTH {
            let stale = self.panel(side).stale;
            if stale && !self.panel_busy(side) {
                self.panel_mut(side).stale = false;
                self.reload_panel(side, None);
            }
        }
    }
}
