//! The git branches a directory's repository has, and changing to another,
//! and what has changed in it, file by file - by running git itself, which
//! knows a repository's every layout and rule, rather than reading refs the
//! way the path bar's label does.

use std::borrow::Cow;
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
/// under Ctrl+B a path through the directories below. Kept, and looked up,
/// in Unicode's composed form: git on macOS gives `é` as one character
/// while a file made on an older Mac disk holds it as `e` and an accent,
/// and the two would never meet.
#[derive(Debug, Default)]
pub struct Status {
    /// What git named itself: a file, or an untracked or ignored directory
    /// whole.
    named: HashMap<OsString, Mark>,
    /// For a directory, the strongest of what is changed inside it. Nothing
    /// that is only ignored: a directory is not ignored for holding some.
    inside: HashMap<OsString, Mark>,
    /// For a directory git names whole - untracked, or ignored - or one
    /// inside it: what all of it is. Git says nothing of its entries, only of
    /// the directory, and so does `--`.
    whole: Option<Mark>,
}

impl Status {
    /// The mark for the entry at `name`, if git has one for it. A file in an
    /// untracked or ignored directory - which git names whole - is as that
    /// directory is.
    pub fn mark(&self, name: &OsStr, is_dir: bool) -> Option<Mark> {
        // The way out is not in the directory listed.
        if name == ".." {
            return None;
        }
        let name = composed(name);
        let name: &OsStr = &name;
        if let Some(&mark) = self.named.get(name) {
            return Some(mark);
        }
        if is_dir && let Some(&mark) = self.inside.get(name) {
            return Some(mark);
        }
        // What git says of the entry itself comes first - an ignored file in
        // an untracked directory is named as ignored - and only then what it
        // says of the directory it is all in.
        Path::new(name).ancestors().skip(1).filter(|dir| !dir.as_os_str().is_empty()).find_map(|dir| self.named.get(dir.as_os_str()).copied()).or(self.whole)
    }

    /// Take in a line of `git status --porcelain`, by its path from the
    /// directory listed.
    fn add(&mut self, path: PathBuf, mark: Mark) {
        let path = PathBuf::from(composed(path.as_os_str()).into_owned());
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
    // Git for Windows could end it as Windows does.
    let prefix = prefix.strip_suffix(b"\n").unwrap_or(&prefix);
    let prefix = prefix.strip_suffix(b"\r").unwrap_or(prefix);
    let output = git_bytes(dir, &["status", "--porcelain=v1", "-z", "--ignored", "--untracked-files=normal", "--", "."])?;
    let mut status = Status::default();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let (x, y) = (record[0], record[1]);
        // A rename or copy gives where it came from next, which is not here
        // any more - one in the index, or one in the work tree, as git sees
        // a file moved and then added with -N.
        if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            records.next();
        }
        let mark = match (x, y) {
            (b'?', b'?') => Mark::Untracked,
            (b'!', b'!') => Mark::Ignored,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Mark::Conflict,
            (_, b' ') => Mark::Staged,
            _ => Mark::Modified,
        };
        // The directory listed, or one it is inside, named whole: what is
        // listed is all of it as that one is.
        if record[3..].ends_with(b"/") && prefix.starts_with(&record[3..]) {
            status.whole = Some(mark);
            continue;
        }
        let Some(path) = record[3..].strip_prefix(prefix) else {
            continue;
        };
        let path = path.strip_suffix(b"/").unwrap_or(path);
        if path.is_empty() {
            continue;
        }
        status.add(from_git(path), mark);
    }
    Ok(status)
}

/// A name in Unicode's composed form, as it is when it already is - most
/// are. One that is not valid Unicode is left as it is.
fn composed(name: &OsStr) -> Cow<'_, OsStr> {
    use unicode_normalization::{UnicodeNormalization, is_nfc};
    match name.to_str() {
        Some(text) if !is_nfc(text) => Cow::Owned(OsString::from(text.nfc().collect::<String>())),
        _ => Cow::Borrowed(name),
    }
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

