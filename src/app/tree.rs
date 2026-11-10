//! Alt+F10: a panel turned into the tree of its drive's directories, as
//! ncd drew it on DOS - opened down to where the panel is, each directory
//! opened and closed with Right and Left. Enter sends the other panel to the
//! directory under the cursor, and the tree stays; Esc or Alt+F10 again
//! puts the panel back to listing files, in that directory.
//!
//! Each directory's own are read the first time it is opened, on a thread
//! waited for only as long as a listing is: a mount that does not answer
//! shows as being read, and holds nothing else up.

use super::*;
use std::ffi::OsString;
use std::io;

/// How far a directory in the tree has been read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeMark {
    /// Not read yet, so whether it has any of its own is not known.
    Unread,
    /// Being read.
    Reading,
    /// Read, with directories of its own: open, or not.
    Open,
    Closed,
    /// Read, with none - or not readable.
    Empty,
}

/// The most directories `*` reads.
pub const TREE_OPEN_MAX: usize = 5000;

/// What `*` read, for the tree of `side`: each directory and those in it;
/// `capped` when it stopped at TREE_OPEN_MAX.
#[derive(Debug)]
pub struct TreeFound {
    pub side: Side,
    pub read: Vec<(PathBuf, Vec<OsString>)>,
    pub capped: bool,
}

/// The filesystem a path is on, to keep `*` on the one it started on.
#[cfg(unix)]
fn device(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(not(unix))]
fn device(_path: &Path) -> Option<u64> {
    None
}

/// The worker thread behind `*`: every directory under `start`, counted as
/// it goes, what it read sent whether it ended or was stopped.
fn run_open_tree(side: Side, start: PathBuf, options: crate::options::Options, mut progress: JobProgress<'_>) {
    let disk = device(&start);
    let mut read = Vec::new();
    let mut stack = vec![start];
    let (mut capped, mut stopped) = (false, false);
    while let Some(dir) = stack.pop() {
        if read.len() >= TREE_OPEN_MAX {
            capped = true;
            break;
        }
        if !progress.step(Step::Starting(&dir)) {
            stopped = true;
            break;
        }
        let dirs = crate::fs_ops::list_subdirs(&dir, &options).unwrap_or_default();
        for name in &dirs {
            let below = dir.join(name);
            let link = std::fs::symlink_metadata(&below).is_ok_and(|metadata| metadata.file_type().is_symlink());
            if !link && device(&below) == disk {
                stack.push(below);
            }
        }
        read.push((dir, dirs));
        progress.step(Step::Advanced(1));
    }
    let _ = progress.updates.send(JobUpdate::Tree(TreeFound { side, read, capped }));
    progress.finish(Ok(if stopped { Transfer::Cancelled } else { Transfer::Done }));
}

/// One row of the tree as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    /// For each level down to it, whether the directory at that level is
    /// the last of its parent's - which says where the lines run. Empty for
    /// the root.
    pub last: Vec<bool>,
    pub mark: TreeMark,
}

/// A panel's tree.
pub struct Tree {
    pub root: PathBuf,
    /// Each directory read so far, and the directories in it by name.
    children: HashMap<PathBuf, Vec<OsString>>,
    /// Directories still being read.
    pending: HashMap<PathBuf, Receiver<io::Result<Vec<OsString>>>>,
    /// What is open, being read again after a job or Ctrl+R: all of it on
    /// one thread, each taken in as it comes, what was there shown till then.
    batch: Option<Receiver<(PathBuf, Vec<OsString>)>>,
    /// Those opened.
    expanded: HashSet<PathBuf>,
    /// What is on show, top to bottom, rebuilt whenever that changes.
    pub rows: Vec<TreeRow>,
    pub cursor: usize,
}

impl Tree {
    fn mark(&self, path: &Path) -> TreeMark {
        match self.children.get(path) {
            _ if self.pending.contains_key(path) && !self.children.contains_key(path) => TreeMark::Reading,
            None => TreeMark::Unread,
            Some(dirs) if dirs.is_empty() => TreeMark::Empty,
            Some(_) if self.expanded.contains(path) => TreeMark::Open,
            Some(_) => TreeMark::Closed,
        }
    }

