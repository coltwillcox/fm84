//! Paths as they are typed and compared: `~`, relative to a panel, completed,
//! and whether two name the same entry.

use super::*;

/// Walk up from `path` until a directory that still exists is found. A panel's
/// directory can be removed underneath it - and so can several of its parents,
/// if something deleted a whole tree - so this climbs until it lands somewhere
/// listable. None only if even the root is unreachable.
pub fn nearest_existing_dir(path: &Path) -> Option<PathBuf> {
    let mut candidate = Some(path);
    while let Some(dir) = candidate {
        if dir.is_dir() {
            return Some(dir.to_path_buf());
        }
        candidate = dir.parent();
    }
    None
}

/// A path as typed into a dialog. `~` at the start is the home directory,
/// as in a shell, and a path that is not absolute is taken from `base`, the
/// directory the active panel is in - so a bare name copies beside the file.
pub fn typed_path(text: &str, base: &Path) -> PathBuf {
    let home = || env::home_dir().unwrap_or_default();
    let path = match text.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with(std::path::is_separator) => home().join(rest.trim_start_matches(std::path::is_separator)),
        _ => PathBuf::from(text),
    };
    base.join(path)
}

/// `.` and `..` worked out from the path as written, as a shell's `cd` does:
/// `/a/b/../c` is `/a/c`, even when `b` is a link somewhere else. The panel
/// shows the path it was given, and one with `..` in it would read as
/// nonsense in the title and in every path built from it.
pub fn tidy_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut tidy = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // Never above the root, or past the start of a relative path.
            Component::ParentDir => {
                if !tidy.pop() {
                    tidy.push(component);
                }
            }
            _ => tidy.push(component),
        }
    }
    tidy
}

/// Tab in a path being typed: the last name in it filled out as far as the
/// entries it could be agree, and a directory's separator added once only
/// one is left. None when there is nothing to add. Hidden entries are offered
/// only when they are shown, or when a dot has been typed asking for one.
/// Smart case, as everywhere else: lower case matches either.
///
/// It reads the directory, so it waits on a dead mount; the caller runs it
/// where that cannot hold up the screen.
pub fn complete_path(text: &str, base: &Path, show_hidden: bool) -> Option<String> {
    let split = text.rfind(std::path::is_separator).map_or(0, |index| index + 1);
    let (head, prefix) = text.split_at(split);
    let dir = if head.is_empty() { base.to_path_buf() } else { typed_path(head, base) };
    let fold = !prefix.chars().any(char::is_uppercase);
    let folded = |name: &str| -> Vec<char> { name.chars().map(|c| if fold { crate::find::fold_char(c) } else { c }).collect() };
    let wanted = folded(prefix);
    let hidden = show_hidden || prefix.starts_with('.');

    let mut candidates: Vec<(String, bool)> = read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let shown = hidden || !name.starts_with('.');
            // Through a link, so one to a directory completes as one.
            (shown && folded(&name).starts_with(&wanted)).then(|| (name, entry.path().is_dir()))
        })
        .collect();
    candidates.sort();
    let (first, first_is_dir) = candidates.first()?.clone();

    let completed = if candidates.len() == 1 {
        if first_is_dir { format!("{first}{}", std::path::MAIN_SEPARATOR) } else { first }
    } else {
        // As far as every one of them agrees. What was typed stays as it was
        // typed, and the rest comes in the case of the first.
        let first_chars: Vec<char> = first.chars().collect();
        let folded_first = folded(&first);
        let common = candidates[1..]
            .iter()
            .fold(first_chars.len(), |length, (name, _)| folded_first.iter().zip(folded(name)).take(length).take_while(|(a, b)| **a == *b).count());
        let typed = wanted.len();
        prefix.chars().chain(first_chars[typed.min(common)..common].iter().copied()).collect()
    };
    let result = format!("{head}{completed}");
    (result != text && completed.chars().count() >= prefix.chars().count()).then_some(result)
}

pub fn get_current_dir() -> Result<PathBuf, Error> {
    env::current_dir()
}

/// True for an ordinary file, following a symlink to one. Pipes, sockets and
/// devices are not, and nothing should open them to read: opening a named pipe
/// waits for a writer that may never come, and a device like /dev/zero reports
/// a size of nothing and then never ends.
pub fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

pub(super) fn not_regular(path: &Path) -> Error {
    Error::new(ErrorKind::InvalidInput, format!("Not a regular file: {}", path.display()))
}

/// True for a name that stays in the directory it is given in: one plain
/// component, so not "." or "..", and no separator - "../x" or "sub/x" would
/// rename or create somewhere else entirely.
pub fn is_plain_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(only)), None) if only == name
    )
}

/// True when two paths in the same directory name one entry: on a
/// case-insensitive filesystem, "readme" and "README" do. Links are not
/// followed - a symlink to a file is an entry of its own, and renaming the
/// file over it would destroy it.
#[cfg(unix)]
pub fn is_same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (a.symlink_metadata(), b.symlink_metadata()) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Windows has no stable file identity in std, but its directories are
/// case-insensitive, so within one of them a name differing only in case is
/// the same entry.
#[cfg(windows)]
pub fn is_same_entry(a: &Path, b: &Path) -> bool {
    let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().to_lowercase());
    a.parent() == b.parent() && name(a).is_some() && name(a) == name(b) && path_exists(a)
}