/// The directory `path` is in, and its name there: git is run from the one,
/// on the other, so a file anywhere - fed to a panel from all over - is
/// asked of the repository it is in.
fn where_and_name(path: &Path) -> Result<(&Path, &OsStr), String> {
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => Ok((dir, name)),
        _ => Err(t!("Not a file in a repository: {path}", path = path.display())),
    }
}

/// `git add` the file or directory at `path`: every change in it, new files
/// too, to go into the next commit.
pub fn stage(path: &Path) -> Result<(), String> {
    let (dir, name) = where_and_name(path)?;
    git_bytes(dir, &[OsStr::new("add"), OsStr::new("--"), name]).map(drop)
}

/// Take what is staged of `path` back out of the index, leaving the file as
/// it is. Before the first commit there is no HEAD to take it back to, and
/// it is taken out of the index altogether.
pub fn unstage(path: &Path) -> Result<(), String> {
    let (dir, name) = where_and_name(path)?;
    if git_bytes(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok() {
        git_bytes(dir, &[OsStr::new("restore"), OsStr::new("--staged"), OsStr::new("--"), name]).map(drop)
    } else {
        git_bytes(dir, &[OsStr::new("rm"), OsStr::new("--cached"), OsStr::new("-r"), OsStr::new("--quiet"), OsStr::new("--"), name]).map(drop)
    }
}

/// Throw away the changes to `path` that are not staged: it goes back to
/// what the index holds, which is HEAD's unless something was staged.
pub fn discard(path: &Path) -> Result<(), String> {
    let (dir, name) = where_and_name(path)?;
    git_bytes(dir, &[OsStr::new("restore"), OsStr::new("--worktree"), OsStr::new("--"), name]).map(drop)
}

/// The file at `path` as the last commit has it.
pub fn at_head(path: &Path) -> Result<Vec<u8>, String> {
    let (dir, name) = where_and_name(path)?;
    git_bytes(dir, &[OsStr::new("show"), head_spec(name).as_os_str()])
}

/// How big the file at `path` is as the last commit has it, asked before
/// reading it.
pub fn size_at_head(path: &Path) -> Result<u64, String> {
    let (dir, name) = where_and_name(path)?;
    let output = git_bytes(dir, &[OsStr::new("cat-file"), OsStr::new("-s"), head_spec(name).as_os_str()])?;
    let said = String::from_utf8_lossy(&output);
    said.trim().parse().map_err(|_| said.trim().to_string())
}

/// `HEAD:./name`, the file in the directory git runs in as HEAD has it.
fn head_spec(name: &OsStr) -> OsString {
    let mut spec = OsString::from("HEAD:./");
    spec.push(name);
    spec
}

/// A commit that changed a file, and where the file was then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCommit {
    pub hash: String,
    /// When it was made, in seconds since the epoch.
    pub when: i64,
    pub subject: String,
    /// The file's path in that commit, from the top of the repository, as
    /// git writes it: it may have had another name before a rename.
    pub path: Vec<u8>,
}

/// The commits that changed the file at `path`, newest first, followed back
/// through its renames. A merge is left out: what it holds of the file is
/// what one of its parents had.
pub fn file_history(path: &Path) -> Result<Vec<FileCommit>, String> {
    let (dir, name) = where_and_name(path)?;
    let output = git_bytes(
        dir,
        &[OsStr::new("log"), OsStr::new("--follow"), OsStr::new("-z"), OsStr::new("--name-only"), OsStr::new("--format=%x01%H%x00%ct%x00%s"), OsStr::new("--"), name],
    )?;
    let mut commits = Vec::new();
    for record in output.split(|&byte| byte == 1).filter(|record| !record.is_empty()) {
        let mut fields = record.split(|&byte| byte == 0);
        let (Some(hash), Some(when), Some(subject), Some(path)) = (fields.next(), fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let path = path.strip_prefix(b"\n").unwrap_or(path);
        if path.is_empty() {
            continue;
        }
        commits.push(FileCommit {
            hash: String::from_utf8_lossy(hash).into_owned(),
            when: String::from_utf8_lossy(when).trim().parse().unwrap_or(0),
            subject: String::from_utf8_lossy(subject).into_owned(),
            path: path.to_vec(),
        });
    }
    Ok(commits)
}

/// Who last changed a line, as `git blame` says: the commit's author, when
/// it was made, in seconds since the epoch, and its subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blamed {
    pub author: String,
    pub when: i64,
    pub summary: String,
}