    /// The rows again, from the root down through what is open, the cursor
    /// kept on the directory it was on - or, gone from sight, on the nearest
    /// above it that is still there.
    fn rebuild(&mut self) {
        let at = self.rows.get(self.cursor).map(|row| row.path.clone());
        let mut rows = Vec::new();
        let mut stack = vec![(self.root.clone(), Vec::new())];
        while let Some((path, last)) = stack.pop() {
            let mark = self.mark(&path);
            if mark == TreeMark::Open
                && let Some(dirs) = self.children.get(&path)
            {
                // Pushed last first, to come off the stack in order.
                for (index, name) in dirs.iter().enumerate().rev() {
                    let mut below = last.clone();
                    below.push(index + 1 == dirs.len());
                    stack.push((path.join(name), below));
                }
            }
            rows.push(TreeRow { path, last, mark });
        }
        self.rows = rows;
        self.cursor = at.and_then(|at| at.ancestors().find_map(|above| self.rows.iter().position(|row| row.path == above))).unwrap_or(0).min(self.rows.len().saturating_sub(1));
    }

    /// Take in what has been read since, the rows rebuilt when anything was.
    /// One that failed is taken to hold nothing.
    fn poll(&mut self) -> bool {
        let mut done: Vec<(PathBuf, Vec<OsString>)> = Vec::new();
        if let Some(batch) = &self.batch {
            loop {
                match batch.try_recv() {
                    Ok(read) => done.push(read),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.batch = None;
                        break;
                    }
                }
            }
        }
        let read: Vec<(PathBuf, Vec<OsString>)> = self
            .pending
            .iter()
            .filter_map(|(path, answer)| match answer.try_recv() {
                Ok(read) => Some((path.clone(), read.unwrap_or_default())),
                Err(mpsc::TryRecvError::Disconnected) => Some((path.clone(), Vec::new())),
                Err(mpsc::TryRecvError::Empty) => None,
            })
            .collect();
        for (path, _) in &read {
            self.pending.remove(path);
        }
        done.extend(read);
        for (path, dirs) in &done {
            self.children.insert(path.clone(), dirs.clone());
        }
        if !done.is_empty() {
            self.rebuild();
        }
        !done.is_empty()
    }
}

impl AppState {
    /// Read `dir`'s directories for `side`'s tree, waiting for them as long
    /// as for a listing. One already being read is left to come.
    fn read_tree_dir(&mut self, side: Side, dir: PathBuf) {
        let (wait, options) = (self.workers.listing_wait, self.options.clone());
        let Some(tree) = self.panel_mut(side).tree.as_mut() else { return };
        if tree.pending.contains_key(&dir) {
            return;
        }
        let (sender, answer) = mpsc::channel();
        let read = dir.clone();
        std::thread::spawn(move || {
            let _ = sender.send(crate::fs_ops::list_subdirs(&read, &options));
        });
        match answer.recv_timeout(wait) {
            Ok(dirs) => {
                tree.children.insert(dir, dirs.unwrap_or_default());
            }
            Err(_) => {
                tree.pending.insert(dir, answer);
            }
        }
    }

    /// Alt+F10: the active panel as the tree of its drive, opened down to
    /// its directory with the cursor there - or, as the tree already, back
    /// to listing files, in the directory under the cursor.
    pub fn toggle_tree(&mut self) {
        let side = self.active_side;
        if let Some(tree) = self.panel_mut(side).tree.take() {
            self.search_input.clear();
            if let Some(row) = tree.rows.get(tree.cursor) {
                self.open_dir(side, row.path.clone(), None);
            }
            return;
        }
        // Inside an archive, the tree is of the disk around it, and the
        // panel goes out there too: what the tree's F-keys do is to the disk.
        let dir = self.panel_real_dir(side);
        if self.panel_archive(side).is_some() {
            self.open_dir(side, dir.clone(), None);
        }
        let root = dir.ancestors().last().unwrap_or(&dir).to_path_buf();
        let chain: Vec<PathBuf> = dir.ancestors().map(Path::to_path_buf).collect::<Vec<_>>().into_iter().rev().collect();
        self.search_input.clear();
        self.panel_mut(side).tree = Some(Tree {
            root,
            children: HashMap::new(),
            pending: HashMap::new(),
            batch: None,
            expanded: chain.iter().cloned().collect(),
            rows: Vec::new(),
            cursor: 0,
        });
        for dir in chain.iter().cloned() {
            self.read_tree_dir(side, dir);
        }
        if let Some(tree) = self.panel_mut(side).tree.as_mut() {
            tree.rebuild();
            tree.cursor = tree.rows.iter().position(|row| row.path == dir).unwrap_or(0);
        }
    }

