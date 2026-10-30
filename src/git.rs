//! The git branches a directory's repository has, and changing to another,
//! and what has changed in it, file by file - by running git itself, which
//! knows a repository's every layout and rule, rather than reading refs the
//! way the path bar's label does.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What git says of a file, the weakest first: a directory holding several
/// shows the strongest of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Ignored,
    Untracked,
    /// Changed, and every change added to the index.
    Staged,
    /// Changed in the working tree since it was last added.
    Modified,
    /// Both sides of a merge changed it.
    Conflict,
}

/// The marks for a directory's entries, by their paths from it - a name, or
/// under Ctrl+B a path through the directories below.
#[derive(Debug, Default)]
pub struct Status {
    /// What git named itself: a file, or an untracked or ignored directory
    /// whole.
    named: HashMap<OsString, Mark>,
    /// For a directory, the strongest of what is changed inside it. Nothing
    /// that is only ignored: a directory is not ignored for holding some.
    inside: HashMap<OsString, Mark>,
}

impl Status {
    /// The mark for the entry at `name`, if git has one for it. A file in an
    /// untracked or ignored directory - which git names whole - is as that
    /// directory is.
    pub fn mark(&self, name: &OsStr, is_dir: bool) -> Option<Mark> {
        if let Some(&mark) = self.named.get(name) {
            return Some(mark);
        }
        if is_dir && let Some(&mark) = self.inside.get(name) {
            return Some(mark);
        }
        Path::new(name).ancestors().skip(1).filter(|dir| !dir.as_os_str().is_empty()).find_map(|dir| self.named.get(dir.as_os_str()).copied())
    }

    /// Take in a line of `git status --porcelain`, by its path from the
    /// directory listed.
    fn add(&mut self, path: PathBuf, mark: Mark) {
        if mark != Mark::Ignored {
            for dir in path.ancestors().skip(1).filter(|dir| !dir.as_os_str().is_empty()) {
                let held = self.inside.entry(dir.as_os_str().to_os_string()).or_insert(mark);
                *held = (*held).max(mark);
            }
        }
        let held = self.named.entry(path.into_os_string()).or_insert(mark);
        *held = (*held).max(mark);
    }
}

/// What git says has changed under `dir`, from `git status`: every file
/// modified, staged, untracked or ignored, and what each directory holds.
pub fn status(dir: &Path) -> Result<Status, String> {
    // Paths come back from the top of the repository; this is where `dir`
    // sits in it, with a / on the end, or nothing at the top.
    let prefix = git_bytes(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = prefix.strip_suffix(b"\n").unwrap_or(&prefix);
    let output = git_bytes(dir, &["status", "--porcelain=v1", "-z", "--ignored", "--untracked-files=normal", "--", "."])?;
    let mut status = Status::default();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let (x, y) = (record[0], record[1]);
        // A rename or copy gives where it came from next, which is not here
        // any more.
        if matches!(x, b'R' | b'C') {
            records.next();
        }
        let Some(path) = record[3..].strip_prefix(prefix) else {
            continue;
        };
        let path = path.strip_suffix(b"/").unwrap_or(path);
        if path.is_empty() {
            continue;
        }
        let mark = match (x, y) {
            (b'?', b'?') => Mark::Untracked,
            (b'!', b'!') => Mark::Ignored,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Mark::Conflict,
            (_, b' ') => Mark::Staged,
            _ => Mark::Modified,
        };
        status.add(from_git(path), mark);
    }
    Ok(status)
}

/// A path as git writes it - bytes, separated by / - as the system's own.
fn from_git(path: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(OsStr::from_bytes(path))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(path).split('/').collect()
    }
}

/// The local branches of the repository `dir` is in, in git's order, and
/// which of them is checked out - None when HEAD is detached.
pub fn branches(dir: &Path) -> Result<(Vec<String>, Option<usize>), String> {
    let output = git(dir, &["for-each-ref", "--format=%(HEAD)%00%(refname:short)", "refs/heads/"])?;
    let mut names = Vec::new();
    let mut current = None;
    for line in output.lines() {
        let Some((head, name)) = line.split_once('\0') else {
            continue;
        };
        if head == "*" {
            current = Some(names.len());
        }
        names.push(name.to_string());
    }
    Ok((names, current))
}

/// Check out `branch` in the repository `dir` is in. Git refuses when the
/// change would lose work - edits to a file the other branch has otherwise -
/// and what it says then is the error.
pub fn switch(dir: &Path, branch: &str) -> Result<(), String> {
    git(dir, &["switch", "--no-guess", branch]).map(drop)
}

/// Run git in `dir`, giving back what it wrote, or what it said went wrong.
/// Nothing it could ask for is answered: a prompt would wait for good on a
/// terminal fm84 is drawing on.
fn git(dir: &Path, arguments: &[&str]) -> Result<String, String> {
    git_bytes(dir, arguments).map(|output| String::from_utf8_lossy(&output).into_owned())
}

