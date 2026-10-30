//! Tabs, the drive and tab strips above the panels, and the drive picker.

use super::*;

/// A tab in a panel: somewhere it was left, to go back to - the directory,
/// the entry the cursor was on and what was selected there - and where it
/// has been, for Alt+Left and Alt+Right. The history lives here even for the
/// tab on show, since nothing else in the panel keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub dir: PathBuf,
    pub cursor: Option<OsString>,
    pub selected: HashSet<OsString>,
    pub back: Vec<Visit>,
    pub forward: Vec<Visit>,
}

impl Tab {
    pub fn new(dir: PathBuf) -> Self {
        Tab {
            dir,
            cursor: None,
            selected: HashSet::new(),
            back: Vec::new(),
            forward: Vec::new(),
        }
    }
}

/// One panel's tabs. The panel's own fields - its directory, cursor and
/// selection - are the tab it shows, so everything else goes on working
/// without knowing tabs exist. That tab's entry here is brought up to date
/// only as the panel leaves it; the rest wait here as they were left.
pub struct Tabs {
    pub list: Vec<Tab>,
    pub active: usize,
    pub strip: Strip,
}

impl Tabs {
    pub fn new(dirs: Vec<PathBuf>, active: usize) -> Self {
        let mut list: Vec<Tab> = dirs.into_iter().map(Tab::new).collect();
        if list.is_empty() {
            list.push(Tab::new(PathBuf::from(".")));
        }
        let active = active.min(list.len() - 1);
        Tabs { list, active, strip: Strip::default() }
    }
}

/// A tab being switched to: what was selected in it, and how to put the tabs
/// back if its directory turns out not to be readable.
pub(super) struct TabSwitch {
    pub(super) selected: HashSet<OsString>,
    pub(super) undo: TabUndo,
}

pub(super) enum TabUndo {
    /// The tab that was active before.
    Active(usize),
    /// The tab closed, and where it was.
    Reopen(usize, Tab),
}

/// What a click at `column` lands on, along a strip.
fn strip_hit(strip: &Strip, column: u16) -> Option<StripHit> {
    let index = slot_at(&strip.slots, column.checked_sub(strip.area.x)?)?;
    strip.hits.get(index).copied()
}

/// Which of a strip's slots a column falls in, measured from the strip's left
/// edge. None for the gaps around them: the space the strip opens with, and
/// everything past the last icon, where the mount's name is written.
pub(super) fn slot_at(slots: &[(u16, u16)], offset: u16) -> Option<usize> {
    slots.iter().position(|&(start, width)| offset >= start && offset < start + width)
}

impl AppState {
    /// A tab switch that did not happen, put back as it was.
    pub(super) fn undo_tab(&mut self, is_left: bool, tab: Option<TabSwitch>) {
        let Some(tab) = tab else {
            return;
        };
        let tabs = self.tabs_mut(is_left);
        match tab.undo {
            TabUndo::Active(previous) => tabs.active = previous,
            TabUndo::Reopen(index, closed) => {
                tabs.list.insert(index, closed);
                tabs.active = index;
            }
        }
    }

    /// Which panel's drive strip a position is on, if either.
    pub fn drive_strip_at(&self, column: u16, row: u16) -> Option<bool> {
        let position = Position::new(column, row);
        if self.drive_strip_left.area.contains(position) {
            Some(true)
        } else if self.drive_strip_right.area.contains(position) {
            Some(false)
        } else {
            None
        }
    }

    /// The panel and what a click landed on, when it was a drive icon or an
    /// arrow. The strip runs the width of its half, so most of it is neither:
    /// a click on the label beside them, or past the last one, is nothing.
    pub fn drive_at(&self, column: u16, row: u16) -> Option<(bool, StripHit)> {
        let is_left = self.drive_strip_at(column, row)?;
        let strip = if is_left { &self.drive_strip_left } else { &self.drive_strip_right };
        let hit = strip_hit(strip, column)?;
        match hit {
            StripHit::Item(mount) if mount >= self.mounts.len() => None,
            _ => Some((is_left, hit)),
        }
    }

    /// Move a panel's drive strip a page along, when not all of it fits.
    pub fn scroll_drive_strip(&mut self, is_left: bool, forward: bool) {
        let strip = if is_left { &mut self.drive_strip_left } else { &mut self.drive_strip_right };
        strip.scroll(forward);
    }

