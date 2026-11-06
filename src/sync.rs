//! Alt+S: the two panels' directories compared all the way down, for a plan
//! of what to copy which way and what to delete. Walked on a thread of its
//! own, as Find files is, so a large tree or a slow disk holds up nothing.

use crate::app::Side;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Modified times this close count as the same, as for Shift+F2: FAT keeps
/// them only to two seconds.
const SAME_TIME: Duration = Duration::from_secs(2);

/// An entry on one side, as it was when compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// A path, from both directories, whose entries differ: on one side only,
/// one newer than the other, or the same age and not the same size, or a
/// file on one side and a directory on the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub path: PathBuf,
    pub left: Option<Entry>,
    pub right: Option<Entry>,
}

impl Difference {
    /// Which side is newer, of two files: None when neither is - the same
    /// age, or no age to go by.
    pub fn newer(&self) -> Option<Side> {
        let (Some(left), Some(right)) = (self.left, self.right) else {
            return None;
        };
        // By the gap between them, which a time at the clock's end has too;
        // adding to one there would run past it.
        let later = |mine: SystemTime, theirs: SystemTime| mine.duration_since(theirs).is_ok_and(|gap| gap > SAME_TIME);
        match (left.modified, right.modified) {
            (Some(mine), Some(theirs)) if later(mine, theirs) => Some(Side::Left),
            (Some(mine), Some(theirs)) if later(theirs, mine) => Some(Side::Right),
            _ => None,
        }
    }
}

/// What the walk reports as it goes.
pub enum Update {
    /// Comparing in this directory, by its path from both. Sent only now and
    /// then.
    Looking(PathBuf),
    Found(Difference),
    /// Over: how many directories could not be read on either side, which
    /// are counted and passed over.
    Finished {
        unreadable: usize,
    },
}

/// Compare `left` and `right` all the way down, sending each difference as
/// it is found. A directory on one side only is one difference, not gone
/// into; one on both is gone into, and is not one itself. Links are
/// compared as the entries they are, never followed: one pointing back up
/// the tree would never end. Hidden entries are left out unless the panels
/// show them. Stops when `cancel` is set or `send` returns false.
pub fn compare(left: &Path, right: &Path, show_hidden: bool, cancel: &AtomicBool, send: &mut dyn FnMut(Update) -> bool) {
    let mut unreadable = 0;
    let mut reported = Instant::now();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if reported.elapsed() >= crate::constants::SIZE_PROGRESS_INTERVAL {
            reported = Instant::now();
            if !send(Update::Looking(relative.clone())) {
                return;
            }
        }
        let (Some(mine), Some(theirs)) = (entries(&left.join(&relative), show_hidden), entries(&right.join(&relative), show_hidden)) else {
            unreadable += 1;
            continue;
        };
        let mut names: Vec<&OsString> = mine.keys().chain(theirs.keys().filter(|name| !mine.contains_key(*name))).collect();
        names.sort();
        // Gone into in name order: the last pushed is the first popped.
        let mut below = Vec::new();
        for name in names {
            let (left, right) = (mine.get(name).copied(), theirs.get(name).copied());
            let path = relative.join(name);
            if let (Some(l), Some(r)) = (left, right) {
                if l.is_dir && r.is_dir {
                    below.push(path);
                    continue;
                }
                if !l.is_dir && !r.is_dir && same(l, r) {
                    continue;
                }
            }
            if !send(Update::Found(Difference { path, left, right })) {
                return;
            }
        }
        stack.extend(below.into_iter().rev());
    }
    send(Update::Finished { unreadable });
}

/// Two files alike: the same size, and the same age or no age to tell by.
fn same(left: Entry, right: Entry) -> bool {
    left.size == right.size
        && match (left.modified, right.modified) {
            (Some(mine), Some(theirs)) => mine.max(theirs).duration_since(mine.min(theirs)).is_ok_and(|gap| gap <= SAME_TIME),
            _ => true,
        }
}

