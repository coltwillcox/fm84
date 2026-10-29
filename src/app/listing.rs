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
}

/// Everything a panel needs from the disk, read on the listing thread.
pub(super) fn read_listing(dir: &Path, options: &Options) -> Listed {
    // Ahead of climbing to a directory that is there, which would climb
    // straight out of an archive.
    if let Some(located) = crate::archive::locate(dir) {
        return read_archive_listing(dir, located, options);
    }
    let Some(found) = nearest_existing_dir(dir) else {
        return Listed {
            dir: None,
            items: Err(std::io::ErrorKind::NotFound.into()),
            branch: None,
            stamp: None,
            disk: None,
            archive: None,
        };
    };
    let stamp = std::fs::metadata(&found).and_then(|metadata| metadata.modified()).ok();
    let items = load_directory_rows(&found, options);
    let disk = disk_usage(&found);
    let branch = crate::fs_ops::git_branch(&found);
    Listed {
        dir: Some(found),
        items,
        branch,
        stamp,
        disk,
        archive: None,
    }
}

/// A directory inside an archive. One the archive no longer has is climbed
/// out of as one gone from the disk is, as far as the archive's root; a file
/// in it stops the climb at the directory it is in, for Ctrl+L to land on.
/// What the disk says - its space, the branch, the time to watch - is of the
/// archive file and the directory it is in.
fn read_archive_listing(dir: &Path, located: crate::archive::Located, options: &Options) -> Listed {
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
            (found, Ok(crate::fs_ops::archive_rows(&index, inner, options)))
        }
        // Said as what it is: an archive fm84 cannot read, not a missing
        // directory.
        Err(e) => (dir.to_path_buf(), Err(std::io::Error::new(e.kind(), format!("not readable as an archive ({e})")))),
    };
    let stamp = std::fs::metadata(&archive).and_then(|metadata| metadata.modified()).ok();
    Listed {
        dir: Some(found),
        items,
        branch: crate::fs_ops::git_branch(&outside),
        stamp,
        disk: disk_usage(&outside),
        archive: Some(archive),
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
    pub(super) fn listing(&self, is_left: bool) -> &Option<PendingListing> {
        if is_left { &self.listing_left } else { &self.listing_right }
    }

    fn listing_mut(&mut self, is_left: bool) -> &mut Option<PendingListing> {
        if is_left { &mut self.listing_left } else { &mut self.listing_right }
    }

    /// The directory a panel is in - or, while it is on its way somewhere
    /// slow to read, the one it is going to, which is the one meant. Asked of
    /// by what reaches into the panel from outside: a copy to it, or a search
    /// from it.
    pub fn panel_dir(&self, is_left: bool) -> PathBuf {
        match self.listing(is_left) {
            Some(pending) if pending.navigate => pending.dir.clone(),
            _ => {
                if is_left {
                    self.dir_left.clone()
                } else {
                    self.dir_right.clone()
                }
            }
        }
    }

    /// True while a panel's directory is still being read, past the moment
    /// the UI waits. Until it lands the panel shows what it did before, and
    /// only Tab and Esc mean anything there.
    pub fn panel_busy(&self, is_left: bool) -> bool {
        self.listing(is_left).is_some()
    }

    /// Point a panel at `dir` and read it. The single way a panel's directory
    /// changes - navigation, and anything else that jumps somewhere. `select`
    /// names the entry to land on, otherwise the cursor goes to the top.
    /// False when the directory was refused and the panel stayed put; true
    /// when it went there, or is still on its way.
    pub fn open_dir(&mut self, is_left: bool, dir: PathBuf, select: Option<&std::ffi::OsStr>) -> bool {
        self.request_listing(is_left, dir, true, select.map(std::ffi::OsStr::to_os_string), None, Travel::Go).unwrap_or(true)
    }

    /// Ctrl+Left and Ctrl+Right, as in Total Commander: the arrow names the
    /// panel, and it is sent to the directory under the cursor in the active
    /// one. On `..` that is the parent, landing on the directory left; on a
    /// file it is the directory the file is in, landing on the file. The focus
    /// stays where it is. Pointed at the active panel itself, it simply goes in.
    pub fn open_in_panel(&mut self, is_left: bool) {
        let (children, state, dir) = if self.is_left_active {
            (&self.children_left, &self.state_left, &self.dir_left)
        } else {
            (&self.children_right, &self.state_right, &self.dir_right)
        };
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
        self.open_dir(is_left, target, select.as_deref());
    }

    /// Reread one panel from disk. `prefer` names the entry to land on - the
    /// file just renamed or created. Otherwise the cursor keeps the *file* it
    /// was on rather than the row, since entries appearing or vanishing above
    /// shift every index below them; if that file is gone, the row is kept.
    /// A panel on its way somewhere else is left to get there: what it lands
    /// on is read fresh anyway.
    pub fn reload_panel(&mut self, is_left: bool, prefer: Option<&std::ffi::OsStr>) {
        if self.listing(is_left).as_ref().is_some_and(|pending| pending.navigate) {
            return;
        }
        let dir = if is_left { self.dir_left.clone() } else { self.dir_right.clone() };
        self.request_listing(is_left, dir, false, prefer.map(std::ffi::OsStr::to_os_string), None, Travel::Stay);
    }

    /// Read a panel's directory on a thread of its own, and take it in at
    /// once if it comes back inside the moment the UI waits - which on a disk
    /// that answers it always does, so nothing looks any different. Otherwise
    /// the panel goes on showing what it did, and poll_listings takes the
    /// listing in when it lands. Some(false) when it was refused there and
    /// then, None while it is still being read. A listing already on its way
    /// for this panel is dropped for this one.
    pub(super) fn request_listing(&mut self, is_left: bool, dir: PathBuf, navigate: bool, select: Option<OsString>, tab: Option<TabSwitch>, travel: Travel) -> Option<bool> {
        let (sender, answer) = mpsc::channel();
        let (worker_dir, options) = (dir.clone(), self.options.clone());
        std::thread::spawn(move || {
            let _ = sender.send(read_listing(&worker_dir, &options));
        });
        let pending = PendingListing { dir, navigate, select, tab, travel, answer };
        match pending.answer.recv_timeout(self.listing_wait) {
            Ok(listed) => {
                // One asked for earlier and still on its way read the
                // directory as it was before this one did. Left waiting, it
                // would land on top of this and take back what it shows.
                *self.listing_mut(is_left) = None;
                Some(self.apply_listing(is_left, pending, listed))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                *self.listing_mut(is_left) = Some(pending);
                None
            }
            // Gone without a word, which only happens if it panicked.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.display_error(format!("Cannot read {}", pending.dir.display()));
                self.undo_tab(is_left, pending.tab);
                Some(false)
            }
        }
    }

    /// Take in whichever listings have landed since the last frame.
    pub fn poll_listings(&mut self) {
        for is_left in [true, false] {
            let Some(pending) = self.listing_mut(is_left).take() else {
                continue;
            };
            match pending.answer.try_recv() {
                Ok(listed) => {
                    self.apply_listing(is_left, pending, listed);
                }
                Err(TryRecvError::Empty) => *self.listing_mut(is_left) = Some(pending),
                Err(TryRecvError::Disconnected) => {
                    self.display_error(format!("Cannot read {}", pending.dir.display()));
                    self.undo_tab(is_left, pending.tab);
                }
            }
        }
    }

    /// Esc on a panel still being read: stop waiting for it. The panel stays
    /// where it was, and a tab it was switching to is switched back. Whatever
    /// the thread finds is never asked for.
    pub fn cancel_listing(&mut self, is_left: bool) {
        if let Some(pending) = self.listing_mut(is_left).take() {
            self.undo_tab(is_left, pending.tab);
        }
    }

    /// Put a panel to a listing that has come back. False when it was a
    /// directory that is there but could not be read, which is refused - /root,
    /// to anyone else - and the panel stays where it was. Moved there anyway,
    /// the panel went on listing the directory it left under the name of one it
    /// could not show, and every operation built its paths in the wrong place.
    /// One that is gone is climbed out of, to the nearest parent still there.
    fn apply_listing(&mut self, is_left: bool, pending: PendingListing, listed: Listed) -> bool {
        let Some(found) = listed.dir else {
            self.display_error(format!("No such directory: {}", pending.dir.display()));
            self.undo_tab(is_left, pending.tab);
            self.forget_travel(is_left, pending.travel);
            return false;
        };
        if pending.navigate
            && found == pending.dir
            && let Err(e) = &listed.items
        {
            self.display_error(format!("Cannot open {}: {}", pending.dir.display(), e));
            self.undo_tab(is_left, pending.tab);
            self.forget_travel(is_left, pending.travel);
            return false;
        }
        // Ctrl+L to a file lands on it, in the directory it is in. Anything
        // else that is not there as typed stays where it is, and says so.
        let to_file = pending.travel == Travel::GoTo && found != pending.dir && pending.dir.parent() == Some(found.as_path()) && listed.items.as_ref().is_ok_and(|items| items.iter().any(|item| Some(item.name_os.as_os_str()) == pending.dir.file_name()));
        if pending.travel == Travel::GoTo && found != pending.dir && !to_file {
            self.display_error(format!("No such file or directory: {}", pending.dir.display()));
            return false;
        }
        let leaving = self.panel_as_tab(is_left);

        // Gone from under it, and climbed out of to `found`.
        let relocated = found != pending.dir && !to_file;
        if pending.navigate || relocated {
            // Cursor, search and selection all referred to entries elsewhere.
            if is_left {
                self.dir_left = found.clone();
                self.selected_left.clear();
                self.state_left.select(Some(0));
            } else {
                self.dir_right = found.clone();
                self.selected_right.clear();
                self.state_right.select(Some(0));
            }
            self.search_clear();
            if leaving.dir != found {
                self.forget_sizes_in(&leaving.dir);
            }
        }

        let (children, state) = if is_left { (&self.children_left, &self.state_left) } else { (&self.children_right, &self.state_right) };
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
                let selected = if is_left { &mut self.selected_left } else { &mut self.selected_right };
                prune_selection(selected, &items);
                let index = wanted.and_then(|name| items.iter().position(|item| item.name_os == name)).unwrap_or(previous_index).min(items.len().saturating_sub(1));
                if is_left {
                    self.children_left = items;
                    self.state_left.select(Some(index));
                } else {
                    self.children_right = items;
                    self.state_right.select(Some(index));
                }
            }
            // Gone unreadable while the panel is in it. Said once: the watcher
            // only speaks up again when the directory changes.
            Err(e) => self.display_error(e.to_string()),
        }

        // What is watched for a change: the archive file, for a directory
        // inside one, which has no time of its own.
        let probe = listed.archive.clone().unwrap_or_else(|| found.clone());
        if is_left {
            self.disk_left = listed.disk;
            self.branch_left = listed.branch;
            self.archive_left = listed.archive;
        } else {
            self.disk_right = listed.disk;
            self.branch_right = listed.branch;
            self.archive_right = listed.archive;
        }
        let watch = if is_left { &self.watch_left } else { &self.watch_right };
        if watch.as_ref().is_none_or(|watch| watch.dir != found) {
            let watch = crate::background::watch_dir(is_left, found.clone(), probe, listed.stamp, listed.disk, self.watch_interval, self.watch_sender.clone());
            if is_left {
                self.watch_left = Some(watch);
            } else {
                self.watch_right = Some(watch);
            }
        }

        // A tab brings back what was selected in it - when the panel got to
        // where the tab was, and not to some parent of it.
        if let Some(tab) = pending.tab {
            let (children, selected) = if is_left { (&self.children_left, &mut self.selected_left) } else { (&self.children_right, &mut self.selected_right) };
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
            let moved = self.record_travel(is_left, pending.travel, leaving, &found);
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
    fn record_travel(&mut self, is_left: bool, travel: Travel, leaving: Tab, found: &Path) -> bool {
        let moved = leaving.dir != found;
        let visit = Visit { dir: leaving.dir, cursor: leaving.cursor };
        let tabs = self.tabs_mut(is_left);
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
    fn forget_travel(&mut self, is_left: bool, travel: Travel) {
        let tabs = self.tabs_mut(is_left);
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
        let is_left = self.is_left_active;
        if self.panel_busy(is_left) {
            return;
        }
        let tabs = self.tabs_mut(is_left);
        let tab = &tabs.list[tabs.active];
        let Some(visit) = (if back { tab.back.last() } else { tab.forward.last() }).cloned() else {
            return;
        };
        let travel = if back { Travel::Back } else { Travel::Forward };
        self.request_listing(is_left, visit.dir, true, visit.cursor, None, travel);
    }

    /// Ctrl+L: edit the active panel's path where it is shown, as it stands.
    pub fn open_go_to(&mut self) {
        let mut text = self.panel_dir(self.is_left_active).display().to_string();
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
        let is_left = self.is_left_active;
        let path = crate::fs_ops::tidy_path(&crate::fs_ops::typed_path(text, &self.panel_dir(is_left)));
        self.request_listing(is_left, path, true, None, None, Travel::GoTo);
    }

    /// Tab on Ctrl+L's prompt: fill out the name being typed. Read on a
    /// thread, waited for only as long as a listing is: on a mount that does
    /// not answer, Tab does nothing rather than hang.
    pub fn complete_go_to(&mut self) {
        let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &self.dialog else {
            return;
        };
        let (text, base, hidden) = (input.text.clone(), self.panel_dir(self.is_left_active), self.options.show_hidden);
        let (sender, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(crate::fs_ops::complete_path(&text, &base, hidden));
        });
        if let Ok(Some(completed)) = answer.recv_timeout(self.listing_wait)
            && let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &mut self.dialog
        {
            input.set(completed);
        }
    }

    /// Take in what the watchers have seen since the last frame.
    pub fn poll_watchers(&mut self) {
        while let Ok(event) = self.watch_events.try_recv() {
            match event {
                WatchEvent::Changed { is_left, dir } => {
                    if dir == *(if is_left { &self.dir_left } else { &self.dir_right }) {
                        if is_left {
                            self.stale_left = true;
                        } else {
                            self.stale_right = true;
                        }
                    }
                }
                // Free space moves without the directory changing - a copy
                // anywhere else on the same filesystem shifts it.
                WatchEvent::Disk { is_left, dir, usage } => {
                    if dir == *(if is_left { &self.dir_left } else { &self.dir_right }) {
                        if is_left {
                            self.disk_left = usage;
                        } else {
                            self.disk_right = usage;
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
        for is_left in [true, false] {
            let stale = if is_left { self.stale_left } else { self.stale_right };
            if stale && !self.panel_busy(is_left) {
                if is_left {
                    self.stale_left = false;
                } else {
                    self.stale_right = false;
                }
                self.reload_panel(is_left, None);
            }
        }
    }
}
