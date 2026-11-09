//! Alt+G: git, beside the marks - what is chosen staged, unstaged or its
//! changes thrown away, or the file under the cursor compared with what
//! the last commit has of it.

use super::*;

/// Which of Alt+G's answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GitAction {
    Stage,
    Unstage,
    Discard,
}

impl AppState {
    /// Alt+G on what is chosen.
    pub fn open_git(&mut self, items: Vec<(PathBuf, bool)>) {
        if items.is_empty() || self.refuse_in_archive(self.active_side) {
            return;
        }
        self.dialog = Some(Dialog::Git { items, discarding: false });
    }

    /// S, U or D on the popup. D asks first, as F8 for good does: what is
    /// thrown away is in no commit, and nowhere else.
    pub fn git_act(&mut self, action: GitAction) {
        let Some(Dialog::Git { discarding, .. }) = &mut self.dialog else {
            return;
        };
        if action == GitAction::Discard && !*discarding {
            *discarding = true;
            return;
        }
        let Some(Dialog::Git { items, .. }) = self.dialog.take() else {
            return;
        };
        let run = match action {
            GitAction::Stage => crate::git::stage,
            GitAction::Unstage => crate::git::unstage,
            GitAction::Discard => crate::git::discard,
        };
        let failed = items.iter().find_map(|(path, _)| run(path).err());
        // The marks come again with the listing, and a file discarded is
        // what it was.
        self.reload_panel(Side::Left, None);
        self.reload_panel(Side::Right, None);
        match failed {
            Some(message) => self.display_error(message),
            None => {
                self.find_note = Some(
                    match action {
                        GitAction::Stage => t!("Staged"),
                        GitAction::Unstage => t!("Unstaged"),
                        GitAction::Discard => t!("Changes discarded"),
                    }
                    .to_string(),
                )
            }
        }
    }

    /// R on the popup: the revision to open, HEAD to start from.
    pub fn ask_revision(&mut self) {
        let mut input = TextInput::new();
        input.set("HEAD".to_string());
        self.dialog = Some(Dialog::Revision(input));
    }

    /// Enter on the revision: the commit it names, as a read-only folder in
    /// the active panel, at the directory the panel was in - or as near it
    /// as the commit has. From inside one commit, another is gone to there.
    pub fn open_revision(&mut self) {
        let Some(Dialog::Revision(input)) = self.dialog.take() else {
            return;
        };
        let revision = input.text.trim().to_string();
        if revision.is_empty() {
            return;
        }
        let side = self.active_side;
        let dir = self.panel_dir(side);
        let place = match crate::archive::locate(&dir).filter(|located| crate::archive::git_revision(&located.archive).is_some()) {
            Some(located) => Ok((located.archive.parent().map(Path::to_path_buf).unwrap_or_default(), located.inner)),
            None => crate::git::top_and_prefix(&dir),
        };
        let (top, prefix) = match place.and_then(|(top, prefix)| crate::git::resolve(&top, &revision).map(|_| (top, prefix))) {
            Ok(place) => place,
            Err(message) => return self.display_error(message),
        };
        let folder = crate::archive::git_folder(&top, &revision);
        let target = if prefix.as_os_str().is_empty() { folder } else { folder.join(prefix) };
        self.open_dir(side, target, None);
    }

    /// C on the popup: the file side by side with itself as HEAD has it,
    /// the commit on the left as the older side.
    pub fn git_compare_head(&mut self) {
        let Some(Dialog::Git { items, .. }) = self.dialog.take() else {
            return;
        };
        let [(path, false)] = items.as_slice() else {
            return self.display_error(t!("Only a file can be compared with HEAD").to_string());
        };
        let committed = match crate::git::at_head(path) {
            Ok(bytes) => bytes,
            Err(message) => return self.display_error(message),
        };
        let name = path.file_name().unwrap_or_default();
        let saved = crate::archive::scratch_dir().and_then(|dir| {
            let at = dir.join(name);
            std::fs::write(&at, committed).map(|()| at)
        });
        match saved {
            Ok(at) => {
                let mut shown = name.to_os_string();
                shown.push(" @ HEAD");
                self.compare_shown((at, path.clone()), (path.with_file_name(shown), path.clone()));
            }
            Err(error) => self.display_error(error.to_string()),
        }
    }
}
