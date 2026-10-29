//! A panel inside an archive: what it can do there, and what it refuses.

use super::*;

/// What is said of anything that would write into an archive.
pub const READ_ONLY: &str = "Archives are read-only";

impl AppState {
    /// The archive a panel's directory runs through, as its listing found it.
    pub fn panel_archive(&self, is_left: bool) -> Option<&PathBuf> {
        if is_left { self.archive_left.as_ref() } else { self.archive_right.as_ref() }
    }

    /// The directory on the disk a panel is in: the one it shows, or for a
    /// panel inside an archive, the one the archive is in - for what needs a
    /// real directory, such as a terminal, git, or a shell's cd.
    pub fn panel_real_dir(&self, is_left: bool) -> PathBuf {
        match self.panel_archive(is_left) {
            Some(archive) => archive.parent().map_or_else(|| archive.clone(), Path::to_path_buf),
            None => self.panel_dir(is_left),
        }
    }

    /// Refuse, with a word, what would write into the archive the panel is
    /// in. True when it did.
    pub fn refuse_in_archive(&mut self, is_left: bool) -> bool {
        let inside = self.panel_archive(is_left).is_some();
        if inside {
            self.display_error(READ_ONLY.to_string());
        }
        inside
    }

    /// A file inside the archive the active panel is in, copied out to a
    /// scratch directory of its own and then shown in the viewer, or handed
    /// to the program the system opens it with. A job, with its bar and Esc:
    /// out of a large .tar.gz it can take a while.
    pub fn open_archive_entry(&mut self, path: &Path, view: bool) {
        let Some(archive) = self.panel_archive(self.is_left_active).cloned() else {
            return;
        };
        let (Ok(inner), Some(name)) = (path.strip_prefix(&archive), path.file_name()) else {
            return;
        };
        let dest = match crate::archive::scratch_dir() {
            Ok(dir) => dir.join(name),
            Err(e) => {
                self.display_error(format!("Cannot make room to open it: {e}"));
                return;
            }
        };
        let after = if view { AfterJob::View(dest.clone()) } else { AfterJob::Open(dest.clone()) };
        self.start_extract(archive, vec![(inner.to_path_buf(), dest)], Clashes::ReplaceAll, Some(after));
    }

    /// Shift+F3 on two files, either of them inside an archive - one a panel
    /// is in. What is inside one is copied out to a scratch directory first,
    /// a job for each archive, and the two compared from there under the
    /// paths they have in the panels. Two on the disk are compared at once.
    pub fn compare_anywhere(&mut self, left: PathBuf, right: PathBuf) {
        let archives: Vec<PathBuf> = [self.archive_left.clone(), self.archive_right.clone()].into_iter().flatten().collect();
        // Each archive with what is to come out of it.
        let mut out_of: Vec<(PathBuf, Vec<(PathBuf, PathBuf)>)> = Vec::new();
        let mut files = Vec::new();
        for path in [&left, &right] {
            let Some(archive) = archives.iter().find(|archive| path.starts_with(archive)) else {
                files.push(path.clone());
                continue;
            };
            let (Ok(inner), Some(name)) = (path.strip_prefix(archive), path.file_name()) else {
                return;
            };
            let dest = match crate::archive::scratch_dir() {
                Ok(dir) => dir.join(name),
                Err(e) => return self.display_error(format!("Cannot make room to compare: {e}")),
            };
            match out_of.iter_mut().find(|(known, _)| known == archive) {
                Some((_, items)) => items.push((inner.to_path_buf(), dest.clone())),
                None => out_of.push((archive.clone(), vec![(inner.to_path_buf(), dest.clone())])),
            }
            files.push(dest);
        }
        let (Some(right_file), Some(left_file)) = (files.pop(), files.pop()) else {
            return;
        };
        if out_of.is_empty() {
            return self.compare_files(left, right);
        }
        let (archive, items) = out_of.remove(0);
        let after = AfterJob::Compare {
            files: (left_file, right_file),
            shown: (left, right),
            then: out_of.pop(),
        };
        self.start_extract(archive, items, Clashes::ReplaceAll, Some(after));
    }

    /// Compare two files, the diff headed with the paths they are known by.
    pub(super) fn compare_shown(&mut self, files: (PathBuf, PathBuf), shown: (PathBuf, PathBuf)) {
        self.compare_files(files.0, files.1);
        if let Some(state) = self.diff_mut() {
            (state.left_path, state.right_path) = shown;
        }
    }
}