#[cfg(not(any(unix, windows)))]
pub fn is_same_entry(a: &Path, b: &Path) -> bool {
    a == b
}

/// A name beside `path` that nothing has taken, for a copy kept beside the
/// file in its way: `notes (2).txt`, then `(3)`, as far as it has to go. An
/// archive's double ending stays whole - `src (2).tar.gz`, not
/// `src.tar (2).gz` - and a name that is all ending, as `.bashrc` is, takes
/// the number after it.
pub fn free_name(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let lower = name.to_lowercase();
    let double = [".tar.gz", ".tar.zst", ".tar.xz", ".tar.bz2"].into_iter().find(|ending| lower.ends_with(ending) && lower.len() > ending.len());
    let split = match double {
        Some(ending) => name.len() - ending.len(),
        None => match name.rfind('.') {
            Some(0) | None => name.len(),
            Some(dot) => dot,
        },
    };
    // Not split where it would cut a character - a lower case that is
    // shorter than the name it came from.
    let split = if name.is_char_boundary(split) { split } else { name.len() };
    let (stem, ending) = name.split_at(split);
    (2u64..).map(|number| path.with_file_name(format!("{stem} ({number}){ending}"))).find(|candidate| !path_exists(candidate)).expect("some number is free")
}

/// True if anything occupies this path, including a dangling symlink - which
/// Path::exists() reports as absent because it follows the link.
pub fn path_exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn dots_are_worked_out_as_written() {
        let tidy = |path: &str| tidy_path(Path::new(path));
        assert_eq!(tidy("/a/b/../c"), Path::new("/a/c"));
        assert_eq!(tidy("/a/./b/"), Path::new("/a/b"));
        assert_eq!(tidy("/a/b/../.."), Path::new("/"));
        // Never above the root.
        assert_eq!(tidy("/../.."), Path::new("/"));
        assert_eq!(tidy("../x"), Path::new("../x"));
    }

    #[test]
    fn tab_fills_out_a_name_as_far_as_the_entries_agree() {
        let dir = std::env::temp_dir().join(format!("fm84-complete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["Documents", "downloads", "music", ".config"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        fs::write(dir.join("music.txt"), "").unwrap();
        let base = dir.display().to_string();
        let complete = |text: &str, hidden: bool| complete_path(text, &dir, hidden);
        let sep = std::path::MAIN_SEPARATOR;

        // One left: its whole name, and a separator after a directory.
        assert_eq!(complete(&format!("{base}/Doc"), false), Some(format!("{base}/Documents{sep}")));
        // Smart case: lower case finds either, and the name comes as it is.
        assert_eq!(complete(&format!("{base}/doc"), false), Some(format!("{base}/Documents{sep}")));
        assert_eq!(complete(&format!("{base}/Dow"), false), None);
        // Several: as far as they agree, what was typed kept as typed.
        assert_eq!(complete(&format!("{base}/d"), false), Some(format!("{base}/do")));
        assert_eq!(complete(&format!("{base}/mu"), false), Some(format!("{base}/music")));
        // Nothing further to add, and nothing that matches.
        assert_eq!(complete(&format!("{base}/music"), false), None);
        assert_eq!(complete(&format!("{base}/x"), false), None);
        // From the panel's directory, and a hidden one only when asked for.
        assert_eq!(complete("Doc", false), Some(format!("Documents{sep}")));
        assert_eq!(complete("c", false), None);
        assert_eq!(complete("c", true), None);
        assert_eq!(complete(".c", false), Some(format!(".config{sep}")));
        assert_eq!(complete("", true).as_deref(), None);
        // Not a directory at all.
        assert_eq!(complete(&format!("{base}/nope/x"), false), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The branch, from anywhere under the repository; the commit when
    /// there is none; through a `.git` file, as a worktree has; and nothing
    /// outside one.
    #[test]
    fn the_git_branch_a_directory_is_on() {
        let dir = std::env::temp_dir().join(format!("fm84-git-branch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("src").join("deep")).unwrap();
        fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/feature/tabs\n").unwrap();
        assert_eq!(git_branch(&repo).as_deref(), Some("feature/tabs"));
        assert_eq!(git_branch(&repo.join("src").join("deep")).as_deref(), Some("feature/tabs"));

        fs::write(repo.join(".git").join("HEAD"), "3d8eea0a1b2c3d4e5f60718293a4b5c6d7e8f901\n").unwrap();
        assert_eq!(git_branch(&repo).as_deref(), Some("3d8eea0"));

        let worktree = dir.join("worktree");
        let worktree_git = dir.join("gitdirs").join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&worktree_git).unwrap();
        fs::write(worktree.join(".git"), "gitdir: ../gitdirs/worktree\n").unwrap();
        fs::write(worktree_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(git_branch(&worktree).as_deref(), Some("main"));

        let outside = dir.join("outside");
        fs::create_dir_all(&outside).unwrap();
        // Unless the temporary directory is itself in a repository.
        assert_eq!(git_branch(&outside), git_branch(&std::env::temp_dir()));
        fs::remove_dir_all(&dir).unwrap();
    }
}
