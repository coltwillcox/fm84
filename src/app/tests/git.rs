//! Alt+G: git from the panel.

use super::*;
use crate::app::{Dialog, GitAction, Screen};

#[test]
fn alt_g_asks_before_discarding_and_compares_with_head() {
    let dir = std::env::temp_dir().join(format!("fm84-alt-g-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !git(&["init", "--quiet"]) {
        return;
    }
    let file = dir.join("notes.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    assert!(git(&["add", "notes.txt"]) && git(&["commit", "--quiet", "-m", "first"]));
    std::fs::write(&file, "one\nthree\n").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_git(vec![(file.clone(), false)]);
    app_state.git_compare_head();
    let Screen::Diff(state) = &app_state.screen else { panic!("no Compare view: {:?}", app_state.error) };
    assert_eq!(state.left_path, dir.join("notes.txt @ HEAD"));
    assert_eq!(state.right_path, file);
    app_state.close_diff();

    // D asks; only the answer throws the change away.
    app_state.open_git(vec![(file.clone(), false)]);
    app_state.git_act(GitAction::Discard);
    assert!(matches!(app_state.dialog, Some(Dialog::Git { discarding: true, .. })));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
    app_state.git_act(GitAction::Discard);
    assert!(app_state.dialog.is_none());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
    assert_eq!(app_state.find_note.as_deref(), Some("Changes discarded"));

    // A directory has no one file to compare.
    app_state.open_git(vec![(dir.clone(), true)]);
    app_state.git_compare_head();
    assert_eq!(app_state.error.as_deref(), Some("Only a file can be compared with HEAD"));
    std::fs::remove_dir_all(&dir).unwrap();
}
