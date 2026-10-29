//! Copying out of an archive - F5 from a panel inside one, and an entry
//! opened to be viewed - asked about as a copy is: what is in the way, and
//! what fails, an entry at a time.
//!
//! Nothing is written outside a destination. A name that would reach out
//! never got into the index; a link is recreated as a link, never followed,
//! and nothing in the archive lies under one.

use super::transfer::{Outcome, settle_clash, write_beside};
use super::*;
use crate::archive::{EntryKind, Next};
use std::collections::HashMap;

/// Bytes extracting the entries at `inners` will write, for the bar.
pub fn measure_extract(archive: &Path, inners: &[PathBuf]) -> u64 {
    crate::archive::index(archive).map(|index| inners.iter().map(|inner| index.size_of(inner)).sum()).unwrap_or(0)
}

/// Copy `items` out of `archive`: each the path of an entry inside it and
/// where that goes. A directory goes whole, merged into one already there as
/// a copy merges.
pub fn extract(archive: &Path, items: &[(PathBuf, PathBuf)], report: Report<'_>) -> Result<Transfer, Error> {
    let index = crate::archive::index(archive)?;
    let source = |inner: &Path| archive.join(inner);

    // Every entry with where it goes, parents before what they hold.
    let mut plan: Vec<(usize, PathBuf)> = Vec::new();
    for (inner, dest) in items {
        for at in index.subtree(inner) {
            let below = index.entry(at).path.strip_prefix(inner).unwrap_or(Path::new("")).to_path_buf();
            plan.push((at, if below.as_os_str().is_empty() { dest.clone() } else { dest.join(below) }));
        }
    }
    // Where each entry went, for a hard link to find the one it names.
    let placed: HashMap<PathBuf, PathBuf> = plan.iter().map(|(at, target)| (index.entry(*at).path.clone(), target.clone())).collect();

    // Left where they were, with everything under them.
    let mut skipped_dirs: Vec<PathBuf> = Vec::new();
    let passed_over = |skipped: &[PathBuf], target: &Path| skipped.iter().any(|dir| target.starts_with(dir));
    let mut cancelled = false;

    // Directories first, so everything has somewhere to go.
    for (at, target) in &plan {
        if index.entry(*at).kind != EntryKind::Dir || passed_over(&skipped_dirs, target) {
            continue;
        }
        if attempt(&source(&index.entry(*at).path), report, |_| fs::create_dir_all(target))?.is_none() {
            skipped_dirs.push(target.clone());
        }
    }

    // Links whose target the index holds - a tar's - need nothing read.
    for (at, target) in &plan {
        let entry = index.entry(*at);
        if entry.kind != EntryKind::Link || passed_over(&skipped_dirs, target) {
            continue;
        }
        let Some(points_to) = &entry.link else {
            continue;
        };
        let target = match settle_clash(&source(&entry.path), target, report) {
            Ok(target) => target,
            Err(Outcome::Cancelled) => return Ok(Transfer::Cancelled),
            Err(_) => continue,
        };
        attempt(&source(&entry.path), report, |_| {
            clear_for_link(&target)?;
            make_link(points_to, &target)
        })?;
    }

    // What has to be read: files, and a zip's links, which keep their target
    // as their contents.
    let mut reading: HashMap<usize, (usize, PathBuf)> = HashMap::new();
    for (at, target) in &plan {
        let entry = index.entry(*at);
        let read = entry.kind == EntryKind::File || (entry.kind == EntryKind::Link && entry.link.is_none());
        if let (true, Some(position)) = (read, entry.position)
            && !passed_over(&skipped_dirs, target)
        {
            reading.insert(position, (*at, target.clone()));
        }
    }
    let mut positions: Vec<usize> = reading.keys().copied().collect();
    positions.sort_unstable();
    let mut aborted: Option<Error> = None;
    crate::archive::read_entries(archive, index.kind, &positions, &mut |position, contents| {
        let (at, target) = &reading[&position];
        let entry = index.entry(*at);
        let from = source(&entry.path);
        let mut target = target.clone();
        if contents.is_ok() {
            match settle_clash(&from, &target, &mut *report) {
                Ok(chosen) => target = chosen,
                Err(Outcome::Cancelled) => {
                    cancelled = true;
                    return Next::Stop;
                }
                Err(_) => {
                    // Counted as dealt with, so the bar still reaches the end.
                    report.step(Step::Advanced(entry.size));
                    return Next::Go;
                }
            }
        }
        let written = contents.and_then(|reader| {
            if !report.step(Step::Starting(&from)) {
                return Ok(Transfer::Cancelled);
            }
            if entry.kind == EntryKind::Link {
                let mut points_to = Vec::new();
                reader.take(4096).read_to_end(&mut points_to)?;
                clear_for_link(&target)?;
                make_link(&crate::archive::link_target(&points_to), &target)?;
                return Ok(Transfer::Done);
            }
            write_beside(&target, reader, &mut *report, &|file, temp| finish_file(file, temp, entry))
        });
        match written {
            Ok(Transfer::Done) => Next::Go,
            Ok(Transfer::Cancelled) => {
                cancelled = true;
                Next::Stop
            }
            Err(error) => match report.failed(&from, &error) {
                Reply::Retry => Next::Again,
                Reply::Skip | Reply::Delete => Next::Go,
                Reply::Abort => {
                    aborted = Some(error);
                    Next::Stop
                }
            },
        }
    })?;
    if let Some(error) = aborted {
        return Err(error);
    }
    if cancelled {
        return Ok(Transfer::Cancelled);
    }

    // Hard links, once what they name is there, and what cannot be made.
    for (at, target) in &plan {
        let entry = index.entry(*at);
        if passed_over(&skipped_dirs, target) {
            continue;
        }
        let from = source(&entry.path);
        match entry.kind {
            EntryKind::HardLink => {
                let target = match settle_clash(&from, target, report) {
                    Ok(target) => target,
                    Err(Outcome::Cancelled) => return Ok(Transfer::Cancelled),
                    Err(_) => continue,
                };
                let original = entry.link.as_ref().and_then(|named| placed.get(named));
                attempt(&from, report, |_| -> Result<(), Error> {
                    match original {
                        Some(original) if path_exists(original) => {
                            clear_for_link(&target)?;
                            fs::hard_link(original, &target)
                        }
                        _ => Err(Error::other(format!("A hard link to {}, which is not being copied out", entry.link.as_deref().unwrap_or(Path::new("?")).display()))),
                    }
                })?;
            }
            EntryKind::Other => {
                attempt(&from, report, |_| -> Result<(), Error> { Err(not_regular(&from)) })?;
            }
            _ => {}
        }
    }

    // Last, once everything is inside: a directory given its time first would
    // have it moved on by every file written into it, and a read-only one
    // would refuse them. Deepest first.
    for (at, target) in plan.iter().rev() {
        let entry = index.entry(*at);
        if entry.kind == EntryKind::Dir && entry.position.is_some() && !passed_over(&skipped_dirs, target) {
            set_dir_metadata(target, entry);
        }
    }
    Ok(Transfer::Done)
}