/// Each line of the file at `path` as HEAD has it, by the commit that last
/// changed it: an index into the commits given back beside them, each told
/// once however many lines it changed.
pub fn blame(path: &Path) -> Result<(Vec<usize>, Vec<Blamed>), String> {
    let (dir, name) = where_and_name(path)?;
    let output = git_bytes(dir, &[OsStr::new("blame"), OsStr::new("--porcelain"), OsStr::new("HEAD"), OsStr::new("--"), name])?;
    Ok(parse_blame(&output))
}

/// `git blame --porcelain`: a line of the commit's hash and where the line
/// is, then - the first time a commit is met - lines of what it is, then
/// the line itself after a tab.
fn parse_blame(output: &[u8]) -> (Vec<usize>, Vec<Blamed>) {
    let mut lines = Vec::new();
    let mut commits: Vec<Blamed> = Vec::new();
    let mut by_hash: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut current = None;
    for line in output.split(|&byte| byte == b'\n') {
        if line.first() == Some(&b'\t') {
            if let Some(at) = current {
                lines.push(at);
            }
            continue;
        }
        let (key, value) = match line.iter().position(|&byte| byte == b' ') {
            Some(space) => (&line[..space], &line[space + 1..]),
            None => (line, &line[line.len()..]),
        };
        if key.len() == 40 && key.iter().all(u8::is_ascii_hexdigit) {
            let next = commits.len();
            let at = *by_hash.entry(key.to_vec()).or_insert(next);
            if at == next {
                commits.push(Blamed {
                    author: String::new(),
                    when: 0,
                    summary: String::new(),
                });
            }
            current = Some(at);
            continue;
        }
        let Some(commit) = current.and_then(|at| commits.get_mut(at)) else {
            continue;
        };
        let text = || String::from_utf8_lossy(value).into_owned();
        match key {
            b"author" => commit.author = text(),
            b"author-time" => commit.when = text().trim().parse().unwrap_or(0),
            b"summary" => commit.summary = text(),
            _ => {}
        }
    }
    (lines, commits)
}

/// The file `commit` is of, as it was in that commit - read in the
/// repository `dir` is in.
pub fn file_at(dir: &Path, commit: &FileCommit) -> Result<Vec<u8>, String> {
    git_bytes(dir, &[OsStr::new("cat-file"), OsStr::new("blob"), blob_spec(commit).as_os_str()])
}

/// How big the file `commit` is of was in it, asked before reading it.
pub fn file_size_at(dir: &Path, commit: &FileCommit) -> Result<u64, String> {
    let output = git_bytes(dir, &[OsStr::new("cat-file"), OsStr::new("-s"), blob_spec(commit).as_os_str()])?;
    let said = String::from_utf8_lossy(&output);
    said.trim().parse().map_err(|_| said.trim().to_string())
}

/// `hash:path`, the file as `commit` had it.
fn blob_spec(commit: &FileCommit) -> OsString {
    let mut spec = commit.hash.clone().into_bytes();
    spec.push(b':');
    spec.extend_from_slice(&commit.path);
    #[cfg(unix)]
    let spec = {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(&spec).to_os_string()
    };
    #[cfg(not(unix))]
    let spec = OsString::from(String::from_utf8_lossy(&spec).into_owned());
    spec
}

/// When each commit that changed anything under `dir` in the last `weeks`
/// weeks was made, in seconds since the epoch, newest first.
pub fn commit_times(dir: &Path, weeks: usize) -> Result<Vec<i64>, String> {
    let since = format!("--since={weeks}.weeks.ago");
    let output = git(dir, &["log", since.as_str(), "--format=%ct", "--", "."])?;
    Ok(output.lines().filter_map(|line| line.trim().parse().ok()).collect())
}

