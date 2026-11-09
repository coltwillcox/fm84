//! Ctrl+Z: the last job's journal, taken back a step at a time.

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fm84-undo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app() -> AppState {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state
}

/// Poll until `done`, or fail after a while.
fn wait(app_state: &mut AppState, done: impl Fn(&AppState) -> bool) {
    let started = Instant::now();
    while !done(app_state) {
        assert!(started.elapsed() < Duration::from_secs(10), "the job never got there");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
}

fn finish(app_state: &mut AppState) {
    wait(app_state, |app_state| app_state.workers.job.is_none());
}

fn undo(app_state: &mut AppState) {
    app_state.undo_last();
    finish(app_state);
}

#[test]
fn a_copy_undone_is_deleted_and_the_original_kept() {
    let dir = scratch("copy");
    std::fs::create_dir_all(dir.join("tree/inner")).unwrap();
    std::fs::write(dir.join("tree/inner/a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    std::fs::create_dir(dir.join("to")).unwrap();

    let mut app_state = app();
    let items = vec![(dir.join("tree"), dir.join("to/tree"), true), (dir.join("b.txt"), dir.join("to/b.txt"), false)];
    app_state.start_transfer(items, true, crate::app::Clashes::Ask);
    finish(&mut app_state);
    assert!(dir.join("to/tree/inner/a.txt").exists());
    assert_eq!(app_state.undo.len(), 1);

    undo(&mut app_state);
    assert_eq!(app_state.error, None);
    assert!(!dir.join("to/tree").exists());
    assert!(!dir.join("to/b.txt").exists());
    assert!(dir.join("tree/inner/a.txt").exists());
    assert!(dir.join("b.txt").exists());
    assert!(app_state.undo.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_move_undone_goes_back() {
    let dir = scratch("move");
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree/a.txt"), "a").unwrap();
    std::fs::create_dir(dir.join("to")).unwrap();

    let mut app_state = app();
    app_state.start_transfer(vec![(dir.join("tree"), dir.join("to/tree"), true)], false, crate::app::Clashes::Ask);
    finish(&mut app_state);
    assert!(!dir.join("tree").exists());

    undo(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(dir.join("tree/a.txt")).unwrap(), "a");
    assert!(!dir.join("to/tree").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A copy worked on after it was made is not deleted by taking the copy
/// back: the undo asks, and a skip leaves it.
#[test]
fn a_copy_changed_since_is_refused() {
    let dir = scratch("changed");
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree/a.txt"), "a").unwrap();

    let mut app_state = app();
    app_state.start_transfer(vec![(dir.join("tree"), dir.join("copy"), true)], true, crate::app::Clashes::Ask);
    finish(&mut app_state);
    std::fs::write(dir.join("copy/a.txt"), "worked on, and longer").unwrap();

    app_state.undo_last();
    wait(&mut app_state, |app_state| app_state.workers.job.as_ref().is_some_and(|job| job.problem.is_some()));
    let problem = app_state.workers.job.as_ref().unwrap().problem.as_ref().unwrap();
    assert_eq!(problem.path, dir.join("copy"));
    assert!(problem.message.contains("has changed since"), "{}", problem.message);
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    finish(&mut app_state);
    assert_eq!(app_state.error.as_deref(), Some("Undo finished, 1 entry skipped"));
    assert_eq!(std::fs::read_to_string(dir.join("copy/a.txt")).unwrap(), "worked on, and longer");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Writing over a file cannot be taken back - the file written over is
/// gone - so nothing of it is kept to undo.
#[test]
fn a_copy_over_a_file_is_not_kept_to_undo() {
    let dir = scratch("over");
    std::fs::write(dir.join("a.txt"), "new").unwrap();
    std::fs::create_dir(dir.join("to")).unwrap();
    std::fs::write(dir.join("to/a.txt"), "old").unwrap();

    let mut app_state = app();
    app_state.start_transfer(vec![(dir.join("a.txt"), dir.join("to/a.txt"), false)], true, crate::app::Clashes::ReplaceAll);
    finish(&mut app_state);
    assert!(app_state.undo.is_empty());

    app_state.undo_last();
    assert!(app_state.workers.job.is_none());
    assert_eq!(app_state.find_note.as_deref(), Some("Nothing to undo"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Names swapped are swapped back, through a scratch name, and something
/// that has taken an old name since keeps it.
#[test]
fn renames_are_put_back_together() {
    let dir = scratch("rename");
    for name in ["a", "b", "c"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let moves = vec![(dir.join("a"), dir.join("b")), (dir.join("b"), dir.join("a")), (dir.join("c"), dir.join("d"))];
    crate::app::multi_rename::rename_all(&moves).unwrap();
    let mut app_state = app();
    app_state.journal_renames(&moves);
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!((read("a"), read("b"), read("d")), ("b".into(), "a".into(), "c".into()));
    // Something new where `c` was.
    std::fs::write(dir.join("c"), "new").unwrap();

    app_state.undo_last();
    wait(&mut app_state, |app_state| app_state.workers.job.as_ref().is_some_and(|job| job.problem.is_some()));
    assert_eq!(app_state.workers.job.as_ref().unwrap().problem.as_ref().unwrap().path, dir.join("d"));
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    finish(&mut app_state);
    assert_eq!((read("a"), read("b"), read("c"), read("d")), ("a".into(), "b".into(), "new".into(), "c".into()));
    assert_no_scratch(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn assert_no_scratch(dir: &Path) {
    let left: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().filter(|entry| entry.file_name().to_string_lossy().starts_with(".fm84")).collect();
    assert!(left.is_empty(), "{left:?}");
}

/// The journal is a stack: each Ctrl+Z takes back the one before.
#[test]
fn each_undo_takes_back_the_job_before() {
    let dir = scratch("stack");
    std::fs::write(dir.join("a"), "a").unwrap();
    let mut app_state = app();
    for (from, to) in [("a", "b"), ("b", "c")] {
        crate::fs_ops::rename_path(dir.join(from), dir.join(to)).unwrap();
        app_state.journal_renames(&[(dir.join(from), dir.join(to))]);
    }
    undo(&mut app_state);
    assert!(dir.join("b").exists());
    undo(&mut app_state);
    assert!(dir.join("a").exists());
    assert!(!dir.join("b").exists() && !dir.join("c").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+E from the start: the panel's names in the editor, edited, saved
/// to a plan, run - and then taken back with Ctrl+Z.
#[test]
fn a_directory_edited_as_text_and_undone() {
    let dir = scratch("dir-edit");
    for name in ["a", "b", "c"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    std::fs::create_dir(dir.join("d")).unwrap();
    let mut app_state = app();
    app_state.options.delete_to_trash = false;
    app_state.open_dir(crate::app::Side::Left, dir.clone(), None);
    app_state.active_side = crate::app::Side::Left;
    app_state.open_dir_edit();
    let lines = app_state.editor().unwrap().lines.clone();
    assert_eq!(lines, ["1 d/", "2 a", "3 b", "4 c"]);

    // a and b swapped, c deleted, d copied.
    app_state.editor_mut().unwrap().lines = ["1 d/", "1 e/", "2 b", "3 a"].map(String::from).to_vec();
    assert_eq!(app_state.editor_save(), Ok(crate::app::Saved::Planned));
    assert!(matches!(app_state.dialog, Some(crate::app::Dialog::DirEdit(_))));
    app_state.apply_dir_edit();
    assert!(!app_state.is_editing());
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!((read("a"), read("b")), ("b".into(), "a".into()));
    assert!(!dir.join("c").exists());
    assert!(dir.join("e").is_dir());

    // The delete was for good, so only the rest comes back.
    undo(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!((read("a"), read("b")), ("a".into(), "b".into()));
    assert!(!dir.join("e").exists());
    assert_no_scratch(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A save with nothing changed says so, and lines that cannot be a plan
/// keep the editor open with the reason.
#[test]
fn a_directory_edit_with_nothing_or_nonsense_in_it() {
    let dir = scratch("dir-edit-none");
    std::fs::write(dir.join("a"), "a").unwrap();
    let mut app_state = app();
    app_state.open_dir(crate::app::Side::Left, dir.clone(), None);
    app_state.active_side = crate::app::Side::Left;
    app_state.open_dir_edit();
    assert_eq!(app_state.editor_save(), Ok(crate::app::Saved::Done));
    assert_eq!(app_state.find_note.as_deref(), Some("Nothing to change"));
    app_state.editor_mut().unwrap().lines = vec!["a".into()];
    assert_eq!(app_state.editor_save(), Err("Line 1: no entry number in front of the name".into()));
    assert!(app_state.is_editing());
    // Nothing was written where the lines came from.
    assert!(dir.is_dir());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A move undone only puts the entry back, so working on it since - a file
/// saved in a directory moved, a file moved and then edited - does not stop
/// it: nothing is lost by moving it home. Another entry in its place does.
#[cfg(unix)]
#[test]
fn a_move_worked_on_since_still_goes_back() {
    let dir = scratch("move-worked-on");
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree/a.txt"), "a").unwrap();
    std::fs::write(dir.join("note.txt"), "short").unwrap();
    std::fs::create_dir(dir.join("to")).unwrap();

    let mut app_state = app();
    let items = vec![(dir.join("tree"), dir.join("to/tree"), true), (dir.join("note.txt"), dir.join("to/note.txt"), false)];
    app_state.start_transfer(items, false, crate::app::Clashes::Ask);
    finish(&mut app_state);
    // Worked on since: a file added in the directory, the file made longer,
    // with times a minute on, past any clock's tick.
    std::fs::write(dir.join("to/tree/b.txt"), "b").unwrap();
    std::fs::write(dir.join("to/note.txt"), "a good deal longer").unwrap();
    let later = std::time::SystemTime::now() + Duration::from_secs(60);
    for path in [dir.join("to/tree"), dir.join("to/note.txt")] {
        std::fs::File::open(&path).unwrap().set_modified(later).unwrap();
    }

    undo(&mut app_state);
    assert_eq!(app_state.error, None);
    assert!(app_state.workers.job.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("tree/b.txt")).unwrap(), "b");
    assert_eq!(std::fs::read_to_string(dir.join("note.txt")).unwrap(), "a good deal longer");
    std::fs::remove_dir_all(&dir).unwrap();
}
