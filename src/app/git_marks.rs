//! What git says of each entry in a panel: modified, staged, untracked,
//! ignored or in conflict. Asked on a thread of its own once the listing is
//! in, so a repository slow to answer never holds the panel up - the marks
//! come in when git does.

use super::*;

/// One panel's marks, and those on their way.
#[derive(Default)]
pub struct PanelGit {
    /// The directory they are for.
    pub dir: PathBuf,
    pub status: Option<crate::git::Status>,
    answer: Option<Receiver<Option<crate::git::Status>>>,
}

impl AppState {
    pub fn git_status(&self, is_left: bool) -> Option<&crate::git::Status> {
        if is_left { self.git_left.status.as_ref() } else { self.git_right.status.as_ref() }
    }

    fn panel_git(&mut self, is_left: bool) -> &mut PanelGit {
        if is_left { &mut self.git_left } else { &mut self.git_right }
    }

    /// Ask git again about `dir`, which the panel has just listed: in a
    /// repository, and not inside an archive, where git has nothing to say -
    /// and only while F11 has the marks on.
    /// The marks there are kept until the new ones come, so a reread does not
    /// flicker; another directory's go at once.
    pub(super) fn request_git_marks(&mut self, is_left: bool, dir: &Path, in_repository: bool) {
        let wanted = in_repository && self.options.git_marks;
        let git = self.panel_git(is_left);
        if git.dir != dir {
            git.status = None;
            git.dir = dir.to_path_buf();
        }
        if !wanted {
            git.status = None;
            git.answer = None;
            return;
        }
        let (sender, answer) = mpsc::channel();
        let worker_dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let _ = sender.send(crate::git::status(&worker_dir).ok());
        });
        git.answer = Some(answer);
    }

    /// Take in whichever marks have come since the last frame.
    pub fn poll_git_marks(&mut self) {
        for is_left in [true, false] {
            let git = self.panel_git(is_left);
            let Some(answer) = &git.answer else {
                continue;
            };
            match answer.try_recv() {
                Ok(status) => {
                    git.status = status;
                    git.answer = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => git.answer = None,
            }
        }
    }
}