/// The commit `revision` names - a branch, a tag, `HEAD~3`, `stash@{0}` -
/// in the repository `dir` is in.
pub fn resolve(dir: &Path, revision: &str) -> Result<String, String> {
    let named = format!("{revision}^{{commit}}");
    match git(dir, &["rev-parse", "--verify", "--quiet", "--end-of-options", named.as_str()]) {
        Ok(commit) if !commit.trim().is_empty() => Ok(commit.trim().to_string()),
        _ => Err(t!("No such revision: {revision}", revision)),
    }
}

/// Everything in `commit`'s tree, directories too, as `git ls-tree` lists
/// it: NUL-separated, each with its size, and from the top whatever `dir`
/// is in the repository.
pub fn tree(dir: &Path, commit: &str) -> Result<Vec<u8>, String> {
    git_bytes(dir, &["ls-tree", "-r", "-t", "-l", "-z", "--full-tree", commit])
}

/// What a revision on R's list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RevisionKind {
    Branch,
    Remote,
    Tag,
    Stash,
    Commit,
}

/// A revision R can open: its name, when its commit was made, what that
/// commit - or the tag, or the stash - says it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub kind: RevisionKind,
    pub name: String,
    pub when: Option<i64>,
    pub subject: String,
    /// The branch checked out.
    pub current: bool,
}

/// Tags listed at most, the newest first: a repository with thousands of
/// them would bury everything else.
const TAGS: usize = 100;
/// The last commits listed.
const COMMITS: usize = 50;

/// Everything R lists for the repository `dir` is in: the local branches,
/// those of the remotes, the tags, the stashes and the last commits - in
/// that order, each kind as git sorts it but the tags, newest first.
pub fn revisions(dir: &Path) -> Result<Vec<Revision>, String> {
    let line = |text: &str| -> Vec<String> { text.split('\0').map(str::to_string).collect() };
    let refs = git(
        dir,
        &["for-each-ref", "--format=%(HEAD)%00%(refname)%00%(refname:short)%00%(creatordate:unix)%00%(contents:subject)", "refs/heads", "refs/remotes", "refs/tags"],
    )?;
    let mut found: Vec<Revision> = Vec::new();
    for fields in refs.lines().map(line) {
        let [head, full, short, when, subject] = fields.as_slice() else { continue };
        let kind = if full.starts_with("refs/heads/") {
            RevisionKind::Branch
        } else if full.starts_with("refs/remotes/") {
            // origin/HEAD is only another name for one of the others.
            if full.ends_with("/HEAD") {
                continue;
            }
            RevisionKind::Remote
        } else {
            RevisionKind::Tag
        };
        found.push(Revision {
            kind,
            name: short.clone(),
            when: when.parse().ok(),
            subject: subject.clone(),
            current: head == "*",
        });
    }
    // Newest tag first, and only so many.
    let (mut tags, mut rest): (Vec<Revision>, Vec<Revision>) = found.into_iter().partition(|revision| revision.kind == RevisionKind::Tag);
    tags.sort_by_key(|tag| std::cmp::Reverse(tag.when));
    tags.truncate(TAGS);
    rest.extend(tags);
    let stashes = git(dir, &["stash", "list", "--format=%gd%x00%ct%x00%gs"]).unwrap_or_default();
    for fields in stashes.lines().map(line) {
        if let [name, when, subject] = fields.as_slice() {
            rest.push(Revision {
                kind: RevisionKind::Stash,
                name: name.clone(),
                when: when.parse().ok(),
                subject: subject.clone(),
                current: false,
            });
        }
    }
    // A repository with no commit yet has none to list.
    let count = format!("-n{COMMITS}");
    let commits = git(dir, &["log", count.as_str(), "--format=%h%x00%ct%x00%s"]).unwrap_or_default();
    for fields in commits.lines().map(line) {
        if let [name, when, subject] = fields.as_slice() {
            rest.push(Revision {
                kind: RevisionKind::Commit,
                name: name.clone(),
                when: when.parse().ok(),
                subject: subject.clone(),
                current: false,
            });
        }
    }
    // Stable, so each kind keeps the order it was put in.
    rest.sort_by_key(|revision| revision.kind);
    Ok(rest)
}

