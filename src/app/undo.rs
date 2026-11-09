//! Ctrl+Z in the panels: the last rename, move, copy or move to the trash
//! taken back. Each job keeps a journal of what it actually did, and undoing
//! goes through it a step at a time, refusing a step whose target has changed
//! since - so taking a copy back never deletes something worked on after.

use super::*;
use std::io::Error;

/// Jobs kept to undo, the oldest let go of first.
const JOURNAL_LENGTH: usize = 32;

/// What an entry was like when a job left it, to tell whether it has been
/// changed since. A rename or a move undone only puts it back, so the entry
/// itself is enough; a copy undone deletes it, so a directory copied is
/// summed up all the way down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    kind: Kind,
    len: u64,
    modified: Option<SystemTime>,
    /// The inode, where there is one: another entry put under the same name
    /// is not the one the job left.
    id: Option<(u64, u64)>,
    /// For a directory summed up: its entries, their bytes, and the latest
    /// any of them was changed.
    inside: Option<(u64, u64, Option<SystemTime>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Link,
    Other,
}

impl Mark {
    /// `path` as it is now, links not followed; None when it is not there.
    /// `deep` sums up what is inside a directory as well.
    pub fn of(path: &Path, deep: bool) -> Option<Mark> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            Kind::Link
        } else if file_type.is_dir() {
            Kind::Dir
        } else if file_type.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        #[cfg(unix)]
        let id = {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino()))
        };
        #[cfg(not(unix))]
        let id = None;
        let inside = (deep && kind == Kind::Dir).then(|| {
            let mut sum = (0, 0, None);
            sum_up(path, &mut sum);
            sum
        });
        Some(Mark {
            kind,
            len: metadata.len(),
            modified: metadata.modified().ok(),
            id,
            inside,
        })
    }

    fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

/// Every entry under `dir`, counted, its bytes added and its time weighed,
/// links not followed into.
fn sum_up(dir: &Path, sum: &mut (u64, u64, Option<SystemTime>)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.path().symlink_metadata() else {
            continue;
        };
        sum.0 += 1;
        if metadata.is_file() {
            sum.1 += metadata.len();
        }
        if let Ok(modified) = metadata.modified() {
            sum.2 = sum.2.max(Some(modified));
        }
        if metadata.is_dir() {
            sum_up(&entry.path(), sum);
        }
    }
}

/// One thing a job did, and what undoes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// Renamed or moved from `from` to `to`, where it was left as `mark`.
    /// Undone by putting it back.
    Moved { from: PathBuf, to: PathBuf, mark: Mark },
    /// Copied to `to`, where nothing was before. Undone by deleting the copy.
    Copied { to: PathBuf, mark: Mark },
    /// Moved to the trash from `from`, no earlier than `since`, in seconds
    /// as the trash keeps them. Undone by restoring it.
    Trashed { from: PathBuf, since: i64 },
}

/// What kind of job a journal entry was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoKind {
    /// F2, on one name or several: put back together, since names that
    /// went round in a chain or were swapped need scratch names to go back.
    Rename,
    Copy,
    Move,
    Trash,
}

/// One job's journal: what it did, in the order it did it.
#[derive(Clone, Debug)]
pub struct Undo {
    pub kind: UndoKind,
    pub steps: Vec<Done>,
}

impl AppState {
    /// Keep what a job did, to be undone - if it did anything.
    pub(super) fn journal(&mut self, kind: UndoKind, steps: Vec<Done>) {
        if steps.is_empty() {
            return;
        }
        if self.undo.len() == JOURNAL_LENGTH {
            self.undo.remove(0);
        }
        self.undo.push(Undo { kind, steps });
    }

    /// Ctrl+Z: the last job taken back, as a job of its own.
    pub fn undo_last(&mut self) {
        let Some(undo) = self.undo.pop() else {
            self.find_note = Some(t!("Nothing to undo").to_string());
            return;
        };
        self.start_job(TransferKind::Undo, Clashes::Ask, move |progress| run_undo(undo, progress));
    }
}

/// Seconds since the epoch, as the trash dates what it takes.
pub(super) fn now_seconds() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// Passes a report on with every amount taken out: an undo counts in steps,
/// and a move back or a copy deleted would count bytes or entries.
struct InSteps<'a, 'b>(&'a mut (dyn Progress + 'b));

impl Progress for InSteps<'_, '_> {
    fn step(&mut self, step: Step<'_>) -> bool {
        match step {
            Step::Advanced(_) => self.0.step(Step::Advanced(0)),
            starting => self.0.step(starting),
        }
    }

    fn failed(&mut self, path: &Path, error: &Error) -> Reply {
        self.0.failed(path, error)
    }

    fn exists(&mut self, source: &Path, dest: &Path) -> Clash {
        self.0.exists(source, dest)
    }
}

/// Whether a step may go ahead: `check` says why not, and that is asked
/// about like any failure - looked at again, skipped, or the end of it.
fn may(path: &Path, report: &mut dyn Progress, check: impl Fn() -> Result<(), Error>) -> Result<bool, Error> {
    loop {
        let Err(error) = check() else {
            return Ok(true);
        };
        match report.failed(path, &error) {
            Reply::Retry => {}
            Reply::Skip | Reply::Delete => return Ok(false),
            Reply::Abort => return Err(error),
        }
    }
}

