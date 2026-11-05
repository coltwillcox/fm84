//! Alt+S: synchronize the panels' directories. They are compared all the
//! way down, and a plan made of what differs - each entry copied the way
//! its newer side says, or as it is changed here - and then run as one job.

use super::*;
use crate::sync::Difference;

/// What is to happen to one entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncAction {
    /// Copied from the left to the right, over what is there.
    ToRight,
    ToLeft,
    /// Deleted from the side that has it.
    DeleteLeft,
    DeleteRight,
    /// Left as it is.
    Skip,
}

/// One entry of the plan.
pub struct SyncEntry {
    pub difference: Difference,
    pub action: SyncAction,
}

impl SyncEntry {
    fn new(difference: Difference) -> Self {
        let action = default_action(&difference);
        SyncEntry { difference, action }
    }

    /// Whether `action` can be had: copied from a side that has it, deleted
    /// from one that has it alone. A file on one side and a directory on the
    /// other is not copied over either way, which would mix the two; it can
    /// only be left.
    pub fn allows(&self, action: SyncAction) -> bool {
        let (left, right) = (self.difference.left, self.difference.right);
        let mixed = matches!((left, right), (Some(l), Some(r)) if l.is_dir != r.is_dir);
        match action {
            SyncAction::Skip => true,
            _ if mixed => false,
            SyncAction::ToRight => left.is_some(),
            SyncAction::ToLeft => right.is_some(),
            SyncAction::DeleteLeft => left.is_some() && right.is_none(),
            SyncAction::DeleteRight => right.is_some() && left.is_none(),
        }
    }

    pub fn is_dir(&self) -> bool {
        self.difference.left.or(self.difference.right).is_some_and(|side| side.is_dir)
    }
}

/// Copied to the side it is missing from, or from the newer side; left
/// when neither is newer, or a file meets a directory, for the plan to
/// say which.
fn default_action(difference: &Difference) -> SyncAction {
    match (difference.left, difference.right) {
        (Some(_), None) => SyncAction::ToRight,
        (None, Some(_)) => SyncAction::ToLeft,
        (Some(l), Some(r)) if l.is_dir == r.is_dir => match difference.left_newer() {
            Some(true) => SyncAction::ToRight,
            Some(false) => SyncAction::ToLeft,
            None => SyncAction::Skip,
        },
        _ => SyncAction::Skip,
    }
}

/// The plan on screen, and the comparing still going on behind it.
pub struct SyncPlan {
    pub left: PathBuf,
    pub right: PathBuf,
    pub entries: Vec<SyncEntry>,
    pub cursor: usize,
    pub scroll: usize,
    /// Rows the plan showed last, for a page up or down.
    pub room: usize,
    /// Where the comparing is, while it goes on.
    pub looking_in: Option<PathBuf>,
    pub done: bool,
    pub unreadable: usize,
    /// Enter pressed once on a plan that deletes: the next one runs it.
    pub armed: bool,
    cancel: Arc<AtomicBool>,
    updates: Receiver<crate::sync::Update>,
}

impl Drop for SyncPlan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl SyncPlan {
    /// How many entries go each way, are deleted, and are left.
    pub fn counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for entry in &self.entries {
            let at = match entry.action {
                SyncAction::ToRight => 0,
                SyncAction::ToLeft => 1,
                SyncAction::DeleteLeft | SyncAction::DeleteRight => 2,
                SyncAction::Skip => 3,
            };
            counts[at] += 1;
        }
        counts
    }
}

