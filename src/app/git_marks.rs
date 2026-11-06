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
    /// The one git running for the panel, and the directory it was asked
    /// about. Never more than one: in a large repository a status can take
    /// seconds, and a directory that keeps changing - a build writing into
    /// it - would otherwise have a new one started at every reread, while
    /// the ones before it still ran.
    running: Option<(PathBuf, Receiver<Option<crate::git::Status>>)>,
    /// Asked for again while one was running: once it is done, one more,
    /// for however many rereads came meanwhile.
    queued: bool,
}

impl AppState {
    pub fn git_status(&self, is_left: bool) -> Option<&crate::git::Status> {
        self.panel(is_left).git.status.as_ref()
    }

    /// Ask git again about `dir`, which the panel has just listed: in a
    /// repository, and not inside an archive, where git has nothing to say -
    /// and only while F11 has the marks on.
    /// The marks there are kept until the new ones come, so a reread does not
    /// flicker; another directory's go at once.
    pub(super) fn request_git_marks(&mut self, is_left: bool, dir: &Path, in_repository: bool) {
        let wanted = in_repository && self.options.git_marks;
        let git = &mut self.panel_mut(is_left).git;
        if git.dir != dir {
            git.status = None;
            git.dir = dir.to_path_buf();
        }
        if !wanted {
            // What is running is let go: it would only bring marks back.
            git.status = None;
            git.running = None;
            git.queued = false;
            return;
        }
        if git.running.is_some() {
            git.queued = true;
        } else {
            git.start();
        }
    }

    /// Take in whichever marks have come since the last frame, and start
    /// the git waiting for its turn.
    pub fn poll_git_marks(&mut self) {
        for is_left in [true, false] {
            let git = &mut self.panel_mut(is_left).git;
            let Some((asked, answer)) = &git.running else {
                continue;
            };
            let status = match answer.try_recv() {
                Ok(status) => status,
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => None,
            };
            // About a directory the panel has since left, it is no use.
            if *asked == git.dir {
                git.status = status;
            }
            git.running = None;
            if std::mem::take(&mut git.queued) {
                git.start();
            }
        }
    }
}

impl PanelGit {
    /// Git, on a thread of its own, about the panel's directory.
    fn start(&mut self) {
        let (sender, answer) = mpsc::channel();
        let dir = self.dir.clone();
        std::thread::spawn(move || {
            let _ = sender.send(crate::git::status(&dir).ok());
        });
        self.running = Some((self.dir.clone(), answer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rereads while git runs wait for it, and come to one more run.
    #[test]
    fn one_git_at_a_time_and_one_more_for_what_came_meanwhile() {
        let dir = std::env::temp_dir().join(format!("fm84-git-throttle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let made = std::process::Command::new("git").arg("-C").arg(&dir).args(["init", "--quiet"]).output().is_ok_and(|output| output.status.success());
        if !made {
            return;
        }
        let mut app_state = AppState::new();
        let other = dir.join("other");
        for _ in 0..3 {
            app_state.request_git_marks(true, &dir, true);
        }
        assert!(app_state.left.git.running.is_some() && app_state.left.git.queued);
        // The queued one runs once the first is in, and then nothing more.
        let wait = |app_state: &mut AppState| {
            let start = Instant::now();
            while app_state.left.git.running.is_some() && start.elapsed() < Duration::from_secs(10) {
                app_state.poll_git_marks();
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let start = Instant::now();
        while app_state.left.git.queued && start.elapsed() < Duration::from_secs(10) {
            app_state.poll_git_marks();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app_state.left.git.running.is_some(), "the one queued was not started");
        wait(&mut app_state);
        assert!(app_state.left.git.status.is_some() && !app_state.left.git.queued);

        // An answer about a directory the panel has left is not taken.
        app_state.request_git_marks(true, &dir, true);
        std::fs::create_dir_all(&other).unwrap();
        app_state.request_git_marks(true, &other, true);
        assert!(app_state.left.git.status.is_none());
        let start = Instant::now();
        while app_state.left.git.queued && start.elapsed() < Duration::from_secs(10) {
            app_state.poll_git_marks();
            assert!(app_state.left.git.status.is_none(), "marks for the directory left behind");
            std::thread::sleep(Duration::from_millis(5));
        }
        wait(&mut app_state);
        assert!(app_state.left.git.status.is_some() && app_state.left.git.dir == other);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
