//! F11's git gutter: beside the line numbers in the viewer and the editor, a
//! bar on each line added or changed against what HEAD has of the file, and
//! a mark where lines were taken away; Alt+] and Alt+[ - or Alt+Down and
//! Alt+Up, where the terminal cannot send the brackets - go from one run of
//! changes to the next; and the status bar says who last changed the line
//! the cursor is on. What HEAD has, and who changed what, is read on a
//! thread when a file is opened, and again when HEAD moves - a commit made
//! while it is open; the marks are worked out again whenever the text
//! changes.

use super::*;
use crate::git::Blamed;

/// What a line is against HEAD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Not there in HEAD.
    Added,
    /// In HEAD, with other text.
    Changed,
    /// As HEAD has it, with lines HEAD has taken away just above it.
    RemovedAbove,
    /// The last line, with lines HEAD has taken away below it.
    RemovedBelow,
}

/// What HEAD has of the file: its lines, and who last changed each.
struct Head {
    lines: Vec<String>,
    /// For each line, an index into `commits`; empty when blame said nothing.
    blamed: Vec<usize>,
    commits: Vec<Blamed>,
}

/// What a read of HEAD brings: its version of the file, and what tells when
/// HEAD moves again - the file git writes to then, and its time now.
struct Loaded {
    head: Option<Head>,
    moved: Option<(PathBuf, Option<SystemTime>)>,
}

/// How often the gutter looks whether HEAD has moved.
const HEAD_CHECK: Duration = Duration::from_secs(1);

/// The gutter of the file open in the viewer or the editor.
pub struct GitGutter {
    pub path: PathBuf,
    /// HEAD's version on its way; None once it is here, or nothing is coming.
    loading: Option<Receiver<Loaded>>,
    head: Option<Head>,
    /// The file git writes to as HEAD moves, and its time when HEAD's
    /// version was read: a time since then, and it is read again.
    moved: Option<(PathBuf, Option<SystemTime>)>,
    /// When it was last looked whether HEAD has moved.
    checked: Instant,
    /// What the text was when the marks were worked out, to tell when it
    /// has changed since.
    shown: Option<(u64, usize, usize)>,
    /// Each line's mark.
    pub marks: Vec<Option<Change>>,
    /// The first line of each run of marks, top to bottom.
    hunks: Vec<usize>,
    /// Each line as HEAD has it, for one that is the same there.
    to_head: Vec<Option<usize>>,
}

/// Lines as the viewer and the editor split a file: an empty last one after a
/// final line break, and always one.
pub fn shown_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if text.ends_with('\n') || lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// HEAD's version of the file at `path`, read on a thread of its own, and
/// what tells when HEAD moves again - looked at first, so a commit made while
/// it is read is not missed.
fn load(path: &Path, limit: u64) -> Receiver<Loaded> {
    let (sender, receiver) = mpsc::channel();
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        // Through a link, the file it leads to - what is shown, and saved -
        // in whichever repository that is in: HEAD holds of a link only the
        // name it points at, which every line would read as changed against.
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let moved = crate::git::head_log(&path).map(|log| {
            let time = std::fs::metadata(&log).and_then(|metadata| metadata.modified()).ok();
            (log, time)
        });
        let _ = sender.send(Loaded { head: read_head(&path, limit), moved });
    });
    receiver
}

/// HEAD's version of the file at `path`, and who changed what in it. None for
/// a file not in a repository, not in HEAD, larger than `limit`, or not text.
fn read_head(path: &Path, limit: u64) -> Option<Head> {
    if crate::git::size_at_head(path).ok()? > limit {
        return None;
    }
    let text = String::from_utf8(crate::git::at_head(path).ok()?).ok()?;
    let (blamed, commits) = crate::git::blame(path).unwrap_or_default();
    Some(Head { lines: shown_lines(&text), blamed, commits })
}

