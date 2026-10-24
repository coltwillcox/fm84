//! The git branches a directory's repository has, and changing to another -
//! by running git itself, which knows a repository's every layout and rule,
//! rather than reading refs the way the path bar's label does.

use std::path::Path;
use std::process::{Command, Stdio};

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
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Cannot run git: {e}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
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