impl AppState {
    /// Alt+S: compare the panels' directories, all the way down. Not
    /// inside an archive, which cannot be written to, nor with one directory
    /// inside the other, where copying across would copy into itself.
    pub fn open_sync(&mut self) {
        if self.panel_busy(true) || self.panel_busy(false) {
            return;
        }
        if self.panel_archive(true).is_some() || self.panel_archive(false).is_some() {
            self.display_error(crate::app::READ_ONLY.to_string());
            return;
        }
        let (left, right) = (self.panel_dir(true), self.panel_dir(false));
        // As the directories really are: one reached through a link may be
        // inside the other all the same.
        let real = |dir: &Path| dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let (real_left, real_right) = (real(&left), real(&right));
        if real_left.starts_with(&real_right) || real_right.starts_with(&real_left) {
            let said = if real_left == real_right { "Both panels show the same directory" } else { "One panel's directory is inside the other's" };
            self.display_error(format!("{said}: nothing to synchronize"));
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let (worker_cancel, worker_left, worker_right, show_hidden) = (Arc::clone(&cancel), left.clone(), right.clone(), self.options.show_hidden);
        std::thread::spawn(move || {
            crate::sync::compare(&worker_left, &worker_right, show_hidden, &worker_cancel, &mut |update| sender.send(update).is_ok());
        });
        self.dialog = Some(Dialog::Sync(Box::new(SyncPlan {
            left,
            right,
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            room: 1,
            looking_in: Some(PathBuf::new()),
            done: false,
            unreadable: 0,
            armed: false,
            cancel,
            updates,
        })));
    }

    fn sync_plan(&mut self) -> Option<&mut SyncPlan> {
        match &mut self.dialog {
            Some(Dialog::Sync(plan)) => Some(plan),
            _ => None,
        }
    }

    /// Take in what the comparing has found since the last frame.
    pub fn poll_sync(&mut self) {
        let Some(plan) = self.sync_plan() else {
            return;
        };
        loop {
            match plan.updates.try_recv() {
                Ok(crate::sync::Update::Looking(dir)) => plan.looking_in = Some(dir),
                Ok(crate::sync::Update::Found(difference)) => plan.entries.push(SyncEntry::new(difference)),
                Ok(crate::sync::Update::Finished { unreadable }) => {
                    plan.unreadable = unreadable;
                    plan.looking_in = None;
                    plan.done = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    plan.looking_in = None;
                    plan.done = true;
                    break;
                }
            }
        }
    }

    /// The cursor `by` entries along, stopping at either end. Any key but a
    /// second Enter takes back the first.
    pub fn sync_move(&mut self, by: isize) {
        if let Some(plan) = self.sync_plan() {
            plan.armed = false;
            plan.cursor = plan.cursor.saturating_add_signed(by).min(plan.entries.len().saturating_sub(1));
            if plan.cursor < plan.scroll {
                plan.scroll = plan.cursor;
            } else if plan.cursor >= plan.scroll + plan.room {
                plan.scroll = plan.cursor + 1 - plan.room;
            }
        }
    }

    /// → ← Delete Space on the plan: what happens to the entry under the
    /// cursor, if it can, and on to the next - so a run of them is set by
    /// holding the key. Delete deletes from whichever side has it alone.
    pub fn sync_set(&mut self, action: SyncAction) {
        let Some(plan) = self.sync_plan() else {
            return;
        };
        let Some(entry) = plan.entries.get_mut(plan.cursor) else {
            return;
        };
        let action = match action {
            SyncAction::DeleteLeft | SyncAction::DeleteRight if entry.difference.left.is_none() => SyncAction::DeleteRight,
            SyncAction::DeleteLeft | SyncAction::DeleteRight => SyncAction::DeleteLeft,
            other => other,
        };
        if !entry.allows(action) {
            return;
        }
        entry.action = action;
        self.sync_move(1);
    }

    /// The > and < keys: make one side as the other is - everything copied
    /// onto it, and what only it has deleted. R puts back what the
    /// comparing said.
    pub fn sync_mirror(&mut self, onto_right: Option<bool>) {
        let Some(plan) = self.sync_plan() else {
            return;
        };
        plan.armed = false;
        for entry in &mut plan.entries {
            let wanted = match onto_right {
                None => default_action(&entry.difference),
                Some(true) if entry.difference.left.is_none() => SyncAction::DeleteRight,
                Some(true) => SyncAction::ToRight,
                Some(false) if entry.difference.right.is_none() => SyncAction::DeleteLeft,
                Some(false) => SyncAction::ToLeft,
            };
            entry.action = if entry.allows(wanted) { wanted } else { SyncAction::Skip };
        }
    }

    /// Enter: run the plan as one job, the copies writing over what is in
    /// the way - the plan has said which way each goes. One that deletes
    /// asks for a second Enter first, unless F11 says not to ask. Not while
    /// the comparing is still going on.
    pub fn run_sync(&mut self) {
        let confirm = self.options.confirm_delete;
        let to_trash = self.options.delete_to_trash;
        let Some(plan) = self.sync_plan() else {
            return;
        };
        if !plan.done {
            return;
        }
        let [_, _, deleting, _] = plan.counts();
        if deleting > 0 && confirm && !plan.armed {
            plan.armed = true;
            return;
        }
        let (left, right) = (plan.left.clone(), plan.right.clone());
        let steps: Vec<SyncStep> = plan
            .entries
            .iter()
            .filter_map(|entry| {
                let (at_left, at_right, is_dir) = (left.join(&entry.difference.path), right.join(&entry.difference.path), entry.is_dir());
                match entry.action {
                    SyncAction::ToRight => Some(SyncStep::Copy(at_left, at_right, is_dir)),
                    SyncAction::ToLeft => Some(SyncStep::Copy(at_right, at_left, is_dir)),
                    SyncAction::DeleteLeft => Some(SyncStep::Delete(at_left, is_dir)),
                    SyncAction::DeleteRight => Some(SyncStep::Delete(at_right, is_dir)),
                    SyncAction::Skip => None,
                }
            })
            .collect();
        self.dialog = None;
        if steps.is_empty() {
            self.find_note = Some("Nothing to synchronize".to_string());
            return;
        }
        self.start_sync(steps, to_trash);
    }
}

/// One step of a synchronization, as the job takes it.
pub enum SyncStep {
    /// From, to, and whether it is a directory.
    Copy(PathBuf, PathBuf, bool),
    Delete(PathBuf, bool),
}
