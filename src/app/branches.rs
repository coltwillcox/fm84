//! A click on a panel's git branch: the repository's branches, to change to
//! another of them.

use super::*;

/// The branches of the repository a panel is in, with the one highlighted.
pub struct BranchList {
    /// The directory of the panel it was opened from, which git runs in.
    pub dir: PathBuf,
    pub names: Vec<String>,
    /// The one checked out, if any: none while HEAD is detached.
    pub current: Option<usize>,
    pub index: usize,
}

/// A switch git is still making, past the moment the UI waited for it.
pub struct PendingSwitch {
    pub branch: String,
    pub(super) answer: Receiver<Result<(), String>>,
}

impl AppState {
    /// Open the list of a panel's branches, on the one checked out. Git is
    /// run on a thread and waited for only so long, as a listing is: on a
    /// repository on a mount that has stopped answering, the click says so
    /// rather than hold up the screen.
    pub fn open_branches(&mut self, is_left: bool) {
        if self.branch_switch.is_some() {
            return;
        }
        // Inside an archive, the repository the archive is in.
        let dir = self.panel_real_dir(is_left);
        let (sender, answer) = mpsc::channel();
        let worker_dir = dir.clone();
        std::thread::spawn(move || {
            let _ = sender.send(crate::git::branches(&worker_dir));
        });
        match answer.recv_timeout(crate::constants::GIT_WAIT) {
            Ok(Ok((names, _))) if names.is_empty() => self.display_error("No branches yet: nothing has been committed".to_string()),
            Ok(Ok((names, current))) => {
                self.dialog = Some(Dialog::Branches(Box::new(BranchList {
                    dir,
                    names,
                    current,
                    index: current.unwrap_or(0),
                })));
            }
            Ok(Err(e)) => self.display_error(e),
            Err(_) => self.display_error("git did not answer".to_string()),
        }
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_branch_cursor(&mut self, step: isize) {
        if let Some(Dialog::Branches(list)) = &mut self.dialog {
            list.index = list.index.saturating_add_signed(step).min(list.names.len().saturating_sub(1));
        }
    }

    /// A click on a row of the list: the first highlights it, and one on
    /// the row already highlighted switches to it, as Enter does.
    pub fn click_branch(&mut self, column: u16, row: u16) {
        let Some(&(_, index)) = self.branch_rows.iter().find(|(cell, _)| cell.contains(Position::new(column, row))) else {
            return;
        };
        if let Some(Dialog::Branches(list)) = &mut self.dialog {
            if list.index == index {
                self.switch_branch();
            } else {
                list.index = index;
            }
        }
    }

    /// Enter: check out the branch highlighted, and close the list. The one
    /// already checked out only closes it. Waited for as the list was, and
    /// when git takes longer, taken in when it is done.
    pub fn switch_branch(&mut self) {
        let Some(Dialog::Branches(list)) = self.dialog.take() else {
            return;
        };
        if Some(list.index) == list.current {
            return;
        }
        let Some(branch) = list.names.get(list.index).cloned() else {
            return;
        };
        let (sender, answer) = mpsc::channel();
        let (dir, worker_branch) = (list.dir, branch.clone());
        std::thread::spawn(move || {
            let _ = sender.send(crate::git::switch(&dir, &worker_branch));
        });
        match answer.recv_timeout(crate::constants::GIT_WAIT) {
            Ok(result) => self.finish_switch(result),
            Err(mpsc::RecvTimeoutError::Timeout) => self.branch_switch = Some(PendingSwitch { branch, answer }),
            Err(mpsc::RecvTimeoutError::Disconnected) => self.finish_switch(Err(format!("Switching to {branch} stopped unexpectedly"))),
        }
    }

    /// Take in a switch that went on past the wait, once git is done.
    pub fn poll_branch_switch(&mut self) {
        let Some(pending) = &self.branch_switch else {
            return;
        };
        let result = match pending.answer.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(format!("Switching to {} stopped unexpectedly", pending.branch)),
        };
        self.branch_switch = None;
        self.finish_switch(result);
    }

    /// A switch done, or refused. Both panels are read again: either may be
    /// in the repository, and a switch changes its files as well as its
    /// branch.
    fn finish_switch(&mut self, result: Result<(), String>) {
        self.reload_panel(true, None);
        self.reload_panel(false, None);
        if let Err(e) = result {
            self.display_error(e);
        }
    }
}