/// As `git`, what it wrote as bytes: a file name need not be UTF-8.
fn git_bytes(dir: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Locks nothing: a status that refreshed the index could get in the
        // way of a commit being made in the shell beside it.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Cannot run git: {e}"))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    // Git's message as one line, as the error popup wraps it: its line
    // breaks, and the tabs it indents file names by, made single spaces. The
    // "error:" it opens with goes, since the popup says as much, and nothing
    // that would reach the terminal is left in.
    let said = String::from_utf8_lossy(&output.stderr).split_whitespace().collect::<Vec<_>>().join(" ");
    let said = said.strip_prefix("error: ").or_else(|| said.strip_prefix("fatal: ")).unwrap_or(&said);
    let message: String = said.chars().map(|c| if c.is_control() { '.' } else { c }).collect();
    Err(if message.is_empty() { format!("git {} failed", arguments[0]) } else { message })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository of its own, with git told who is committing so a machine
    /// with no identity set can still make one. None when there is no git to
    /// run, which leaves nothing to test.
    fn repository(name: &str) -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("fm84-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |arguments: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
                .args(arguments)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !run(&["init", "--quiet", "--initial-branch=main"]) {
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        std::fs::write(dir.join("file.txt"), "main\n").unwrap();
        assert!(run(&["add", "file.txt"]));
        assert!(run(&["commit", "--quiet", "-m", "first"]));
        assert!(run(&["branch", "feature/tabs"]));
        Some(dir)
    }

    #[test]
    fn the_branches_and_which_is_checked_out_and_changing_it() {
        let Some(dir) = repository("switch") else {
            return;
        };
        assert_eq!(branches(&dir).unwrap(), (vec!["feature/tabs".to_string(), "main".to_string()], Some(1)));
        // From anywhere inside it.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert_eq!(branches(&dir.join("sub")).unwrap().1, Some(1));

        switch(&dir, "feature/tabs").unwrap();
        assert_eq!(branches(&dir).unwrap().1, Some(0));
        assert_eq!(crate::fs_ops::git_branch(&dir).as_deref(), Some("feature/tabs"));

        // A branch that is not there is refused, in git's words.
        let refused = switch(&dir, "nowhere").unwrap_err();
        assert!(refused.contains("nowhere"), "{refused}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Each kind of change, by the path from where it is asked - a
    /// directory below the top included - and what directories hold.
    #[test]
    fn what_has_changed_file_by_file() {
        let Some(dir) = repository("status") else {
            return;
        };
        let run = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(&dir).args(arguments).output().unwrap().status.success());
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::create_dir_all(dir.join("fresh")).unwrap();
        std::fs::write(dir.join("src/deep/lib.rs"), "one").unwrap();
        std::fs::write(dir.join("src/main.rs"), "one").unwrap();
        run(&["add", "src"]);
        run(&["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "src"]);
        std::fs::write(dir.join("file.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("src/deep/lib.rs"), "two").unwrap();
        std::fs::write(dir.join("src/main.rs"), "two").unwrap();
        run(&["add", "src/main.rs"]);
        std::fs::write(dir.join("new.txt"), "").unwrap();
        std::fs::write(dir.join("fresh/a.txt"), "").unwrap();
        std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(dir.join("build/out"), "").unwrap();

        let status = status(&dir).unwrap();
        let mark = |name: &str, is_dir: bool| status.mark(OsStr::new(name), is_dir);
        assert_eq!(mark("file.txt", false), Some(Mark::Modified));
        assert_eq!(mark("new.txt", false), Some(Mark::Untracked));
        assert_eq!(mark("build", true), Some(Mark::Ignored));
        assert_eq!(mark("fresh", true), Some(Mark::Untracked));
        // The strongest of what is inside: one modified beside one staged.
        assert_eq!(mark("src", true), Some(Mark::Modified));
        assert_eq!(mark(".git", true), None);
        // Under Ctrl+B, by paths: each its own, and inside an untracked
        // directory as the directory is.
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(mark(&format!("src{sep}main.rs"), false), Some(Mark::Staged));
        assert_eq!(mark(&format!("src{sep}deep{sep}lib.rs"), false), Some(Mark::Modified));
        assert_eq!(mark(&format!("fresh{sep}a.txt"), false), Some(Mark::Untracked));
        assert_eq!(mark(&format!("src{sep}deep{sep}other.rs"), false), None);

        // From a directory below the top, by paths from there.
        let below = super::status(&dir.join("src")).unwrap();
        assert_eq!(below.mark(OsStr::new("main.rs"), false), Some(Mark::Staged));
        assert_eq!(below.mark(OsStr::new("deep"), true), Some(Mark::Modified));
        assert_eq!(below.mark(OsStr::new("file.txt"), false), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn outside_a_repository_is_an_error() {
        let dir = std::env::temp_dir().join(format!("fm84-git-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Unless the temporary directory is itself in a repository.
        if crate::fs_ops::git_branch(&dir).is_none() {
            assert!(branches(&dir).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
