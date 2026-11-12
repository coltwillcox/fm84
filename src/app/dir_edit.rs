//! Ctrl+E: the panel's names in the editor, a numbered line each, as vidir
//! has them. A line edited renames its entry, a line deleted deletes it, and
//! a line duplicated copies it - the number says which entry a line is. The
//! save shows the plan, and it runs as one job.

use super::*;
use std::io::Error;

/// What a directory edit opened on: the entries, in the order of their
/// numbers.
#[derive(Clone, Debug)]
pub struct DirEdit {
    entries: Vec<EditEntry>,
}

#[derive(Clone, Debug)]
struct EditEntry {
    path: PathBuf,
    /// Its name as its line shows it. A line left saying this keeps the name
    /// as the filesystem holds it - which for one that is not valid UTF-8,
    /// or has a control character in it, the line could not.
    shown: String,
    is_dir: bool,
}

/// One thing the plan does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOp {
    /// To the trash, or for good, as F11 says F8 goes.
    Delete(PathBuf, bool),
    Rename(PathBuf, PathBuf),
    /// From the entry as it is, which a rename may have moved first.
    Copy(PathBuf, PathBuf, bool),
}

/// The plan on screen, waiting on Enter.
pub struct EditPlan {
    pub ops: Vec<EditOp>,
    pub to_trash: bool,
    pub scroll: usize,
    /// Rows the popup showed last, for a page up or down.
    pub room: usize,
}

impl DirEdit {
    /// The lines the editor opens on: each entry's number, padded to one
    /// width, then its name, a directory's with a separator on the end.
    pub fn lines(&self) -> Vec<String> {
        let width = self.entries.len().to_string().len();
        self.entries.iter().enumerate().map(|(index, entry)| format!("{:0width$} {}{}", index + 1, entry.shown, if entry.is_dir { "/" } else { "" })).collect()
    }

    /// What `lines` asks for, or why it cannot be had. Deletes come first,
    /// then the renames - together, so names that swap or go round in a
    /// chain do - and the copies last, so that a name either of the others
    /// gives up can be copied to.
    pub fn plan(&self, lines: &[String]) -> Result<Vec<EditOp>, String> {
        // The lines that name each entry, with where they are.
        let mut named: Vec<Vec<(usize, &str)>> = vec![Vec::new(); self.entries.len()];
        for (index, line) in lines.iter().enumerate() {
            let at = index + 1;
            let line = line.trim_start();
            if line.trim().is_empty() {
                continue;
            }
            let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let Some(name) = line[digits..].strip_prefix(' ').filter(|_| digits > 0) else {
                return Err(t!("Line {line}: no entry number in front of the name", line = at));
            };
            let number = &line[..digits];
            let Some(entry) = number.parse::<usize>().ok().and_then(|number| number.checked_sub(1)).and_then(|index| named.get_mut(index)) else {
                return Err(t!("Line {line}: there is no entry {number}", line = at, number));
            };
            let name = name.strip_suffix('/').or_else(|| name.strip_suffix(std::path::MAIN_SEPARATOR)).unwrap_or(name);
            entry.push((at, name));
        }

        let mut deletes = Vec::new();
        let mut renames = Vec::new();
        let mut copies = Vec::new();
        // Every name there will be, and the line that gives it.
        let mut after: HashMap<OsString, (usize, &str)> = HashMap::new();
        // Names given up, by a rename or a delete, free to be taken.
        let mut leaving = HashSet::new();
        // Names taken that something may be in the way of.
        let mut taking = Vec::new();
        for (entry, lines) in self.entries.iter().zip(&named) {
            if lines.is_empty() {
                deletes.push(EditOp::Delete(entry.path.clone(), entry.is_dir));
                leaving.insert(fold_for_fs(&entry.path));
                continue;
            }
            let kept = lines.iter().position(|(_, name)| *name == entry.shown);
            // Where the entry is once renamed, for its copies to come from.
            // Without a line keeping its name the first renames it, so its
            // copies all come after that.
            let mut now = entry.path.clone();
            for (index, &(at, name)) in lines.iter().enumerate() {
                let to = if Some(index) == kept {
                    entry.path.clone()
                } else {
                    if !crate::fs_ops::is_plain_name(name) {
                        return Err(t!("Line {line}: {name} - {why}", line = at, name, why = t!("not a plain name")));
                    }
                    entry.path.with_file_name(name)
                };
                if after.insert(fold_for_fs(&to), (at, name)).is_some() {
                    return Err(t!("Line {line}: {name} - {why}", line = at, name, why = t!("given twice")));
                }
                if Some(index) == kept {
                    continue;
                }
                taking.push((at, name, entry.path.clone(), to.clone()));
                if kept.is_none() && index == 0 {
                    leaving.insert(fold_for_fs(&entry.path));
                    renames.push(EditOp::Rename(entry.path.clone(), to.clone()));
                    now = to;
                } else {
                    copies.push((now.clone(), to, entry.is_dir));
                }
            }
        }
        for (at, name, from, to) in taking {
            // The entry itself under another case, on a filesystem that does
            // not tell the two apart, is not in its own way.
            if path_exists(&to) && !leaving.contains(&fold_for_fs(&to)) && !crate::fs_ops::is_same_entry(&from, &to) {
                return Err(t!("Line {line}: {name} - {why}", line = at, name, why = t!("already exists")));
            }
        }
        Ok(deletes.into_iter().chain(renames).chain(copies.into_iter().map(|(from, to, is_dir)| EditOp::Copy(from, to, is_dir))).collect())
    }
}

