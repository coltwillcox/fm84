//! Alt+G: git, beside the marks - what is chosen staged, unstaged or its
//! changes thrown away, or the file under the cursor compared with what
//! the last commit has of it.

use super::*;

/// R's list: every revision there is to open, narrowed to those whose name
/// or subject has each word typed.
pub struct RevisionList {
    pub input: TextInput,
    pub all: Vec<crate::git::Revision>,
    /// Those `input` lets through, by their place in `all`.
    pub shown: Vec<usize>,
    pub index: usize,
    /// The top of the repository, and where the panel is below it.
    top: PathBuf,
    prefix: PathBuf,
}

impl RevisionList {
    /// Smart case, as Find is: all in lower case matches either case.
    fn narrow(&mut self) {
        let fold = !self.input.text.chars().any(char::is_uppercase);
        let folded = |text: &str| -> String { if fold { text.chars().map(crate::find::fold_char).collect() } else { text.to_string() } };
        let words: Vec<String> = self.input.text.split_whitespace().map(folded).collect();
        self.shown = (0..self.all.len())
            .filter(|&at| {
                let revision = &self.all[at];
                let text = folded(&format!("{} {}", revision.name, revision.subject));
                words.iter().all(|word| text.contains(word.as_str()))
            })
            .collect();
        self.index = 0;
    }
}

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
        // Each of them, whichever fails: stopping at the first left the rest
        // not done, and nothing said which. The first failure is told.
        let mut failed = None;
        for (path, _) in &items {
            if let Err(message) = run(path) {
                failed.get_or_insert(message);
            }
        }
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

    /// R on the popup: what there is to open - the branches, the tags, the
    /// stashes, the last commits - to pick from or narrow down by typing.
    /// Where the panel is in the repository is worked out now: from inside
    /// one commit's folder, another is gone to at the same place.
    pub fn ask_revision(&mut self) {
        let dir = self.panel_dir(self.active_side);
        let place = match crate::archive::locate(&dir).filter(|located| crate::archive::git_revision(&located.archive).is_some()) {
            Some(located) => Ok((located.archive.parent().map(Path::to_path_buf).unwrap_or_default(), located.inner)),
            None => crate::git::top_and_prefix(&dir),
        };
        let listed = place.and_then(|(top, prefix)| crate::git::revisions(&top).map(|all| (top, prefix, all)));
        match listed {
            Ok((top, prefix, all)) => {
                self.dialog = Some(Dialog::Revision(Box::new(RevisionList {
                    input: TextInput::new(),
                    shown: (0..all.len()).collect(),
                    all,
                    index: 0,
                    top,
                    prefix,
                })))
            }
            Err(message) => self.display_error(message),
        }
    }

    /// The words typed, which the list is narrowed to.
    pub fn revisions_typed(&mut self) {
        if let Some(Dialog::Revision(list)) = &mut self.dialog {
            list.narrow();
        }
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_revision_cursor(&mut self, step: isize) {
        if let Some(Dialog::Revision(list)) = &mut self.dialog {
            list.index = list.index.saturating_add_signed(step).min(list.shown.len().saturating_sub(1));
        }
    }

    /// Enter: the revision highlighted - or, with nothing on the list to
    /// highlight, the one typed, as written - as a read-only folder in the
    /// active panel, at the directory the panel was in, or as near it as
    /// the commit has.
    pub fn open_revision(&mut self) {
        let Some(Dialog::Revision(list)) = self.dialog.take() else {
            return;
        };
        let revision = match list.shown.get(list.index) {
            Some(&at) => list.all[at].name.clone(),
            None => list.input.text.trim().to_string(),
        };
        if revision.is_empty() {
            return;
        }
        if let Err(message) = crate::git::resolve(&list.top, &revision) {
            return self.display_error(message);
        }
        self.scores.revisions += 1;
        self.scores.save();
        let folder = crate::archive::git_folder(&list.top, &revision);
        let target = if list.prefix.as_os_str().is_empty() { folder } else { folder.join(&list.prefix) };
        self.open_dir(self.active_side, target, None);
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
