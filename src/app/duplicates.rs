//! Alt+D: the files that are the same as another. Grouped by size first,
//! which costs nothing, and only those of a size shared read and compared
//! by checksum. The groups go to the panel as Find files feeds it, every one
//! of a group selected but the first, ready for F8.

use super::*;
use crate::checksum::{Algorithm, hash_file};

/// What a search for duplicates found: under `base`, each group of files
/// with the same content, the one to keep first.
#[derive(Debug)]
pub struct Duplicates {
    pub base: PathBuf,
    pub groups: Vec<Vec<PathBuf>>,
}

impl AppState {
    /// Alt+D: what is selected, or with nothing selected the whole of the
    /// active panel's directory, searched for duplicates as a job.
    pub fn find_duplicates(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let panel = self.panel(side);
        let base = self.panel_dir(side);
        let sources: Vec<PathBuf> = if panel.selected.is_empty() {
            vec![base.clone()]
        } else {
            panel.children.iter().filter(|item| panel.selected.contains(&item.name_os)).map(|item| item.path_in(&panel.dir)).collect()
        };
        self.start_job(TransferKind::Duplicates, Clashes::Ask, move |progress| run_duplicates(sources, base, progress));
    }

    /// The groups found, in the active panel, each but its first selected.
    pub(super) fn feed_duplicates(&mut self, found: Duplicates) {
        let Duplicates { base, groups } = found;
        let relative = |path: &PathBuf| path.strip_prefix(&base).map(Path::to_path_buf).ok();
        let feed: Vec<(PathBuf, bool)> = groups.iter().flatten().filter_map(relative).map(|path| (path, false)).collect();
        let Some((first, _)) = feed.first() else {
            self.find_note = Some(t!("No duplicates found").to_string());
            return;
        };
        let first = first.clone().into_os_string();
        let side = self.active_side;
        let panel = self.panel_mut(side);
        panel.flat = Some(Flat {
            dir: base.clone(),
            feed: Some(Arc::new(feed)),
            kind: FeedKind::Found,
        });
        // Before the listing, which keeps what it finds of them.
        panel.selected = groups.iter().flat_map(|group| group.iter().skip(1)).filter_map(relative).map(PathBuf::into_os_string).collect();
        if self.panel_dir(side) == base {
            self.reload_panel(side, Some(&first));
        } else {
            self.open_dir(side, base, Some(&first));
        }
    }
}

/// True for the first name met of a file - a file with more than one, by
/// hard links, is there once, and deleting one name would free nothing.
#[cfg(unix)]
pub(super) fn first_name(metadata: &std::fs::Metadata, seen: &mut HashSet<(u64, u64)>) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() < 2 || seen.insert((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
pub(super) fn first_name(_metadata: &std::fs::Metadata, _seen: &mut HashSet<(u64, u64)>) -> bool {
    true
}

/// Of files by their size, those of a size another has too: only they can
/// be the same as another.
pub(super) fn shared_sizes(by_size: HashMap<u64, Vec<PathBuf>>) -> Vec<(u64, Vec<PathBuf>)> {
    by_size.into_iter().filter(|(_, paths)| paths.len() > 1).collect()
}

/// What reading all of `sizes` comes to, for the bar.
pub(super) fn bytes_to_read(sizes: &[(u64, Vec<PathBuf>)]) -> u64 {
    sizes.iter().map(|(size, paths)| size.saturating_mul(paths.len() as u64)).fold(0, u64::saturating_add)
}

/// The files among `sizes` with the same content as another, in groups, by
/// checksum. None when the job was stopped. One that cannot be read is
/// left out, and counted as skipped.
pub(super) fn same_content(sizes: Vec<(u64, Vec<PathBuf>)>, progress: &mut JobProgress<'_>) -> Option<Vec<Vec<PathBuf>>> {
    let mut groups = Vec::new();
    for (_, paths) in sizes {
        let mut by_sum: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in paths {
            if !progress.step(Step::Starting(&path)) {
                return None;
            }
            match hash_file(&path, Algorithm::Sha256, progress) {
                Ok(Some(sum)) => by_sum.entry(sum).or_default().push(path),
                Ok(None) => return None,
                Err(_) => {
                    let _ = progress.updates.send(JobUpdate::Skipped);
                }
            }
        }
        groups.extend(by_sum.into_values().filter(|group| group.len() > 1));
    }
    Some(groups)
}

/// The worker thread behind Alt+D. The walk is counted in nothing, and then
/// the reading in the bytes of every file of a size another has too. One
/// that cannot be read is left out, and said to be at the end.
fn run_duplicates(sources: Vec<PathBuf>, base: PathBuf, mut progress: JobProgress<'_>) {
    // Every regular file by its size. Links are left out, as their own
    // files: deleting what one points at would leave it pointing at nothing.
    // Empty files are all the same and take no room.
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
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
        } else if metadata.is_file() && metadata.len() > 0 && first_name(&metadata, &mut seen) {
            by_size.entry(metadata.len()).or_default().push(path);
        }
    }

    let sizes = shared_sizes(by_size);
    let _ = progress.updates.send(JobUpdate::Total(bytes_to_read(&sizes)));
    let Some(mut groups) = same_content(sizes, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    // The one kept is the one nearest the top, and of those the first by
    // name; the groups in the order of their first.
    for group in &mut groups {
        group.sort_by(|a, b| a.components().count().cmp(&b.components().count()).then_with(|| a.cmp(b)));
    }
    groups.sort();
    let _ = progress.updates.send(JobUpdate::Duplicates(Duplicates { base, groups }));
    progress.finish(Ok(Transfer::Done));
}