impl AppState {
    /// Ctrl+E: the active panel's entries, as it shows them, in the editor.
    pub fn open_dir_edit(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let panel = self.panel(side);
        // Its names are paths through the directories under it, and a copy
        // or a rename would have to say which of them it goes to.
        if panel.flat.is_some() {
            self.display_error(t!("Ctrl+B's view cannot be edited as text").to_string());
            return;
        }
        let entries = panel
            .children
            .iter()
            .filter(|item| !item.is_parent())
            .map(|item| EditEntry {
                path: item.path_in(&panel.dir),
                shown: crate::utils::printable_name(&item.name_os.to_string_lossy()),
                is_dir: item.is_dir,
            })
            .collect();
        let edit = DirEdit { entries };
        let mut lines = edit.lines();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let line_widths = lines.iter().map(|line| crate::utils::line_display_width(line)).collect();
        self.screen = Screen::Editor(EditorState {
            file_path: panel.dir.clone(),
            lines,
            highlighted_lines: Vec::new(),
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            horizontal_offset: 0,
            modified: false,
            auto_scroll: true,
            selection_anchor: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            line_states: Vec::new(),
            line_ending: "\n",
            disk_stamp: None,
            line_widths,
            dir_edit: Some(Box::new(edit)),
            edits: 0,
        });
    }

    /// A save in a directory edit: the plan, shown to be run, or why the
    /// lines cannot be one. Done when there is nothing to do.
    pub(super) fn plan_dir_edit(&mut self) -> Result<Saved, String> {
        let Some(state) = self.editor() else {
            return Ok(Saved::Done);
        };
        let Some(edit) = &state.dir_edit else {
            return Ok(Saved::Done);
        };
        let ops = edit.plan(&state.lines)?;
        if ops.is_empty() {
            if let Some(state) = self.editor_mut() {
                state.modified = false;
            }
            self.find_note = Some(t!("Nothing to change").to_string());
            return Ok(Saved::Done);
        }
        let to_trash = self.options.delete_to_trash;
        self.dialog = Some(Dialog::DirEdit(Box::new(EditPlan { ops, to_trash, scroll: 0, room: 1 })));
        Ok(Saved::Planned)
    }

    /// Enter on the plan: the editor closed, and the plan run.
    pub fn apply_dir_edit(&mut self) {
        let Some(Dialog::DirEdit(plan)) = self.dialog.take() else {
            return;
        };
        self.close_editor();
        let EditPlan { ops, to_trash, .. } = *plan;
        self.start_job(TransferKind::DirEdit, Clashes::Ask, move |progress| run_dir_edit(ops, to_trash, progress));
    }
}

/// The worker thread behind a directory edit, counted in what it does.
fn run_dir_edit(ops: Vec<EditOp>, to_trash: bool, mut progress: JobProgress<'_>) {
    let _ = progress.updates.send(JobUpdate::Total(ops.len() as u64));
    let outcome = apply(ops, to_trash, &mut progress);
    progress.finish(outcome);
}