/// When `commit` was made.
pub fn commit_time(dir: &Path, commit: &str) -> Option<std::time::SystemTime> {
    let seconds: u64 = git(dir, &["show", "-s", "--format=%ct", commit]).ok()?.trim().parse().ok()?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

/// git writing the contents of `object` - a file as a commit has it - to
/// the pipe of the process given back.
pub fn blob(dir: &Path, object: &str) -> std::io::Result<std::process::Child> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["cat-file", "blob", object])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
}

/// The top of the repository `dir` is in, and where `dir` is below it.
pub fn top_and_prefix(dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let top = git(dir, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(top.trim_end_matches(['\n', '\r']));
    let prefix = git_bytes(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = from_git(prefix.strip_suffix(b"\n").unwrap_or(&prefix));
    Ok((top, prefix))
}

/// The file git writes to each time HEAD moves - a commit, a checkout, a
/// reset - in the repository the file at `path` is in: its HEAD's log, or
/// HEAD itself where no log is kept. Its time says when HEAD last changed.
pub fn head_log(path: &Path) -> Option<PathBuf> {
    let (dir, _) = where_and_name(path).ok()?;
    let git_dir = PathBuf::from(git(dir, &["rev-parse", "--absolute-git-dir"]).ok()?.trim_end_matches(['\n', '\r']));
    let log = git_dir.join("logs").join("HEAD");
    Some(if log.is_file() { log } else { git_dir.join("HEAD") })
}

/// Run git in `dir`, giving back what it wrote, or what it said went wrong.
/// Nothing it could ask for is answered: a prompt would wait for good on a
/// terminal fm84 is drawing on.
fn git(dir: &Path, arguments: &[&str]) -> Result<String, String> {
    git_bytes(dir, arguments).map(|output| String::from_utf8_lossy(&output).into_owned())
}

/// As `git`, what it wrote as bytes: a file name need not be UTF-8.
fn git_bytes<S: AsRef<OsStr>>(dir: &Path, arguments: &[S]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        // A name is the file of that name: `*` or `[ab].rs` matching others
        // instead, or `:(top)` read as git's own, would stage or throw away
        // the changes of files no one chose.
        .env("GIT_LITERAL_PATHSPECS", "1")
        // Locks nothing: a status that refreshed the index could get in the
        // way of a commit being made in the shell beside it.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| t!("Cannot run git: {e}", e))?;
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
    Err(if message.is_empty() { format!("git {} failed", arguments[0].as_ref().to_string_lossy()) } else { message })
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

        // Inside an untracked or an ignored directory, and one below that:
        // all of it as the directory is, but the way out.
        // Git does not see a directory with nothing in it.
        std::fs::create_dir_all(dir.join("fresh/deeper")).unwrap();
        std::fs::write(dir.join("fresh/deeper/b.txt"), "").unwrap();
        std::fs::write(dir.join("fresh/x.log"), "").unwrap();
        std::fs::write(dir.join(".gitignore"), "build/\n*.log\n").unwrap();
        for (inside, mark) in [("fresh", Mark::Untracked), ("fresh/deeper", Mark::Untracked), ("build", Mark::Ignored)] {
            let status = super::status(&dir.join(inside)).unwrap();
            assert_eq!(status.mark(OsStr::new("a.txt"), false), Some(mark), "{inside}");
            assert_eq!(status.mark(OsStr::new(".."), true), None);
        }
        // But an ignored file in an untracked one is ignored.
        let fresh = super::status(&dir.join("fresh")).unwrap();
        assert_eq!(fresh.mark(OsStr::new("x.log"), false), Some(Mark::Ignored));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file moved and added with -N is a rename in the work tree, which
    /// git follows with where it came from: that is not a record of its own,
    /// to mark whatever its letters past the third happen to name.
    #[test]
    fn a_rename_in_the_work_tree_marks_only_where_it_went() {
        let Some(dir) = repository("worktree-rename") else {
            return;
        };
        let run = |arguments: &[&str]| assert!(Command::new("git").arg("-C").arg(&dir).args(arguments).output().unwrap().status.success());
        std::fs::write(dir.join("abcdefg.txt"), "a line long enough\nto be found again\nwhen it has moved\n").unwrap();
        std::fs::write(dir.join("defg.txt"), "left alone\n").unwrap();
        run(&["add", "."]);
        run(&["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "two"]);
        std::fs::rename(dir.join("abcdefg.txt"), dir.join("moved.txt")).unwrap();
        run(&["add", "-N", "moved.txt"]);

        let status = status(&dir).unwrap();
        assert_eq!(status.mark(OsStr::new("moved.txt"), false), Some(Mark::Modified));
        assert_eq!(status.mark(OsStr::new("defg.txt"), false), None, "untouched");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An accented name matches whichever way either side spells it.
    #[test]
    fn an_accent_matches_composed_or_not() {
        let (composed, decomposed) = ("caf\u{e9}.txt", "cafe\u{301}.txt");
        let sep = std::path::MAIN_SEPARATOR;
        for (from_git, on_disk) in [(composed, decomposed), (decomposed, composed)] {
            let mut status = Status::default();
            status.add(PathBuf::from(format!("r\u{e9}sum\u{e9}{sep}{from_git}")), Mark::Modified);
            assert_eq!(status.mark(OsStr::new(&format!("re\u{301}sume\u{301}{sep}{on_disk}")), false), Some(Mark::Modified));
            assert_eq!(status.mark(OsStr::new("re\u{301}sume\u{301}"), true), Some(Mark::Modified));
        }
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

    /// A file changed, staged, unstaged and its changes thrown away; and
    /// what the last commit holds of it, from a directory below the top.
    #[test]
    fn a_file_is_staged_unstaged_and_its_changes_discarded() {
        let Some(dir) = repository("stage") else {
            return;
        };
        let mark = |dir: &Path, name: &str| status(dir).unwrap().mark(OsStr::new(name), false);
        let file = dir.join("file.txt");
        std::fs::write(&file, "changed\n").unwrap();
        assert_eq!(mark(&dir, "file.txt"), Some(Mark::Modified));
        stage(&file).unwrap();
        assert_eq!(mark(&dir, "file.txt"), Some(Mark::Staged));
        unstage(&file).unwrap();
        assert_eq!(mark(&dir, "file.txt"), Some(Mark::Modified));
        assert_eq!(at_head(&file).unwrap(), b"main\n");
        discard(&file).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "main\n");
        assert_eq!(mark(&dir, "file.txt"), None);

        // A new file in a directory: staged, and taken out again.
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let new = dir.join("sub").join("new.txt");
        std::fs::write(&new, "new\n").unwrap();
        stage(&new).unwrap();
        assert_eq!(mark(&dir.join("sub"), "new.txt"), Some(Mark::Staged));
        // Not in HEAD, which git says.
        assert!(at_head(&new).is_err());
        unstage(&new).unwrap();
        assert_eq!(mark(&dir.join("sub"), "new.txt"), Some(Mark::Untracked));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Blame's porcelain read: each line by its commit, a commit's author,
    /// time and subject from the first of its lines, and the same commit met
    /// again further down - where git does not say them again - the same one.
    #[test]
    fn blame_is_read_line_by_line() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let output = format!("{a} 1 1 2\nauthor Ann\nauthor-mail <ann@x>\nauthor-time 100\nsummary First\nfilename f\n\tone\n{a} 2 2\n\ttwo\n{b} 3 3 1\nauthor Bob\nauthor-time 200\nsummary Second\nfilename f\n\tthree\n{a} 3 4 1\n\tfour\n");
        let (lines, commits) = parse_blame(output.as_bytes());
        assert_eq!(lines, [0, 0, 1, 0]);
        assert_eq!(
            commits[0],
            Blamed {
                author: "Ann".to_string(),
                when: 100,
                summary: "First".to_string()
            }
        );
        assert_eq!(
            commits[1],
            Blamed {
                author: "Bob".to_string(),
                when: 200,
                summary: "Second".to_string()
            }
        );
    }

    /// A name is the file of that name, never a pattern: staging or throwing
    /// away the changes of a file called `*` touches it alone, not every
    /// file beside it.
    #[test]
    fn a_name_like_a_pattern_is_only_itself() {
        let Some(dir) = repository("pattern") else {
            return;
        };
        let star = dir.join("*");
        std::fs::write(&star, "star\n").unwrap();
        let mut committed = Command::new("git");
        committed
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "star"])
            .env("GIT_LITERAL_PATHSPECS", "1");
        let file = dir.join("file.txt");
        let outcome = (|| {
            stage(&star)?;
            if !committed.output().is_ok_and(|output| output.status.success()) {
                return Err("could not commit".to_string());
            }
            std::fs::write(&star, "star changed\n").unwrap();
            std::fs::write(&file, "file changed\n").unwrap();
            stage(&star)?;
            let staged_beside = status(&dir)?.mark(OsStr::new("file.txt"), false);
            unstage(&star)?;
            discard(&star)?;
            Ok((staged_beside, std::fs::read_to_string(&star).unwrap(), std::fs::read_to_string(&file).unwrap()))
        })();
        std::fs::remove_dir_all(&dir).unwrap();
        let (staged_beside, star_now, file_now) = outcome.unwrap();
        assert_eq!(staged_beside, Some(Mark::Modified), "file.txt not staged with *");
        assert_eq!(star_now, "star\n", "* back as committed");
        assert_eq!(file_now, "file changed\n", "file.txt's changes kept");
    }

    /// Before the first commit there is no HEAD to unstage back to: what is
    /// staged comes out of the index all the same.
    #[test]
    fn a_file_is_unstaged_before_the_first_commit() {
        let dir = std::env::temp_dir().join(format!("fm84-git-unborn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if !Command::new("git").arg("-C").arg(&dir).args(["init", "--quiet"]).output().is_ok_and(|output| output.status.success()) {
            return;
        }
        let file = dir.join("first.txt");
        std::fs::write(&file, "first\n").unwrap();
        stage(&file).unwrap();
        assert_eq!(status(&dir).unwrap().mark(OsStr::new("first.txt"), false), Some(Mark::Staged));
        unstage(&file).unwrap();
        assert_eq!(status(&dir).unwrap().mark(OsStr::new("first.txt"), false), Some(Mark::Untracked));
        std::fs::remove_dir_all(&dir).unwrap();
    }
    /// A file's commits, newest first and back through a rename, and the
    /// file as each of them had it - asked from a directory below the top.
    #[test]
    fn a_files_history_follows_it_through_a_rename() {
        let Some(dir) = repository("history") else {
            return;
        };
        let run = |arguments: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
                .args(arguments)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        };
        std::fs::write(dir.join("file.txt"), "second\n").unwrap();
        run(&["commit", "--quiet", "-am", "second"]);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        run(&["mv", "file.txt", "sub/moved.txt"]);
        run(&["commit", "--quiet", "-m", "moved"]);
        let file = dir.join("sub").join("moved.txt");
        let history = file_history(&file).unwrap();
        let subjects: Vec<&str> = history.iter().map(|commit| commit.subject.as_str()).collect();
        assert_eq!(subjects, ["moved", "second", "first"]);
        assert_eq!(history[0].path, b"sub/moved.txt");
        assert_eq!(history[2].path, b"file.txt");
        assert!(history.iter().all(|commit| commit.hash.len() >= 40 && commit.when > 0));
        let below = dir.join("sub");
        assert_eq!(file_at(&below, &history[1]).unwrap(), b"second\n");
        assert_eq!(file_at(&below, &history[2]).unwrap(), b"main\n");
        // A file git has never seen has no history to step through.
        std::fs::write(below.join("new.txt"), "new\n").unwrap();
        assert!(file_history(&below.join("new.txt")).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
