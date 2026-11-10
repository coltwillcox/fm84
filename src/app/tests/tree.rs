//! Alt+F10: the panel as the tree of its drive's directories, opened down
//! to where it is; Right and Left open and close; Enter sends the other
//! panel; Esc back to the files, where the cursor is.

use super::*;
use crate::app::TreeMark;
use std::path::{Path, PathBuf};

/// The rows under `dir`, one level down, by name and mark.
fn under(app_state: &AppState, dir: &Path) -> Vec<(String, TreeMark)> {
    let tree = app_state.left.tree.as_ref().unwrap();
    tree.rows.iter().filter(|row| row.path.parent() == Some(dir)).map(|row| (row.path.file_name().unwrap().to_string_lossy().into_owned(), row.mark)).collect()
}

fn at(app_state: &AppState) -> PathBuf {
    let tree = app_state.left.tree.as_ref().unwrap();
    tree.rows[tree.cursor].path.clone()
}

#[test]
fn the_tree_opens_down_to_the_panel_and_sends_the_other_one() {
    let dir = std::env::temp_dir().join(format!("fm84-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["a/b/c", "a/x", ".hidden", "z"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    std::fs::write(dir.join("a/file.txt"), "not a directory").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    // Hidden directories are left out when F11 hides them.
    app_state.options.show_hidden = false;
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.join("a/b"), None);
    app_state.toggle_tree();

    // Down from the root to the panel's directory, each level open, the
    // cursor on it. Files and hidden directories are not in it.
    let tree = app_state.left.tree.as_ref().unwrap();
    assert_eq!(tree.rows[0].path, dir.ancestors().last().unwrap());
    assert_eq!(at(&app_state), dir.join("a/b"));
    assert_eq!(under(&app_state, &dir), [("a".to_string(), TreeMark::Open), ("z".to_string(), TreeMark::Unread)]);
    assert_eq!(under(&app_state, &dir.join("a")), [("b".to_string(), TreeMark::Open), ("x".to_string(), TreeMark::Unread)]);
    assert_eq!(under(&app_state, &dir.join("a/b")), [("c".to_string(), TreeMark::Unread)]);

    // Right goes down into an open one; on one not read, reads and opens it
    // - found to hold nothing, it says so.
    app_state.tree_expand();
    assert_eq!(at(&app_state), dir.join("a/b/c"));
    app_state.tree_expand();
    assert_eq!(under(&app_state, &dir.join("a/b")), [("c".to_string(), TreeMark::Empty)]);

    // Left goes up from one closed, then closes the one it is on.
    app_state.tree_collapse();
    assert_eq!(at(&app_state), dir.join("a/b"));
    app_state.tree_collapse();
    assert!(under(&app_state, &dir.join("a/b")).is_empty(), "closed");
    assert_eq!(under(&app_state, &dir.join("a"))[0], ("b".to_string(), TreeMark::Closed));
    app_state.tree_expand();
    assert_eq!(under(&app_state, &dir.join("a/b")).len(), 1, "open again");

    // Down to x, Enter: the other panel goes there, the tree stays.
    app_state.tree_move(2);
    assert_eq!(at(&app_state), dir.join("a/x"));
    app_state.tree_enter();
    assert_eq!(app_state.right.dir, dir.join("a/x"));
    assert!(app_state.left.tree.is_some());

    // A directory made meanwhile shows once the panel is read again.
    std::fs::create_dir(dir.join("a/new")).unwrap();
    app_state.reload_panel(Side::Left, None);
    let started = std::time::Instant::now();
    while !under(&app_state, &dir.join("a")).iter().any(|(name, _)| name == "new") {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never read again");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_trees();
    }

    // Esc: the panel lists files again, in the directory under the cursor.
    app_state.toggle_tree();
    assert!(app_state.left.tree.is_none());
    assert_eq!(app_state.left.dir, dir.join("a/x"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A directory slow to read shows as being read, and is taken in once it
/// comes, without holding anything up.
#[test]
fn a_slow_directory_is_taken_in_when_it_comes() {
    let dir = std::env::temp_dir().join(format!("fm84-tree-slow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("one/two")).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.toggle_tree();
    // No waiting at all from here on.
    app_state.workers.listing_wait = std::time::Duration::ZERO;
    app_state.tree_move(1);
    app_state.tree_expand();
    let started = std::time::Instant::now();
    while under(&app_state, &dir.join("one")).is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never read");
        assert!(matches!(app_state.left.tree.as_ref().unwrap().rows[app_state.left.tree.as_ref().unwrap().cursor].mark, TreeMark::Reading | TreeMark::Open));
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_trees();
    }
    assert_eq!(under(&app_state, &dir.join("one")), [("two".to_string(), TreeMark::Unread)]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Letters typed go to the first directory on show from the cursor whose
/// name starts with them; Down goes to the next; one that none matches is
/// not taken; any other key lets the typing go.
#[test]
fn typing_finds_a_directory_in_the_tree() {
    let dir = std::env::temp_dir().join(format!("fm84-tree-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["alpha", "beta", "bravo", "charlie"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.toggle_tree();
    app_state.tree_expand();
    app_state.tree_search_add('B');
    assert_eq!(at(&app_state), dir.join("beta"));
    app_state.tree_search_add('r');
    assert_eq!(at(&app_state), dir.join("bravo"));
    app_state.tree_search_add('x');
    assert_eq!(app_state.search_input, "Br", "no match, not taken");
    app_state.tree_search_back();
    // Up: the one before that matches. (Down would go on round the whole
    // drive's tree, which holds /bin and the like.)
    app_state.tree_search_next(false);
    assert_eq!(at(&app_state), dir.join("beta"), "back to the b before");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `*` reads and opens everything under the cursor, as a job - a link to a
/// directory listed, but not gone through.
#[test]
fn star_opens_everything_below() {
    let dir = std::env::temp_dir().join(format!("fm84-tree-star-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a/b/c/d")).unwrap();
    std::fs::create_dir_all(dir.join("e/f")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&dir, dir.join("e/loop")).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.toggle_tree();
    app_state.tree_open_all();
    let started = std::time::Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.error, None);
    assert_eq!(under(&app_state, &dir.join("a/b/c")), [("d".to_string(), TreeMark::Empty)]);
    #[cfg(unix)]
    assert_eq!(under(&app_state, &dir.join("e")), [("f".to_string(), TreeMark::Empty), ("loop".to_string(), TreeMark::Unread)]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Inside an archive the tree is of the archive, from its top, read from
/// its index: the other panel sent into it, a directory copied out, and the
/// archive's whole refused.
#[test]
fn the_tree_inside_an_archive_is_of_the_archive() {
    use crate::archive::fixtures::{Put, scratch, zip};
    let dir = scratch("tree-zip");
    let archive = dir.join("photos.zip");
    zip(&archive, &[("2024/june/beach.jpg", Put::File(b"sand")), ("2024/july/", Put::Dir), ("docs/", Put::Dir), ("readme.txt", Put::File(b"hi"))]);
    std::fs::create_dir_all(dir.join("out")).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Right, dir.join("out"), None);
    app_state.open_dir(Side::Left, archive.join("2024/june"), None);
    app_state.toggle_tree();
    let tree = app_state.left.tree.as_ref().unwrap();
    assert_eq!(tree.rows[0].path, archive, "the archive is the top");
    assert_eq!(at(&app_state), archive.join("2024/june"));
    assert_eq!(under(&app_state, &archive), [("2024".to_string(), TreeMark::Open), ("docs".to_string(), TreeMark::Unread)]);
    assert_eq!(
        under(&app_state, &archive.join("2024")),
        [("july".to_string(), TreeMark::Unread), ("june".to_string(), TreeMark::Empty)],
        "june holds a file and no directories"
    );
    assert!(app_state.panel_archive(Side::Left).is_some(), "the panel stays in it");

    // Enter: the other panel into the archive there.
    app_state.tree_move(-1);
    assert_eq!(at(&app_state), archive.join("2024/july"));
    app_state.tree_enter();
    assert_eq!(app_state.right.dir, archive.join("2024/july"));
    assert_eq!(app_state.panel_archive(Side::Right), Some(&archive));

    // Copied out: june, from the archive to the disk.
    app_state.open_dir(Side::Right, dir.join("out"), None);
    app_state.tree_move(1);
    app_state.start_extract(archive.clone(), vec![(PathBuf::from("2024/june/beach.jpg"), dir.join("out/beach.jpg"))], crate::app::Clashes::Ask, None);
    let started = std::time::Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(std::fs::read_to_string(dir.join("out/beach.jpg")).unwrap(), "sand");

    // `*` reads the archive's index, all of it.
    app_state.tree_move(isize::MIN);
    app_state.tree_open_all();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(under(&app_state, &archive), [("2024".to_string(), TreeMark::Open), ("docs".to_string(), TreeMark::Empty)]);

    // Esc: the panel lists that directory of the archive.
    app_state.tree_move(isize::MAX);
    assert_eq!(at(&app_state), archive.join("docs"));
    app_state.toggle_tree();
    assert_eq!(app_state.left.dir, archive.join("docs"));
    std::fs::remove_dir_all(&dir).unwrap();
}