/// A directory's entries by name, as the entries themselves - a link as a
/// link. None when it cannot be read.
fn entries(dir: &Path, show_hidden: bool) -> Option<BTreeMap<OsString, Entry>> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        let name = entry.file_name();
        if !show_hidden && crate::fs_ops::is_hidden(&name.to_string_lossy(), Some(&metadata)) {
            continue;
        }
        let is_dir = metadata.is_dir();
        found.insert(
            name,
            Entry {
                is_dir,
                size: if is_dir { 0 } else { metadata.len() },
                modified: metadata.modified().ok(),
            },
        );
    }
    Some(found)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn run(left: &Path, right: &Path) -> (Vec<Difference>, usize) {
        let mut found = Vec::new();
        let mut unreadable = None;
        compare(left, right, false, &AtomicBool::new(false), &mut |update| {
            match update {
                Update::Found(difference) => found.push(difference),
                Update::Finished { unreadable: count } => unreadable = Some(count),
                Update::Looking(_) => {}
            }
            true
        });
        (found, unreadable.expect("the walk never said it was over"))
    }

    fn write(path: &Path, body: &str, age: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
    }

    #[test]
    fn differences_all_the_way_down_and_nothing_alike() {
        let root = std::env::temp_dir().join(format!("fm84-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (left, right) = (root.join("left"), root.join("right"));
        // Alike, a second apart: FAT's two seconds.
        write(&left.join("same.txt"), "same", 100);
        write(&right.join("same.txt"), "same", 101);
        write(&left.join("newer-left.txt"), "new", 10);
        write(&right.join("newer-left.txt"), "old", 1000);
        write(&left.join("sub/deep/newer-right.txt"), "old", 1000);
        write(&right.join("sub/deep/newer-right.txt"), "new!", 10);
        write(&left.join("sub/only-left.txt"), "", 10);
        write(&right.join("only-right/inside.txt"), "", 10);
        write(&left.join("sizes.txt"), "four", 50);
        write(&right.join("sizes.txt"), "three", 50);
        write(&left.join("clash"), "", 10);
        fs::create_dir_all(right.join("clash")).unwrap();
        write(&left.join(".hidden"), "", 10);
        fs::create_dir_all(left.join("both-empty")).unwrap();
        fs::create_dir_all(right.join("both-empty")).unwrap();

        let (found, unreadable) = run(&left, &right);
        assert_eq!(unreadable, 0);
        let paths: Vec<String> = found.iter().map(|difference| difference.path.to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(paths, ["clash", "newer-left.txt", "only-right", "sizes.txt", "sub/only-left.txt", "sub/deep/newer-right.txt"]);
        let by = |path: &str| found.iter().find(|difference| difference.path == Path::new(path)).unwrap();
        assert_eq!(by("newer-left.txt").newer(), Some(Side::Left));
        assert_eq!(by("sub/deep/newer-right.txt").newer(), Some(Side::Right));
        assert_eq!(by("sizes.txt").newer(), None);
        assert!(by("only-right").left.is_none() && by("only-right").right.is_some_and(|side| side.is_dir));
        assert!(by("clash").left.is_some_and(|side| !side.is_dir) && by("clash").right.is_some_and(|side| side.is_dir));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cancelled_compare_stops() {
        let mut updates = 0;
        compare(Path::new("/"), Path::new("/"), false, &AtomicBool::new(true), &mut |_| {
            updates += 1;
            true
        });
        assert_eq!(updates, 0);
    }
    /// The last second the system clock holds, for times at its very end.
    pub(crate) fn clock_end() -> SystemTime {
        let (mut low, mut high) = (0u64, u64::MAX);
        while low < high {
            let mid = low + (high - low).div_ceil(2);
            if SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(mid)).is_some() {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        SystemTime::UNIX_EPOCH + Duration::from_secs(low)
    }

    /// A file dated at the end of what the clock holds is compared as any
    /// other, not by adding to a time that cannot go further.
    #[test]
    fn times_at_the_clocks_end_still_compare() {
        let far = clock_end();
        let entry = |modified| Entry {
            is_dir: false,
            size: 1,
            modified: Some(modified),
        };
        let (near, late) = (entry(far - Duration::from_secs(10)), entry(far));
        let difference = |left, right| Difference {
            path: "f".into(),
            left: Some(left),
            right: Some(right),
        };
        assert_eq!(difference(near, late).newer(), Some(Side::Right));
        assert_eq!(difference(late, near).newer(), Some(Side::Left));
        assert!(same(late, late) && !same(near, late));
    }
}