    /// Which panel's tab strip a position is on, if either.
    pub fn tab_strip_at(&self, column: u16, row: u16) -> Option<bool> {
        let position = Position::new(column, row);
        if self.tabs_left.strip.area.contains(position) {
            Some(true)
        } else if self.tabs_right.strip.area.contains(position) {
            Some(false)
        } else {
            None
        }
    }

    /// The panel and what a click landed on, when it was a tab or an arrow.
    pub fn tab_at(&self, column: u16, row: u16) -> Option<(bool, StripHit)> {
        let is_left = self.tab_strip_at(column, row)?;
        let tabs = if is_left { &self.tabs_left } else { &self.tabs_right };
        let hit = strip_hit(&tabs.strip, column)?;
        match hit {
            StripHit::Item(index) if index >= tabs.list.len() => None,
            _ => Some((is_left, hit)),
        }
    }

    pub fn scroll_tab_strip(&mut self, is_left: bool, forward: bool) {
        let tabs = if is_left { &mut self.tabs_left } else { &mut self.tabs_right };
        tabs.strip.scroll(forward);
    }

    pub(super) fn tabs_mut(&mut self, is_left: bool) -> &mut Tabs {
        if is_left { &mut self.tabs_left } else { &mut self.tabs_right }
    }

    /// The panel as it stands, as a tab to come back to.
    pub(super) fn panel_as_tab(&self, is_left: bool) -> Tab {
        let (dir, children, state, selected) = if is_left {
            (&self.dir_left, &self.children_left, &self.state_left, &self.selected_left)
        } else {
            (&self.dir_right, &self.children_right, &self.state_right, &self.selected_right)
        };
        let tabs = if is_left { &self.tabs_left } else { &self.tabs_right };
        let shown = &tabs.list[tabs.active];
        Tab {
            dir: dir.clone(),
            cursor: state.selected().and_then(|index| children.get(index)).map(|item| item.name_os.clone()),
            selected: selected.clone(),
            back: shown.back.clone(),
            forward: shown.forward.clone(),
        }
    }

    /// Every tab of a panel's directory, the one it shows as it is now, and
    /// which that is - for the strip's labels and the session file.
    pub fn tab_dirs(&self, is_left: bool) -> (Vec<PathBuf>, usize) {
        let (tabs, dir) = if is_left { (&self.tabs_left, &self.dir_left) } else { (&self.tabs_right, &self.dir_right) };
        let mut dirs: Vec<PathBuf> = tabs.list.iter().map(|tab| tab.dir.clone()).collect();
        dirs[tabs.active] = dir.clone();
        (dirs, tabs.active)
    }

    /// Ctrl+T: a new tab in the active panel, beside the one it is on and in
    /// the same directory, with the cursor where it was and nothing selected
    /// - the selection stays with the tab it was made in.
    pub fn new_tab(&mut self) {
        let is_left = self.is_left_active;
        if self.panel_busy(is_left) {
            return;
        }
        let here = self.panel_as_tab(is_left);
        let tabs = self.tabs_mut(is_left);
        tabs.list[tabs.active] = here.clone();
        // A history of its own, starting here.
        tabs.list.insert(
            tabs.active + 1,
            Tab {
                selected: HashSet::new(),
                back: Vec::new(),
                forward: Vec::new(),
                ..here
            },
        );
        tabs.active += 1;
        if is_left { self.selected_left.clear() } else { self.selected_right.clear() }
    }

    /// Ctrl+W, or a middle click: close a tab. The last one a panel has stays,
    /// since a panel has to show something. Closing the one shown moves to the
    /// one after it, or before it at the end, as a browser does - unless that
    /// one's directory cannot be read, when the close is refused and the tab
    /// stays, as it was. Not while the panel is still being read.
    pub fn close_tab(&mut self, is_left: bool, index: usize) {
        if self.panel_busy(is_left) {
            return;
        }
        let tabs = self.tabs_mut(is_left);
        if tabs.list.len() <= 1 || index >= tabs.list.len() {
            return;
        }
        let closed = tabs.list.remove(index);
        if index == tabs.active {
            tabs.active = index.min(tabs.list.len() - 1);
            self.show_tab(is_left, TabUndo::Reopen(index, closed));
        } else if index < tabs.active {
            tabs.active -= 1;
        }
    }