/// Each line of `lines` against `head`: its mark, and the line of HEAD it is
/// for one HEAD has as it is.
fn marks_against(head: &[String], lines: &[String]) -> (Vec<Option<Change>>, Vec<Option<usize>>) {
    let mut marks = vec![None; lines.len()];
    let mut to_head = vec![None; lines.len()];
    let mut removed = false;
    for row in crate::diff::rows(head, lines) {
        match (row.left, row.right) {
            (Some(was), Some(now)) if row.same => {
                to_head[now] = Some(was);
                if removed {
                    marks[now] = Some(Change::RemovedAbove);
                }
                removed = false;
            }
            (Some(_), Some(now)) => {
                marks[now] = Some(Change::Changed);
                removed = false;
            }
            (None, Some(now)) => {
                marks[now] = Some(Change::Added);
                removed = false;
            }
            (Some(_), None) => removed = true,
            (None, None) => {}
        }
    }
    if removed && let Some(last) = marks.last_mut() {
        last.get_or_insert(Change::RemovedBelow);
    }
    (marks, to_head)
}

/// The first line of each run of marks.
fn hunks_of(marks: &[Option<Change>]) -> Vec<usize> {
    (0..marks.len()).filter(|&at| marks[at].is_some() && (at == 0 || marks[at - 1].is_none())).collect()
}

/// What tells one text from another cheaply: an editor's count of its edits,
/// or a viewer's lines, how many and how long the last - which following a
/// file that grows, the only way the viewer's text changes, moves.
fn text_key(app_state: &AppState) -> Option<(PathBuf, (u64, usize, usize))> {
    match &app_state.screen {
        Screen::Editor(state) if state.dir_edit.is_none() => Some((state.file_path.clone(), (state.edits, state.lines.len(), 0))),
        Screen::Viewer(state) if state.mode == ViewMode::Text && !state.from_edit && state.history.as_ref().is_none_or(|history| history.at.is_none()) => {
            Some((state.file_path.clone(), (u64::MAX, state.content_lines.len(), state.content_lines.last().map_or(0, String::len))))
        }
        _ => None,
    }
}