fn apply(ops: Vec<EditOp>, to_trash: bool, progress: &mut JobProgress<'_>) -> Result<Transfer, Error> {
    let mut renames = Vec::new();
    let mut rest = Vec::new();
    for op in ops {
        match op {
            EditOp::Rename(from, to) => renames.push((from, to)),
            op => rest.push(op),
        }
    }
    let (deletes, copies): (Vec<EditOp>, Vec<EditOp>) = rest.into_iter().partition(|op| matches!(op, EditOp::Delete(..)));

    for op in deletes {
        let EditOp::Delete(path, is_dir) = op else { continue };
        let since = now_seconds();
        let outcome = if to_trash {
            crate::fs_ops::trash_path(path.clone(), is_dir, &mut InSteps(progress))?
        } else {
            delete_path(path.clone(), is_dir, &mut InSteps(progress))?
        };
        if to_trash && !path_exists(&path) {
            progress.did(Done::Trashed { from: path, since });
        }
        if outcome == Transfer::Cancelled || !progress.step(Step::Advanced(1)) {
            return Ok(Transfer::Cancelled);
        }
    }

    if let Some((first, _)) = renames.first() {
        if !progress.step(Step::Starting(first)) {
            return Ok(Transfer::Cancelled);
        }
        let result = rename_all(&renames);
        let done = match &result {
            Ok(()) => renames.len(),
            Err((done, _)) => *done,
        };
        for (from, to) in &renames[..done] {
            if let Some(mark) = Mark::of(to, false) {
                progress.did(Done::Moved { from: from.clone(), to: to.clone(), mark });
            }
        }
        result.map_err(|(_, error)| error)?;
        if !progress.step(Step::Advanced(renames.len() as u64)) {
            return Ok(Transfer::Cancelled);
        }
    }

    for op in copies {
        let EditOp::Copy(from, to, is_dir) = op else { continue };
        let new = !path_exists(&to);
        let outcome = copy_path(from, to.clone(), is_dir, &mut InSteps(progress));
        if new && let Some(mark) = Mark::of(&to, true) {
            progress.did(Done::Copied { to, mark });
        }
        if outcome? == Transfer::Cancelled || !progress.step(Step::Advanced(1)) {
            return Ok(Transfer::Cancelled);
        }
    }
    Ok(Transfer::Done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(dir: &Path, names: &[(&str, bool)]) -> DirEdit {
        DirEdit {
            entries: names
                .iter()
                .map(|&(name, is_dir)| EditEntry {
                    path: dir.join(name),
                    shown: name.to_string(),
                    is_dir,
                })
                .collect(),
        }
    }

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn lines_are_numbered_to_one_width_and_directories_marked() {
        let names: Vec<(String, bool)> = (1..=10).map(|n| (format!("f{n}"), n == 1)).collect();
        let names: Vec<(&str, bool)> = names.iter().map(|(name, is_dir)| (name.as_str(), *is_dir)).collect();
        let shown = edit(Path::new("/nowhere"), &names).lines();
        assert_eq!(shown[0], "01 f1/");
        assert_eq!(shown[9], "10 f10");
    }

    #[test]
    fn edited_deleted_and_duplicated_lines_make_the_plan() {
        let dir = Path::new("/nowhere-fm84");
        let edit = edit(dir, &[("a", false), ("b", true), ("c", false)]);
        // a renamed, b deleted, c kept and copied twice - once before it.
        let plan = edit.plan(&lines(&["1 x", "3 c2", "3 c", "", "3 c3"])).unwrap();
        assert_eq!(
            plan,
            vec![
                EditOp::Delete(dir.join("b"), true),
                EditOp::Rename(dir.join("a"), dir.join("x")),
                EditOp::Copy(dir.join("c"), dir.join("c2"), false),
                EditOp::Copy(dir.join("c"), dir.join("c3"), false),
            ]
        );
        // Untouched, there is nothing to do; a directory's separator is
        // taken off, and its line can be left without it.
        assert_eq!(edit.plan(&lines(&["1 a", "2 b/", "3 c"])).unwrap(), vec![]);
        assert_eq!(edit.plan(&lines(&["1 a", "2 b", "3 c"])).unwrap(), vec![]);
    }

    #[test]
    fn a_copy_of_an_entry_renamed_comes_from_its_new_name() {
        let dir = Path::new("/nowhere-fm84");
        let edit = edit(dir, &[("a", false)]);
        let plan = edit.plan(&lines(&["1 b", "1 c"])).unwrap();
        assert_eq!(plan, vec![EditOp::Rename(dir.join("a"), dir.join("b")), EditOp::Copy(dir.join("b"), dir.join("c"), false)]);
    }

    #[test]
    fn names_swapped_and_names_given_up_can_be_taken() {
        let dir = Path::new("/nowhere-fm84");
        let edit = edit(dir, &[("a", false), ("b", false)]);
        assert_eq!(edit.plan(&lines(&["1 b", "2 a"])).unwrap(), vec![EditOp::Rename(dir.join("a"), dir.join("b")), EditOp::Rename(dir.join("b"), dir.join("a"))]);
    }

    #[test]
    fn what_cannot_be_had_says_which_line() {
        let dir = std::env::temp_dir().join(format!("fm84-dir-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hidden"), "").unwrap();
        let edit = edit(&dir, &[("a", false), ("b", false)]);
        let error = |text: &[&str]| edit.plan(&lines(text)).unwrap_err();
        assert_eq!(error(&["1 a", "b"]), "Line 2: no entry number in front of the name");
        assert_eq!(error(&["1 a", "3 b"]), "Line 2: there is no entry 3");
        assert_eq!(error(&["1 a", "2 a"]), "Line 2: a - given twice");
        assert_eq!(error(&["1 a", "2 sub/b"]), "Line 2: sub/b - not a plain name");
        // Not in the panel, but there.
        assert_eq!(error(&["1 a", "2 hidden"]), "Line 2: hidden - already exists");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
