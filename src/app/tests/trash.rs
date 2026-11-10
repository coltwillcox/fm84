//! Alt+T: the trash as a folder - listed by the names things had, looked
//! into and copied out of as an archive is, put back and emptied. Against
//! the stand-in trash the tests have, never the real one.

use super::*;
use crate::archive::trash::{system::throw, trash_folder};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn finish(app_state: &mut AppState) {
    let started = Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < Duration::from_secs(10), "the job never finished");
        if let Some(job) = app_state.workers.job.as_mut().filter(|job| job.problem.is_some()) {
            job.answer(crate::app::Answer::Skip);
        }
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
}

fn names(app_state: &AppState) -> Vec<String> {
    let mut names: Vec<String> = app_state.left.children.iter().filter(|item| !item.is_parent()).map(|item| item.name_full.clone()).collect();
    names.sort();
    names
}

#[test]
fn the_trash_is_looked_into_put_back_and_emptied() {
    let dir = std::env::temp_dir().join(format!("fm84-trash-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (home, keep) = (dir.join("home"), dir.join("kept"));
    std::fs::create_dir_all(home.join("old/photos")).unwrap();
    std::fs::create_dir_all(&keep).unwrap();
    std::fs::write(home.join("a.txt"), "first").unwrap();
    std::fs::write(home.join("old/a.txt"), "second").unwrap();
    std::fs::write(home.join("old/photos/sun.jpg"), "sun").unwrap();
    throw(&home.join("a.txt"), &keep, 1_700_000_000);
    throw(&home.join("old/a.txt"), &keep, 1_700_000_100);
    throw(&home.join("old/photos"), &keep, 1_700_000_200);

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, home.clone(), None);
    app_state.toggle_trash();
    let folder = trash_folder().unwrap();
    assert_eq!(app_state.left.dir, folder);
    assert!(app_state.panel_in_trash(Side::Left));
    // By the names they had, two of one name told apart - the latest
    // keeping it.
    assert_eq!(names(&app_state), ["a (2).txt", "a.txt", "photos"]);
    let last = app_state.left.children.iter().find(|item| item.name_full == "a.txt").unwrap();
    assert_eq!(last.modified_at, Some(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_100)));

    // A directory that went is gone into, and what is in it copied out.
    app_state.open_dir(Side::Left, folder.join("photos"), None);
    assert_eq!(names(&app_state), ["sun.jpg"]);
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    app_state.start_extract(folder.clone(), vec![(PathBuf::from("photos/sun.jpg"), out.join("sun.jpg"))], crate::app::Clashes::Ask, None);
    finish(&mut app_state);
    assert_eq!(std::fs::read_to_string(out.join("sun.jpg")).unwrap(), "sun");
    // Only the whole of what went is put back.
    app_state.restore_from_trash(vec![(folder.join("photos/sun.jpg"), false)]);
    assert_eq!(app_state.error.take().as_deref(), Some("Only what went to the trash whole can be put back or emptied"));
    assert!(app_state.workers.job.is_none());

    // Put back where it came from.
    app_state.open_dir(Side::Left, folder.clone(), None);
    app_state.restore_from_trash(vec![(folder.join("a.txt"), false)]);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(home.join("old/a.txt")).unwrap(), "second");
    assert_eq!(names(&app_state), ["a.txt", "photos"]);

    // Its place taken since: asked about, and skipped, it stays.
    std::fs::write(home.join("a.txt"), "in the way").unwrap();
    app_state.restore_from_trash(vec![(folder.join("a.txt"), false)]);
    finish(&mut app_state);
    assert_eq!(app_state.error.take().as_deref(), Some("Put back finished, 1 entry skipped"));
    assert_eq!(std::fs::read_to_string(home.join("a.txt")).unwrap(), "in the way");

    // Emptied for good.
    app_state.purge_from_trash(vec![(folder.join("photos"), true)]);
    finish(&mut app_state);
    assert_eq!(names(&app_state), ["a.txt"]);
    assert_eq!(std::fs::read_dir(&keep).unwrap().count(), 1);
    assert!(!home.join("old/photos").exists());

    // Something trashed meanwhile, elsewhere: the panel shows it of itself,
    // as it would a directory changing.
    std::fs::write(home.join("late.txt"), "late").unwrap();
    throw(&home.join("late.txt"), &keep, 1_700_000_300);
    let started = Instant::now();
    while names(&app_state) != ["a.txt", "late.txt"] {
        assert!(started.elapsed() < Duration::from_secs(10), "never seen: {:?}", names(&app_state));
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_watchers();
        app_state.refresh_stale_panels();
        app_state.poll_listings();
    }

    // The tree in the trash: of what is in it, from its top.
    app_state.toggle_tree();
    let tree = app_state.left.tree.as_ref().unwrap();
    assert_eq!(tree.rows[0].path, folder);
    assert_eq!(tree.rows.len(), 1, "files only: nothing under it in the tree");
    app_state.toggle_tree();
    assert_eq!(app_state.left.dir, folder);

    // Alt+T again: back where the panel was.
    app_state.toggle_trash();
    assert_eq!(app_state.left.dir, home);
    std::fs::remove_dir_all(&dir).unwrap();
}