    /// Show another of a panel's tabs, leaving this one as it is to come back
    /// to. One whose directory cannot be read is refused, and this one stays
    /// active: moved there regardless, the panel went on showing the directory
    /// it was in under the other tab, which took that directory over for good
    /// as soon as it was left. Not while the panel is still being read, for
    /// the same reason: it is not yet showing the tab it says it is.
    pub fn select_tab(&mut self, is_left: bool, index: usize) {
        if self.panel_busy(is_left) {
            return;
        }
        let here = self.panel_as_tab(is_left);
        let tabs = self.tabs_mut(is_left);
        if index >= tabs.list.len() || index == tabs.active {
            return;
        }
        let previous = tabs.active;
        tabs.list[previous] = here;
        tabs.active = index;
        self.show_tab(is_left, TabUndo::Active(previous));
    }

    /// Ctrl+U, as in Midnight Commander: the panels change sides, each with
    /// its tabs, cursors, selections and history, and the focus goes with the
    /// panel it was in. Not while either is still being read.
    pub fn swap_panels(&mut self) {
        if self.panel_busy(true) || self.panel_busy(false) {
            return;
        }
        let (left, right) = (self.panel_as_tab(true), self.panel_as_tab(false));
        self.tabs_left.list[self.tabs_left.active] = left;
        self.tabs_right.list[self.tabs_right.active] = right;
        std::mem::swap(&mut self.tabs_left, &mut self.tabs_right);
        // Each filter goes with its directory, and is kept once there.
        std::mem::swap(&mut self.filter_left, &mut self.filter_right);
        std::mem::swap(&mut self.flat_left, &mut self.flat_right);
        std::mem::swap(&mut self.git_left, &mut self.git_right);
        for is_left in [true, false] {
            let active = self.tabs_mut(is_left).active;
            self.show_tab(is_left, TabUndo::Active(active));
        }
        self.is_left_active = !self.is_left_active;
    }

    /// Alt+I, as in Midnight Commander: the other panel goes to the active
    /// one's directory, its cursor on the same entry.
    pub fn same_dir_in_other_panel(&mut self) {
        let is_left = self.is_left_active;
        let here = self.panel_as_tab(is_left);
        self.open_dir(!is_left, here.dir, here.cursor.as_deref());
    }

    /// Ctrl+PageDown and Ctrl+PageUp: the next tab in the active panel, or the
    /// one before, round from the end to the start.
    pub fn cycle_tab(&mut self, forward: bool) {
        let is_left = self.is_left_active;
        let tabs = self.tabs_mut(is_left);
        let count = tabs.list.len();
        let next = if forward { (tabs.active + 1) % count } else { (tabs.active + count - 1) % count };
        self.select_tab(is_left, next);
    }

    /// Point the panel at the tab now active. A directory that cannot be read
    /// is refused as anywhere else, leaving the panel - selection and all -
    /// where it was, and `undo` puts the tabs back. A directory gone
    /// altogether is climbed out of, as anywhere else, and its selection goes
    /// with it.
    fn show_tab(&mut self, is_left: bool, undo: TabUndo) {
        let tabs = self.tabs_mut(is_left);
        let tab = tabs.list[tabs.active].clone();
        self.request_listing(is_left, tab.dir, true, tab.cursor, Some(TabSwitch { selected: tab.selected, undo }), Travel::Stay);
    }

    /// The mount a panel is sitting on: the longest one its directory is under.
    pub fn current_mount(&self, is_left: bool) -> Option<usize> {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| dir.starts_with(&mount.path))
            .max_by_key(|(_, mount)| mount.path.as_os_str().len())
            .map(|(index, _)| index)
    }

    /// Start choosing a drive for a panel, highlighting the one it is on.
    pub fn open_drive_picker(&mut self, is_left: bool) {
        // Kept current by the watcher, so a stick plugged in a moment ago is
        // already there.
        if self.mounts.is_empty() {
            return;
        }
        let current = self.current_mount(is_left).unwrap_or(0);
        self.dialog = Some(Dialog::DrivePicker { is_left, index: current });
        // Into view, even if the strip was scrolled away from it.
        let strip = if is_left { &mut self.drive_strip_left } else { &mut self.drive_strip_right };
        strip.followed = None;
    }

    pub fn move_drive_picker(&mut self, forward: bool) {
        let count = self.mounts.len();
        if let Some(Dialog::DrivePicker { index, .. }) = &mut self.dialog
            && count > 0
        {
            *index = if forward { (*index + 1) % count } else { (*index + count - 1) % count };
        }
    }

    /// Take the highlighted drive; the panel jumps to that mount point.
    pub fn confirm_drive_picker(&mut self) {
        if let Some(Dialog::DrivePicker { is_left, index }) = self.dialog.take()
            && let Some(mount) = self.mounts.get(index)
        {
            let path = mount.path.clone();
            self.open_dir(is_left, path, None);
        }
    }
}
