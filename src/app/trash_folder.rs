//! Alt+T: the system's trash in a panel, gone into as an archive or a git
//! commit is. F3 views what is there and F5 copies it out, as from an
//! archive; F6 - or Enter, on a file - puts it back where it came from, and
//! F8 empties it for good. What is inside a directory in the trash can be
//! looked at and copied out, but only the whole of what went is put back or
//! emptied.

use super::*;
use std::ffi::OsString;

impl AppState {
    /// Whether a panel is in the trash, at its top or further in.
    pub fn panel_in_trash(&self, side: Side) -> bool {
        self.panel_archive(side).is_some_and(|archive| crate::archive::trash::is_trash_folder(archive))
    }

    /// Alt+T: the trash in the active panel - or, in it already, back to
    /// where the panel was before.
    pub fn toggle_trash(&mut self) {
        let side = self.active_side;
        if self.panel_in_trash(side) {
            return self.travel(true);
        }
        match crate::archive::trash::trash_folder() {
            Some(folder) => {
                self.open_dir(side, folder, None);
            }
            None => self.display_error(t!("No home directory to find the trash from").to_string()),
        }
    }

    /// Of `items`, chosen in the trash, what the system knows each by and
    /// where it came from - or, when one is inside something that went
    /// rather than the whole of it, why none can be.
    fn trashed(&self, items: &[(PathBuf, bool)]) -> Result<Vec<(PathBuf, OsString)>, String> {
        let archive = self.panel_archive(self.active_side).cloned().unwrap_or_default();
        let index = crate::archive::index(&archive).map_err(|error| error.to_string())?;
        let listed = crate::archive::trash::list().map_err(|error| error.to_string())?;
        items
            .iter()
            .map(|(path, _)| {
                let name = path.strip_prefix(&archive).ok().filter(|inner| inner.components().count() == 1);
                let id = name.and_then(|name| index.trash_id(name)).ok_or_else(|| t!("Only what went to the trash whole can be put back or emptied").to_string())?;
                let original = listed.iter().find(|item| item.id == id).map_or_else(|| path.clone(), |item| item.original.join(&item.name));
                Ok((original, id.to_os_string()))
            })
            .collect()
    }

    /// F6, or Enter on a file: `items`, chosen in the trash, put back where
    /// each came from. One whose place has been taken since is asked about.
    pub fn restore_from_trash(&mut self, items: Vec<(PathBuf, bool)>) {
        match self.trashed(&items) {
            Ok(found) => self.start_job(TransferKind::Restore, Clashes::Ask, move |progress| run_trash_ops(found, crate::archive::trash::restore, progress)),
            Err(message) => self.display_error(message),
        }
    }

    /// F8, once asked: `items`, chosen in the trash, gone for good.
    pub fn purge_from_trash(&mut self, items: Vec<(PathBuf, bool)>) {
        match self.trashed(&items) {
            Ok(found) => self.start_job(TransferKind::Delete, Clashes::Ask, move |progress| run_trash_ops(found, crate::archive::trash::purge, progress)),
            Err(message) => self.display_error(message),
        }
    }
}

/// The worker thread behind putting back or emptying: `op` on each item,
/// counted in items, and one that fails asked about by where it came from.
fn run_trash_ops(items: Vec<(PathBuf, OsString)>, op: fn(&std::ffi::OsStr) -> std::io::Result<()>, mut progress: JobProgress<'_>) {
    let _ = progress.updates.send(JobUpdate::Total(items.len() as u64));
    for (original, id) in items {
        if !progress.step(Step::Starting(&original)) {
            return progress.finish(Ok(Transfer::Cancelled));
        }
        loop {
            match op(&id) {
                Ok(()) => {
                    progress.step(Step::Advanced(1));
                    break;
                }
                Err(error) => match progress.failed(&original, &error) {
                    crate::fs_ops::Reply::Retry => {}
                    crate::fs_ops::Reply::Skip | crate::fs_ops::Reply::Delete => break,
                    crate::fs_ops::Reply::Abort => return progress.finish(Err(error)),
                },
            }
        }
    }
    progress.finish(Ok(Transfer::Done));
}