    /// Take in what the trees have read since the last frame.
    pub fn poll_trees(&mut self) {
        for side in Side::BOTH {
            if let Some(tree) = self.panel_mut(side).tree.as_mut() {
                tree.poll();
            }
        }
    }

    /// Read again what `side`'s tree has open - after a job, or Ctrl+R - on
    /// a thread of its own, keeping what it showed until each comes back.
    /// Nothing waits for it: after `*` that can be thousands.
    pub fn reread_tree(&mut self, side: Side) {
        let options = self.options.clone();
        let Some(tree) = self.panel_mut(side).tree.as_mut() else { return };
        let open: Vec<PathBuf> = tree.rows.iter().filter(|row| row.mark == TreeMark::Open).map(|row| row.path.clone()).collect();
        let (sender, answer) = mpsc::channel();
        std::thread::spawn(move || {
            for dir in open {
                let read = crate::fs_ops::list_subdirs(&dir, &options).unwrap_or_default();
                if sender.send((dir, read)).is_err() {
                    return;
                }
            }
        });
        tree.batch = Some(answer);
    }

    /// A click on the row `index` of `side`'s tree: the cursor there, the
    /// panel the active one - and on the row's mark, the directory opened or
    /// closed; twice, Enter.
    pub fn tree_click(&mut self, side: Side, index: usize, on_mark: bool, double: bool) {
        self.active_side = side;
        self.search_input.clear();
        let Some(tree) = self.tree_mut() else { return };
        if index >= tree.rows.len() {
            return;
        }
        tree.cursor = index;
        if double {
            return self.tree_enter();
        }
        if on_mark {
            match tree.rows[index].mark {
                TreeMark::Open => self.tree_collapse(),
                TreeMark::Closed | TreeMark::Unread => self.tree_expand(),
                TreeMark::Reading | TreeMark::Empty => {}
            }
        }
    }

    /// A row's name, as the quick search goes by it: the root by its path.
    fn row_name(row: &TreeRow) -> String {
        match row.path.file_name() {
            Some(name) if !row.last.is_empty() => name.to_string_lossy().to_lowercase(),
            _ => row.path.to_string_lossy().to_lowercase(),
        }
    }

    /// The next row on show from the cursor - itself first when `from_here`
    /// - whose name starts with what is typed, round the end and back.
    fn tree_match(&self, forward: bool, from_here: bool) -> Option<usize> {
        let tree = self.panel(self.active_side).tree.as_ref()?;
        let typed = self.search_input.to_lowercase();
        let count = tree.rows.len();
        if count == 0 {
            return None;
        }
        let skip = usize::from(!from_here);
        (0..count)
            .map(|step| {
                let step = step + skip;
                if forward { (tree.cursor + step) % count } else { (tree.cursor + count * 2 - step) % count }
            })
            .find(|&index| Self::row_name(&tree.rows[index]).starts_with(&typed))
    }

    /// A letter typed in the tree: the cursor to the first directory on show
    /// from here whose name starts with all that has been typed. One that
    /// none does is not taken.
    pub fn tree_search_add(&mut self, c: char) {
        self.search_input.push(c);
        match self.tree_match(true, true) {
            Some(index) => {
                if let Some(tree) = self.tree_mut() {
                    tree.cursor = index;
                }
            }
            None => {
                self.search_input.pop();
            }
        }
    }

    pub fn tree_search_back(&mut self) {
        self.search_input.pop();
    }

    /// Down or Up while typing: the next directory on show that matches,
    /// that way.
    pub fn tree_search_next(&mut self, forward: bool) {
        if let Some(index) = self.tree_match(forward, false)
            && let Some(tree) = self.tree_mut()
        {
            tree.cursor = index;
        }
    }

