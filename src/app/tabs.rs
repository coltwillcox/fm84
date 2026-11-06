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
    pub(super) fn undo_tab(&mut self, side: Side, tab: Option<TabSwitch>) {
        let Some(tab) = tab else {
            return;
        };
        let tabs = &mut self.panel_mut(side).tabs;
        match tab.undo {
            TabUndo::Active(previous) => tabs.active = previous,
            TabUndo::Reopen(index, closed) => {
                tabs.list.insert(index, closed);
                tabs.active = index;
            }
        }
    }

    /// Which panel's drive strip a position is on, if either.
    pub fn drive_strip_at(&self, column: u16, row: u16) -> Option<Side> {
        let position = Position::new(column, row);
        if self.drive_strip_left.area.contains(position) {
            Some(Side::Left)
        } else if self.drive_strip_right.area.contains(position) {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// The panel and what a click landed on, when it was a drive icon or an
    /// arrow. The strip runs the width of its half, so most of it is neither:
    /// a click on the label beside them, or past the last one, is nothing.
    pub fn drive_at(&self, column: u16, row: u16) -> Option<(Side, StripHit)> {
        let side = self.drive_strip_at(column, row)?;
        let strip = if side.is_left() { &self.drive_strip_left } else { &self.drive_strip_right };
        let hit = strip_hit(strip, column)?;
        match hit {
            StripHit::Item(mount) if mount >= self.mounts.len() => None,
            _ => Some((side, hit)),
        }
    }

    /// Move a panel's drive strip a page along, when not all of it fits.
    pub fn scroll_drive_strip(&mut self, side: Side, forward: bool) {
        let strip = if side.is_left() { &mut self.drive_strip_left } else { &mut self.drive_strip_right };
        strip.scroll(forward);
    }

    /// Which panel's tab strip a position is on, if either.
    pub fn tab_strip_at(&self, column: u16, row: u16) -> Option<Side> {
        let position = Position::new(column, row);
        if self.left.tabs.strip.area.contains(position) {
            Some(Side::Left)
        } else if self.right.tabs.strip.area.contains(position) {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// The panel and what a click landed on, when it was a tab or an arrow.
    pub fn tab_at(&self, column: u16, row: u16) -> Option<(Side, StripHit)> {
        let side = self.tab_strip_at(column, row)?;
        let tabs = &self.panel(side).tabs;
        let hit = strip_hit(&tabs.strip, column)?;
        match hit {
            StripHit::Item(index) if index >= tabs.list.len() => None,
            _ => Some((side, hit)),
        }
    }

    pub fn scroll_tab_strip(&mut self, side: Side, forward: bool) {
        let tabs = &mut self.panel_mut(side).tabs;
        tabs.strip.scroll(forward);
    }

    /// The panel as it stands, as a tab to come back to.
    pub(super) fn panel_as_tab(&self, side: Side) -> Tab {
        let panel = self.panel(side);
        let (dir, children, state, selected) = (&panel.dir, &panel.children, &panel.state, &panel.selected);
        let tabs = &self.panel(side).tabs;
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
    pub fn tab_dirs(&self, side: Side) -> (Vec<PathBuf>, usize) {
        let panel = self.panel(side);
        let (tabs, dir) = (&panel.tabs, &panel.dir);
        let mut dirs: Vec<PathBuf> = tabs.list.iter().map(|tab| tab.dir.clone()).collect();
        dirs[tabs.active] = dir.clone();
        (dirs, tabs.active)
    }

    /// Ctrl+T: a new tab in the active panel, beside the one it is on and in
    /// the same directory, with the cursor where it was and nothing selected
    /// - the selection stays with the tab it was made in.
    pub fn new_tab(&mut self) {
        let side = self.active_side;
        if self.panel_busy(side) {
            return;
        }
        let here = self.panel_as_tab(side);
        let tabs = &mut self.panel_mut(side).tabs;
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
        self.panel_mut(side).selected.clear()
    }

    /// Ctrl+W, or a middle click: close a tab. The last one a panel has stays,
    /// since a panel has to show something. Closing the one shown moves to the
    /// one after it, or before it at the end, as a browser does - unless that
    /// one's directory cannot be read, when the close is refused and the tab
    /// stays, as it was. Not while the panel is still being read.
    pub fn close_tab(&mut self, side: Side, index: usize) {
        if self.panel_busy(side) {
            return;
        }
        let tabs = &mut self.panel_mut(side).tabs;
        if tabs.list.len() <= 1 || index >= tabs.list.len() {
            return;
        }
        let closed = tabs.list.remove(index);
        if index == tabs.active {
            tabs.active = index.min(tabs.list.len() - 1);
            self.show_tab(side, TabUndo::Reopen(index, closed));
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
    pub fn select_tab(&mut self, side: Side, index: usize) {
        if self.panel_busy(side) {
            return;
        }
        let here = self.panel_as_tab(side);
        let tabs = &mut self.panel_mut(side).tabs;
        if index >= tabs.list.len() || index == tabs.active {
            return;
        }
        let previous = tabs.active;
        tabs.list[previous] = here;
        tabs.active = index;
        self.show_tab(side, TabUndo::Active(previous));
    }

    /// Ctrl+U, as in Midnight Commander: the panels change sides, each with
    /// its tabs, cursors, selections and history, and the focus goes with the
    /// panel it was in. Not while either is still being read.
    pub fn swap_panels(&mut self) {
        if self.panel_busy(Side::Left) || self.panel_busy(Side::Right) {
            return;
        }
        // Everything a panel has goes with it - its filter, its git marks, its
        // watcher, which reports by directory - so nothing is read again.
        std::mem::swap(&mut self.left, &mut self.right);
        self.active_side = !self.active_side;
    }

    /// Alt+I, as in Midnight Commander: the other panel goes to the active
    /// one's directory, its cursor on the same entry.
    pub fn same_dir_in_other_panel(&mut self) {
        let side = self.active_side;
        let here = self.panel_as_tab(side);
        self.open_dir(!side, here.dir, here.cursor.as_deref());
    }

    /// Ctrl+PageDown and Ctrl+PageUp: the next tab in the active panel, or the
    /// one before, round from the end to the start.
    pub fn cycle_tab(&mut self, forward: bool) {
        let side = self.active_side;
        let tabs = &mut self.panel_mut(side).tabs;
        let count = tabs.list.len();
        let next = if forward { (tabs.active + 1) % count } else { (tabs.active + count - 1) % count };
        self.select_tab(side, next);
    }

    /// Point the panel at the tab now active. A directory that cannot be read
    /// is refused as anywhere else, leaving the panel - selection and all -
    /// where it was, and `undo` puts the tabs back. A directory gone
    /// altogether is climbed out of, as anywhere else, and its selection goes
    /// with it.
    fn show_tab(&mut self, side: Side, undo: TabUndo) {
        let tabs = &mut self.panel_mut(side).tabs;
        let tab = tabs.list[tabs.active].clone();
        self.request_listing(side, tab.dir, true, tab.cursor, Some(TabSwitch { selected: tab.selected, undo }), Travel::Stay);
    }

    /// The mount a panel is sitting on: the longest one its directory is under.
    pub fn current_mount(&self, side: Side) -> Option<usize> {
        let dir = &self.panel(side).dir;
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| dir.starts_with(&mount.path))
            .max_by_key(|(_, mount)| mount.path.as_os_str().len())
            .map(|(index, _)| index)
    }

    /// Start choosing a drive for a panel, highlighting the one it is on.
    pub fn open_drive_picker(&mut self, side: Side) {
        // Kept current by the watcher, so a stick plugged in a moment ago is
        // already there.
        if self.mounts.is_empty() {
            return;
        }
        let current = self.current_mount(side).unwrap_or(0);
        self.dialog = Some(Dialog::DrivePicker { side, index: current });
        // Into view, even if the strip was scrolled away from it.
        let strip = if side.is_left() { &mut self.drive_strip_left } else { &mut self.drive_strip_right };
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
        if let Some(Dialog::DrivePicker { side, index }) = self.dialog.take()
            && let Some(mount) = self.mounts.get(index)
        {
            let path = mount.path.clone();
            self.open_dir(side, path, None);
        }
    }
}
