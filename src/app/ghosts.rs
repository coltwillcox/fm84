//! Alt+Shift+G: a ghost hunt - the files nobody has opened in years, by
//! the time each was last read or written, fed to the panel and drawn faded
//! with a ghost beside them. A clean-up list, with nothing chosen: what to
//! let go of is picked one at a time.

use super::*;

/// How long a file goes untouched before it counts as a ghost.
pub const GHOSTS_AFTER: Duration = Duration::from_secs(2 * 365 * 24 * 60 * 60);

/// What a ghost hunt found: under `base`, the files untouched since before
/// it began less GHOSTS_AFTER.
#[derive(Debug)]
pub struct Ghosts {
    pub base: PathBuf,
    pub found: Vec<PathBuf>,
}

impl AppState {
    /// Alt+Shift+G: what is selected, or with nothing selected the whole of
    /// the active panel's directory, hunted for ghosts as a job. Not where
    /// the filesystem keeps no access times - everything there would look
    /// as old as it was made.
    pub fn hunt_ghosts(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let base = self.panel_dir(side);
        if !crate::fs_ops::keeps_access_times(&base) {
            return self.display_error(t!("{path} is on a filesystem that keeps no access times", path = base.display()));
        }
        let panel = self.panel(side);
        let sources: Vec<PathBuf> = if panel.selected.is_empty() {
            vec![base.clone()]
        } else {
            panel.children.iter().filter(|item| panel.selected.contains(&item.name_os)).map(|item| item.path_in(&panel.dir)).collect()
        };
        let before = SystemTime::now().checked_sub(GHOSTS_AFTER).unwrap_or(SystemTime::UNIX_EPOCH);
        self.start_job(TransferKind::Ghosts, Clashes::Ask, move |progress| run_ghosts(sources, base, before, progress));
    }

    /// What was found, in the active panel, the cursor on the first.
    pub(super) fn feed_ghosts(&mut self, found: Ghosts) {
        let Ghosts { base, found } = found;
        let feed: Vec<(PathBuf, bool)> = found.iter().filter_map(|path| path.strip_prefix(&base).ok()).map(|path| (path.to_path_buf(), false)).collect();
        let Some((first, _)) = feed.first() else {
            self.find_note = Some(t!("No ghosts here").to_string());
            return;
        };
        let first = first.clone().into_os_string();
        let side = self.active_side;
        let panel = self.panel_mut(side);
        panel.flat = Some(Flat {
            dir: base.clone(),
            feed: Some(Arc::new(feed)),
            kind: FeedKind::Ghosts,
        });
        panel.selected.clear();
        if self.panel_dir(side) == base {
            self.reload_panel(side, Some(&first));
        } else {
            self.open_dir(side, base, Some(&first));
        }
    }
}

/// When a file was last read or written, whichever was later: a file
/// written last year was not forgotten, whatever its access time says.
fn last_touched(metadata: &std::fs::Metadata) -> Option<SystemTime> {
    match (metadata.accessed().ok(), metadata.modified().ok()) {
        (Some(accessed), Some(modified)) => Some(accessed.max(modified)),
        (one, other) => one.or(other),
    }
}

/// The worker thread behind Alt+Shift+G: every regular file under
/// `sources` last touched before `before`, oldest first. Counted in the
/// directories walked; one that cannot be read is said at the end.
fn run_ghosts(sources: Vec<PathBuf>, base: PathBuf, before: SystemTime, mut progress: JobProgress<'_>) {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = sources;
    while let Some(path) = stack.pop() {
        let Ok(metadata) = path.symlink_metadata() else { continue };
        if metadata.is_dir() {
            if !progress.step(Step::Starting(&path)) {
                return progress.finish(Ok(Transfer::Cancelled));
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => stack.extend(entries.flatten().map(|entry| entry.path())),
                Err(_) => {
                    let _ = progress.updates.send(JobUpdate::Skipped);
                }
            }
        } else if metadata.is_file()
            && first_name(&metadata, &mut seen)
            && let Some(touched) = last_touched(&metadata).filter(|&touched| touched < before)
        {
            found.push((touched, path));
        }
    }
    found.sort();
    let _ = progress.updates.send(JobUpdate::Ghosts(Ghosts {
        base,
        found: found.into_iter().map(|(_, path)| path).collect(),
    }));
    progress.finish(Ok(Transfer::Done));
}