    /// `*`: everything under the directory under the cursor read and opened,
    /// as a job - Esc stops it, and what it got as far as is kept. Links to
    /// directories are not gone through, nor other filesystems mounted
    /// under it, and it stops at TREE_OPEN_MAX: under `/` it would read the
    /// whole disk, and every network mount on it.
    pub fn tree_open_all(&mut self) {
        let side = self.active_side;
        let Some(start) = self.panel(side).tree.as_ref().and_then(|tree| tree.rows.get(tree.cursor)).map(|row| row.path.clone()) else {
            return;
        };
        let options = self.options.clone();
        self.start_job(TransferKind::Tree, Clashes::Ask, move |progress| run_open_tree(side, start, options, progress));
    }

    /// What `*` read, taken into `side`'s tree: each directory with some of
    /// its own opened.
    pub(super) fn take_tree(&mut self, found: TreeFound) {
        let TreeFound { side, read, capped } = found;
        if let Some(tree) = self.panel_mut(side).tree.as_mut() {
            for (dir, dirs) in read {
                if !dirs.is_empty() {
                    tree.expanded.insert(dir.clone());
                }
                tree.children.insert(dir, dirs);
            }
            tree.rebuild();
        }
        if capped {
            self.find_note = Some(t!("Opened the first {count} directories", count = TREE_OPEN_MAX));
        }
    }

    /// The directory under `side`'s tree's cursor, when it shows its tree.
    pub fn tree_cursor(&self, side: Side) -> Option<PathBuf> {
        let tree = self.panel(side).tree.as_ref()?;
        tree.rows.get(tree.cursor).map(|row| row.path.clone())
    }

    /// `path`, just made, shown in the active panel's tree: the directory it
    /// is in read again and opened, and the cursor on it.
    pub fn tree_reveal(&mut self, path: PathBuf) {
        let side = self.active_side;
        let Some(parent) = path.parent().map(Path::to_path_buf) else { return };
        // Read afresh, not left to an older read still on its way.
        if let Some(tree) = self.tree_mut() {
            tree.pending.remove(&parent);
            tree.expanded.insert(parent.clone());
        }
        self.read_tree_dir(side, parent);
        if let Some(tree) = self.tree_mut() {
            tree.rebuild();
            if let Some(index) = tree.rows.iter().position(|row| row.path == path) {
                tree.cursor = index;
            }
        }
    }

    fn tree_mut(&mut self) -> Option<&mut Tree> {
        let side = self.active_side;
        self.panel_mut(side).tree.as_mut()
    }

    /// Move the cursor by `step` rows, stopping at either end.
    pub fn tree_move(&mut self, step: isize) {
        if let Some(tree) = self.tree_mut() {
            tree.cursor = tree.cursor.saturating_add_signed(step).min(tree.rows.len().saturating_sub(1));
        }
    }

    /// Right: the directory under the cursor opened, read first if it has
    /// not been - or, open already, the cursor down into it.
    pub fn tree_expand(&mut self) {
        let side = self.active_side;
        let Some(tree) = self.tree_mut() else { return };
        let Some(row) = tree.rows.get(tree.cursor).cloned() else { return };
        match row.mark {
            TreeMark::Open => tree.cursor = (tree.cursor + 1).min(tree.rows.len() - 1),
            TreeMark::Empty | TreeMark::Reading => {}
            TreeMark::Closed => {
                tree.expanded.insert(row.path);
                tree.rebuild();
            }
            TreeMark::Unread => {
                tree.expanded.insert(row.path.clone());
                self.read_tree_dir(side, row.path);
                if let Some(tree) = self.tree_mut() {
                    tree.rebuild();
                }
            }
        }
    }

    /// Left: the directory under the cursor closed - or, closed already, the
    /// cursor up to the one it is in.
    pub fn tree_collapse(&mut self) {
        let Some(tree) = self.tree_mut() else { return };
        let Some(row) = tree.rows.get(tree.cursor).cloned() else { return };
        if row.mark == TreeMark::Open {
            tree.expanded.remove(&row.path);
            tree.rebuild();
        } else if let Some(parent) = row.path.parent() {
            tree.cursor = tree.rows.iter().position(|row| row.path == parent).unwrap_or(tree.cursor);
        }
    }

    /// Enter: the other panel sent to the directory under the cursor, the
    /// tree left as it is.
    pub fn tree_enter(&mut self) {
        let side = self.active_side;
        let Some(path) = self.panel(side).tree.as_ref().and_then(|tree| tree.rows.get(tree.cursor)).map(|row| row.path.clone()) else {
            return;
        };
        self.open_dir(!side, path, None);
    }
}