impl AppState {
    /// Once a loop: HEAD's version asked for as a file opens, taken in once
    /// it comes, and the marks worked out again whenever the text changes.
    /// Nothing is kept once the viewer and the editor are closed: a file
    /// opened again is read again, as HEAD has it then.
    pub fn poll_git_gutter(&mut self) {
        if !self.options.git_gutter || !matches!(self.screen, Screen::Viewer(_) | Screen::Editor(_)) {
            self.git_gutter = None;
            return;
        }
        let Some((path, key)) = text_key(self) else {
            return;
        };
        let limit = self.options.large_file_bytes();
        if self.git_gutter.as_ref().is_none_or(|gutter| gutter.path != path) {
            self.git_gutter = Some(GitGutter {
                loading: Some(load(&path, limit)),
                path,
                head: None,
                moved: None,
                checked: Instant::now(),
                shown: None,
                marks: Vec::new(),
                hunks: Vec::new(),
                to_head: Vec::new(),
            });
        }
        let Some(gutter) = self.git_gutter.as_mut() else {
            return;
        };
        if let Some(loading) = &gutter.loading {
            match loading.try_recv() {
                Ok(loaded) => {
                    gutter.head = loaded.head;
                    gutter.moved = loaded.moved;
                    gutter.loading = None;
                    gutter.shown = None;
                    gutter.checked = Instant::now();
                }
                Err(TryRecvError::Disconnected) => gutter.loading = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // HEAD moved since it was read - a commit made in a shell beside it:
        // read again, the marks there are kept until the new ones come.
        if gutter.loading.is_none() && gutter.checked.elapsed() >= HEAD_CHECK {
            gutter.checked = Instant::now();
            if let Some((log, then)) = &gutter.moved
                && std::fs::metadata(log).and_then(|metadata| metadata.modified()).ok() != *then
            {
                gutter.loading = Some(load(&gutter.path, limit));
            }
        }
        let Some(head) = &gutter.head else {
            return;
        };
        if gutter.shown == Some(key) {
            return;
        }
        let lines = match &self.screen {
            Screen::Editor(state) => &state.lines,
            Screen::Viewer(state) => &state.content_lines,
            _ => return,
        };
        (gutter.marks, gutter.to_head) = marks_against(&head.lines, lines);
        gutter.hunks = hunks_of(&gutter.marks);
        gutter.shown = Some(key);
    }

    /// The gutter of what the viewer or the editor shows, when F11 has one,
    /// HEAD's version is here and the marks are of the text as it is.
    pub fn git_marks(&self) -> Option<&[Option<Change>]> {
        let (path, key) = text_key(self)?;
        let gutter = self.git_gutter.as_ref().filter(|gutter| gutter.path == path && gutter.shown == Some(key))?;
        Some(&gutter.marks)
    }

    /// The line the gutter's steps and the status bar go by: the editor's
    /// cursor's, or the viewer's top line.
    fn gutter_line(&self) -> Option<usize> {
        match &self.screen {
            Screen::Editor(state) => Some(state.cursor_line),
            Screen::Viewer(state) => Some(state.row(state.scroll_offset).0),
            _ => None,
        }
    }

    /// Alt+] or Alt+[: to the next run of changes after the line the gutter
    /// goes by, or the one before it - the editor's cursor to its first line,
    /// the viewer to show it a few lines down. Said when there is none.
    pub fn git_change_step(&mut self, forward: bool) {
        let Some(line) = self.gutter_line().filter(|_| self.git_marks().is_some()) else {
            return;
        };
        let hunks = self.git_gutter.as_ref().map(|gutter| gutter.hunks.clone()).unwrap_or_default();
        let target = if forward { hunks.iter().copied().find(|&at| at > line) } else { hunks.iter().rev().copied().find(|&at| at < line) };
        let Some(target) = target else {
            self.find_note = Some(
                if hunks.is_empty() {
                    t!("No changes against HEAD")
                } else if forward {
                    t!("No change below")
                } else {
                    t!("No change above")
                }
                .to_string(),
            );
            return;
        };
        match &mut self.screen {
            Screen::Editor(state) => {
                state.cursor_line = target;
                state.cursor_col = 0;
                state.selection_anchor = None;
                state.auto_scroll = true;
            }
            Screen::Viewer(state) => {
                let row = state.row_of(target, 0).saturating_sub(crate::diff::CONTEXT);
                state.follow = None;
                state.scroll_offset = row.min(state.total_lines.saturating_sub(self.drawn.viewer_rows()));
            }
            _ => {}
        }
    }

    /// Who last changed the line the gutter goes by, for the status bar: the
    /// author, how long ago and the commit's subject; or that it is not
    /// committed yet. None with nothing to say.
    pub fn git_blame_line(&self) -> Option<String> {
        let line = self.gutter_line()?;
        let marks = self.git_marks()?;
        let gutter = self.git_gutter.as_ref()?;
        if matches!(marks.get(line), Some(Some(Change::Added | Change::Changed))) {
            return Some(t!("Not committed yet").to_string());
        }
        let head = gutter.head.as_ref()?;
        let commit = gutter.to_head.get(line).copied().flatten().and_then(|at| head.blamed.get(at)).and_then(|&at| head.commits.get(at))?;
        let when = std::time::UNIX_EPOCH + Duration::from_secs(commit.when.max(0) as u64);
        let ago = crate::utils::format_modified(when, crate::options::DateFormat::Relative, SystemTime::now());
        Some(format!("{}, {ago}: {}", crate::utils::printable_name(&commit.author), crate::utils::printable_name(&commit.summary)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        shown_lines(text)
    }

    /// A line changed is marked; committed, it is not - whether the file is
    /// opened again after the commit, or stays open while it is made.
    #[test]
    fn a_commit_takes_the_marks_away() {
        let dir = std::env::temp_dir().join(format!("fm84-gutter-commit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |arguments: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
                .args(arguments)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !run(&["init", "--quiet"]) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let file = dir.join("file.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        assert!(run(&["add", "."]) && run(&["commit", "--quiet", "-m", "first"]));
        std::fs::write(&file, "one\nTWO\n").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        // Polled until the marks have something on them, or say nothing.
        let marked = |app_state: &mut AppState, want: bool| {
            let started = Instant::now();
            loop {
                app_state.poll_git_gutter();
                let any = app_state.git_marks().is_some_and(|marks| marks.iter().any(Option::is_some));
                if any == want || started.elapsed() > Duration::from_secs(5) {
                    return any;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let outcome = (|| {
            app_state.open_editor(file.clone()).ok()?;
            let before = marked(&mut app_state, true);
            app_state.close_editor();
            app_state.poll_git_gutter();
            run(&["commit", "--quiet", "-am", "second"]).then_some(())?;
            app_state.open_editor(file.clone()).ok()?;
            let reopened = marked(&mut app_state, false);

            // Changed again, open, and committed while it is.
            std::fs::write(&file, "one\nthree\n").ok()?;
            app_state.close_editor();
            app_state.poll_git_gutter();
            app_state.open_editor(file.clone()).ok()?;
            let again = marked(&mut app_state, true);
            run(&["commit", "--quiet", "-am", "third"]).then_some(())?;
            let while_open = marked(&mut app_state, false);
            Some((before, reopened, again, while_open))
        })();
        std::fs::remove_dir_all(&dir).unwrap();
        let (before, reopened, again, while_open) = outcome.expect("the repository's commands");
        assert!(before, "marked before the commit");
        assert!(!reopened, "opened again after it: nothing marked");
        assert!(again, "marked once changed again");
        assert!(!while_open, "committed while open: the marks go");
    }

    /// A file opened through a link is marked against what HEAD has of the
    /// file it leads to: the link in HEAD is only the name it points at, and
    /// against that every line would read as changed.
    #[cfg(unix)]
    #[test]
    fn a_file_opened_through_a_link_is_marked_against_the_file() {
        let dir = std::env::temp_dir().join(format!("fm84-gutter-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |arguments: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
                .args(arguments)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !run(&["init", "--quiet"]) {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::fs::write(dir.join("real.txt"), "one\ntwo\n").unwrap();
        std::os::unix::fs::symlink("real.txt", dir.join("link.txt")).unwrap();
        let committed = run(&["add", "."]) && run(&["commit", "--quiet", "-m", "first"]);
        std::fs::write(dir.join("real.txt"), "one\nTWO\n").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        let marks = app_state.open_editor(dir.join("link.txt")).ok().and_then(|()| {
            let started = Instant::now();
            loop {
                app_state.poll_git_gutter();
                if let Some(marks) = app_state.git_marks() {
                    return Some(marks.to_vec());
                }
                if started.elapsed() > Duration::from_secs(5) {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(committed);
        assert_eq!(marks, Some(vec![None, Some(Change::Changed), None]), "only the line changed in the file itself");
    }

    #[test]
    fn lines_split_as_the_viewer_and_the_editor_split_them() {
        assert_eq!(shown_lines("a\nb\n"), ["a", "b", ""]);
        assert_eq!(shown_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(shown_lines(""), [""]);
    }

    #[test]
    fn each_line_is_marked_against_head() {
        let head = lines("one\ntwo\nthree\nfour\nfive\n");
        let now = lines("one\nTWO\nthree\nnew\nfive\n");
        let (marks, to_head) = marks_against(&head, &now);
        assert_eq!(marks, [None, Some(Change::Changed), None, Some(Change::Changed), None, None]);
        assert_eq!(to_head[2], Some(2));
        assert_eq!(to_head[1], None);

        // Added in the middle, and lines taken away above one kept.
        let now = lines("one\ntwo\nadded\nthree\nfive\n");
        let (marks, _) = marks_against(&head, &now);
        assert_eq!(marks, [None, None, Some(Change::Added), None, Some(Change::RemovedAbove), None]);
        assert_eq!(hunks_of(&marks), [2, 4]);

        // Taken away at the end: the last line says so.
        let (marks, _) = marks_against(&lines("a\nb\nc"), &lines("a"));
        assert_eq!(marks, [Some(Change::RemovedBelow)]);

        // The same: nothing marked.
        let (marks, _) = marks_against(&head, &head);
        assert!(marks.iter().all(Option::is_none));
    }
}
