//! Deleting, for good or to the trash, an entry at a time so a job can report
//! and stop.

use super::*;

/// Entries a delete will remove, for the progress bar to count against. A
/// directory counts itself as well as everything inside it, which is what the
/// walk below removes. Anything unreadable counts as nothing rather than
/// failing the job, since this is only ever a denominator.
pub fn count_entries(items: &[(PathBuf, bool)]) -> u64 {
    fn walk(path: &Path) -> u64 {
        let Ok(entries) = read_dir(path) else {
            return 1;
        };
        1 + entries
            .flatten()
            .map(|entry| match entry.file_type() {
                Ok(file_type) if file_type.is_dir() => walk(&entry.path()),
                _ => 1,
            })
            .sum::<u64>()
    }

    items
        .iter()
        .map(|(path, is_dir)| {
            let link = fs::symlink_metadata(path).map(|meta| meta.file_type().is_symlink()).unwrap_or(false);
            if *is_dir && !link { walk(path) } else { 1 }
        })
        .sum()
}

/// Remove a file, or a directory and everything in it, reporting each entry as
/// it goes and stopping when the report says to.
///
/// Walked rather than handed to remove_dir_all, which has nowhere to report
/// from and no way to be interrupted: a large tree on a slow disk would hold
/// the whole app still. A cancelled delete leaves whatever it had not reached.
pub fn delete_path(path: PathBuf, is_dir: bool, report: Report<'_>) -> Result<Transfer, Error> {
    // A symlink is removed as a link, never followed - including one pointing at
    // a directory, which reaches here with is_dir set because the panel treats it
    // as one. Walking into one would delete what it points at.
    let is_symlink = path.symlink_metadata().map(|metadata| metadata.file_type().is_symlink()).unwrap_or(false);

    let outcome = if is_dir && !is_symlink { delete_dir_recursive(&path, report)? } else { remove_one(&path, false, report)? };
    Ok(outcome.into())
}

/// Move a file or a directory to the system's trash, from where it can be put
/// back. Counted as one, however much is in it: on one filesystem it is a
/// rename and over at once, with nothing to count inside.
///
/// What the trash will not take - a read-only disk, one with nowhere to keep
/// a trash - is asked about like any other failure, with deleting it for good
/// as one more answer.
pub fn trash_path(path: PathBuf, is_dir: bool, report: Report<'_>) -> Result<Transfer, Error> {
    if !report.step(Step::Starting(&path)) {
        return Ok(Transfer::Cancelled);
    }
    loop {
        let error = match trash::delete(&path) {
            Ok(()) => break,
            Err(error) => trash_error(error),
        };
        match report.failed(&path, &error) {
            Reply::Retry => {}
            Reply::Skip => break,
            Reply::Abort => return Err(error),
            Reply::Delete => {
                // Its entries are not what this job counts, so they pass by
                // without adding to it; the one below counts the item.
                let mut uncounted = Uncounted(&mut *report);
                if delete_path(path, is_dir, &mut uncounted)? == Transfer::Cancelled {
                    return Ok(Transfer::Cancelled);
                }
                break;
            }
        }
    }
    Ok(if report.step(Step::Advanced(1)) { Transfer::Done } else { Transfer::Cancelled })
}

/// The trash crate's errors print with a prefix and a Debug dump. What
/// matters to someone looking at the popup is what went wrong.
fn trash_error(error: trash::Error) -> Error {
    match error {
        // The freedesktop trash fails with the filesystem's own error, which
        // already says it plainly. Only that build of the crate has it.
        #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))]
        trash::Error::FileSystem { source, .. } => source,
        trash::Error::TargetedRoot => Error::other("A root directory cannot go to the trash"),
        trash::Error::CouldNotAccess { target } => Error::other(format!("Cannot reach {target}")),
        trash::Error::Os { description, .. } | trash::Error::Unknown { description } => Error::other(description),
        other => Error::other(format!("Cannot move to the trash: {other:?}")),
    }
}

/// Passes a report on with every amount taken out, so a walk inside another
/// job's step counts as nothing of its own - but still stops when it is told.
pub(super) struct Uncounted<'a, 'b>(pub(super) &'a mut (dyn Progress + 'b));

impl Progress for Uncounted<'_, '_> {
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

/// Remove one entry, announcing it first so the popup can name it, and counting
/// it once it is dealt with - removed, or skipped when it would not go.
fn remove_one(path: &Path, is_dir: bool, report: Report<'_>) -> Result<Outcome, Error> {
    if !report.step(Step::Starting(path)) {
        return Ok(Outcome::Cancelled);
    }
    let removed = attempt(path, report, |_| if is_dir { remove_dir(path) } else { remove_entry(path) })?;
    report.step(Step::Advanced(1));
    Ok(if removed.is_some() { Outcome::Done } else { Outcome::Skipped })
}

/// Remove anything that is not a directory, links included. Windows keeps a
/// link to a directory - a directory symlink or a junction - as a directory,
/// which remove_file refuses; remove_dir takes the link away, and never what
/// it points at.
#[cfg(windows)]
fn remove_entry(path: &Path) -> Result<(), Error> {
    use std::os::windows::fs::FileTypeExt;
    if path.symlink_metadata().is_ok_and(|metadata| metadata.file_type().is_symlink_dir()) {
        remove_dir(path)
    } else {
        remove_file(path)
    }
}

/// Elsewhere every link is a file to unlink, whatever it points at.
#[cfg(not(windows))]
fn remove_entry(path: &Path) -> Result<(), Error> {
    remove_file(path)
}

fn delete_dir_recursive(path: &Path, report: Report<'_>) -> Result<Outcome, Error> {
    let Some(entries) = attempt(path, report, |_| read_dir(path))? else {
        return Ok(Outcome::Skipped);
    };
    let mut skipped = false;
    for entry in entries {
        let Some((entry, file_type)) = next_entry(path, entry, report)? else {
            skipped = true;
            continue;
        };
        let entry_path = entry.path();

        // file_type() describes the entry itself, so a link to a directory is a
        // link here and is unlinked rather than followed into.
        let outcome = if file_type.is_dir() { delete_dir_recursive(&entry_path, &mut *report)? } else { remove_one(&entry_path, false, &mut *report)? };
        match outcome {
            Outcome::Cancelled => return Ok(Outcome::Cancelled),
            Outcome::Skipped => skipped = true,
            Outcome::Done => {}
        }
    }

    // Something inside was left where it was, so the directory has to stay
    // too. Trying would only ask about it again, as "Directory not empty".
    if skipped {
        return Ok(Outcome::Skipped);
    }
    // The directory itself, now that it is empty.
    remove_one(path, true, report)
}