/// A file's time and mode, as the archive keeps them, before it takes its
/// place. Allowed to fail, as for a copy. Only the permission bits: a setuid
/// bit from an archive is not something to give a file without being asked.
fn finish_file(file: &File, temp: &Path, entry: &crate::archive::Entry) {
    if let Some(modified) = entry.modified {
        let _ = file.set_times(fs::FileTimes::new().set_modified(modified));
    }
    set_mode(temp, entry.mode);
}

fn set_dir_metadata(dir: &Path, entry: &crate::archive::Entry) {
    if let Some(modified) = entry.modified
        && let Ok(handle) = File::open(dir)
    {
        let _ = handle.set_times(fs::FileTimes::new().set_modified(modified));
    }
    set_mode(dir, entry.mode);
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) {}

#[cfg(unix)]
fn make_link(points_to: &Path, link: &Path) -> Result<(), Error> {
    std::os::unix::fs::symlink(points_to, link)
}

/// Windows needs to know which kind of link to make, and the target may not
/// be out yet to ask; a file link is the usual case.
#[cfg(windows)]
fn make_link(points_to: &Path, link: &Path) -> Result<(), Error> {
    std::os::windows::fs::symlink_file(points_to, link)
}

#[cfg(not(any(unix, windows)))]
fn make_link(_points_to: &Path, _link: &Path) -> Result<(), Error> {
    Err(Error::new(ErrorKind::Unsupported, "Symbolic links are not supported here"))
}

#[cfg(test)]
mod tests;