/// Why `to` cannot be put back at `from`, if it cannot: it is gone, it has
/// changed, or something has taken its old place.
fn check_moved(from: &Path, to: &Path, mark: &Mark, leaving: &HashSet<&Path>) -> Result<(), Error> {
    check_left(to, mark, false)?;
    // A name another of the same renames is leaving is free once it has.
    if path_exists(from) && !crate::fs_ops::is_same_entry(from, to) && !leaving.contains(from) {
        return Err(Error::new(std::io::ErrorKind::AlreadyExists, t!("{path} already exists", path = from.display())));
    }
    Ok(())
}

/// Why `path` is not as a job left it, if it is not.
fn check_left(path: &Path, mark: &Mark, deep: bool) -> Result<(), Error> {
    match Mark::of(path, deep) {
        None => Err(Error::new(std::io::ErrorKind::NotFound, t!("\"{name}\" is no longer there", name = path.display()))),
        Some(now) if now != *mark => Err(Error::other(t!("\"{name}\" has changed since", name = path.display()))),
        Some(_) => Ok(()),
    }
}

/// The worker thread behind Ctrl+Z: the journal gone through backwards, a
/// step at a time, each counted once.
fn run_undo(undo: Undo, mut progress: JobProgress<'_>) {
    let _ = progress.updates.send(JobUpdate::Total(undo.steps.len() as u64));
    let outcome = match undo.kind {
        UndoKind::Rename => undo_renames(&undo.steps, &mut progress),
        _ => undo_each(undo.steps, &mut progress),
    };
    progress.finish(outcome);
}

/// Names put back all together, through scratch names where they went round
/// in a chain - each refused on its own when it has changed.
fn undo_renames(steps: &[Done], report: &mut dyn Progress) -> Result<Transfer, Error> {
    let leaving: HashSet<&Path> = steps
        .iter()
        .filter_map(|step| match step {
            Done::Moved { to, .. } => Some(to.as_path()),
            _ => None,
        })
        .collect();
    let mut back = Vec::new();
    for step in steps {
        let Done::Moved { from, to, mark } = step else { continue };
        if !report.step(Step::Starting(to)) {
            return Ok(Transfer::Cancelled);
        }
        if may(to, report, || check_moved(from, to, mark, &leaving))? {
            back.push((to.clone(), from.clone()));
        }
    }
    rename_all(&back).map_err(|(_, error)| error)?;
    Ok(if report.step(Step::Advanced(steps.len() as u64)) { Transfer::Done } else { Transfer::Cancelled })
}

/// Each step taken back, the last one first.
fn undo_each(steps: Vec<Done>, report: &mut dyn Progress) -> Result<Transfer, Error> {
    for step in steps.into_iter().rev() {
        let outcome = match step {
            Done::Moved { from, to, mark } => {
                if !report.step(Step::Starting(&to)) {
                    return Ok(Transfer::Cancelled);
                }
                if may(&to, report, || check_moved(&from, &to, &mark, &HashSet::new()))? {
                    move_path(to, from, &mut InSteps(report))?
                } else {
                    Transfer::Done
                }
            }
            Done::Copied { to, mark } => {
                if !report.step(Step::Starting(&to)) {
                    return Ok(Transfer::Cancelled);
                }
                if may(&to, report, || check_left(&to, &mark, true))? {
                    delete_path(to, mark.is_dir(), &mut InSteps(report))?
                } else {
                    Transfer::Done
                }
            }
            Done::Trashed { from, since } => {
                if !report.step(Step::Starting(&from)) {
                    return Ok(Transfer::Cancelled);
                }
                may(&from, report, || restore(&from, since))?;
                Transfer::Done
            }
        };
        if outcome == Transfer::Cancelled || !report.step(Step::Advanced(1)) {
            return Ok(Transfer::Cancelled);
        }
    }
    Ok(Transfer::Done)
}

/// Put back what went to the trash from `from` no earlier than `since` -
/// the latest of them, should it have gone more than once - unless something
/// has taken its place.
#[cfg(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
fn restore(from: &Path, since: i64) -> Result<(), Error> {
    if path_exists(from) {
        return Err(Error::new(std::io::ErrorKind::AlreadyExists, t!("{path} already exists", path = from.display())));
    }
    // The trash keeps where it came from as the system spells it, which a
    // path through a link is not.
    let resolved = from.parent().and_then(|parent| parent.canonicalize().ok()).zip(from.file_name()).map(|(parent, name)| parent.join(name));
    let items = trash::os_limited::list().map_err(|error| Error::other(error.to_string()))?;
    // A second either way: the trash keeps whole seconds, in local time.
    let item = items
        .into_iter()
        .filter(|item| item.time_deleted >= since - 1)
        .filter(|item| {
            let original = item.original_path();
            original == from || Some(&original) == resolved.as_ref()
        })
        .max_by_key(|item| item.time_deleted)
        .ok_or_else(|| Error::new(std::io::ErrorKind::NotFound, t!("\"{name}\" is not in the trash", name = from.display())))?;
    trash::os_limited::restore_all([item]).map_err(|error| match error {
        trash::Error::RestoreCollision { path, .. } => Error::new(std::io::ErrorKind::AlreadyExists, t!("{path} already exists", path = path.display())),
        other => Error::other(format!("{other:?}")),
    })
}

/// The trash on macOS can be put into but not looked through.
#[cfg(not(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))))]
fn restore(_from: &Path, _since: i64) -> Result<(), Error> {
    Err(Error::new(std::io::ErrorKind::Unsupported, t!("Restoring from the trash is not supported on macOS")))
}
