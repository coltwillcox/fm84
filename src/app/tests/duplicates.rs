//! Alt+D: files the same as another, fed to the panel with all but one of
//! each selected.

use super::*;
use std::ffi::OsString;
use std::time::{Duration, Instant};

#[test]
fn duplicates_are_fed_with_all_but_the_first_selected() {
    let dir = std::env::temp_dir().join(format!("fm84-duplicates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
    std::fs::write(dir.join("photo.jpg"), "the same bytes").unwrap();
    std::fs::write(dir.join("sub/copy.jpg"), "the same bytes").unwrap();
    std::fs::write(dir.join("sub/deeper/again.jpg"), "the same bytes").unwrap();
    // The same size, but not the same.
    std::fs::write(dir.join("sub/other.jpg"), "different byte").unwrap();
    std::fs::write(dir.join("alone.txt"), "nothing like it").unwrap();
    // Empty files are all alike, and are not what is wanted.
    std::fs::write(dir.join("empty1"), "").unwrap();
    std::fs::write(dir.join("empty2"), "").unwrap();
    #[cfg(unix)]
    {
        // A link is not a copy, nor is a second name for the same file.
        std::os::unix::fs::symlink(dir.join("photo.jpg"), dir.join("link.jpg")).unwrap();
        std::fs::hard_link(dir.join("alone.txt"), dir.join("sub/alone-too.txt")).unwrap();
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.active_side = Side::Left;
    app_state.find_duplicates();
    let started = Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < Duration::from_secs(10), "the search never finished");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.error, None);

    let flat = app_state.left.flat.as_ref().expect("fed to the panel");
    let feed: Vec<String> = flat.feed.as_ref().unwrap().iter().map(|(path, _)| path.to_string_lossy().into_owned()).collect();
    let deeper = format!("sub{0}deeper{0}again.jpg", std::path::MAIN_SEPARATOR);
    let copy = format!("sub{}copy.jpg", std::path::MAIN_SEPARATOR);
    assert_eq!(feed, ["photo.jpg".to_string(), copy.clone(), deeper.clone()]);
    let selected: HashSet<OsString> = [OsString::from(copy), OsString::from(deeper)].into();
    assert_eq!(app_state.left.selected, selected);
    assert_eq!(app_state.left.children.iter().filter(|item| !item.is_parent()).count(), 3);

    // Nothing alike: said, and the panel left as it was.
    std::fs::remove_dir_all(dir.join("sub")).unwrap();
    app_state.left.flat = None;
    app_state.left.selected.clear();
    app_state.reload_panel(Side::Left, None);
    app_state.find_duplicates();
    while app_state.workers.job.is_some() {
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.find_note.as_deref(), Some("No duplicates found"));
    assert!(app_state.left.flat.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file reached twice - chosen itself in a list of what Find found, and
/// inside a directory chosen beside it - is one file, not two the same:
/// nothing is offered for deleting, which would delete the only copy.
#[test]
fn a_file_reached_twice_is_not_its_own_duplicate() {
    let dir = std::env::temp_dir().join(format!("fm84-duplicates-twice-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("only.txt"), "the only copy").unwrap();
    let inner = std::path::Path::new("sub").join("only.txt");

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.left.flat = Some(crate::app::Flat {
        dir: dir.clone(),
        feed: Some(std::sync::Arc::new(vec![(std::path::PathBuf::from("sub"), true), (inner.clone(), false)])),
        kind: crate::app::FeedKind::Found,
    });
    app_state.workers.listing_wait = Duration::from_secs(5);
    app_state.reload_panel(Side::Left, None);
    app_state.left.selected = [OsString::from("sub"), inner.clone().into_os_string()].into();
    let chosen = app_state.left.children.iter().filter(|item| app_state.left.selected.contains(&item.name_os)).count();

    app_state.find_duplicates();
    let started = Instant::now();
    while app_state.workers.job.is_some() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
    let (note, feed) = (app_state.find_note.clone(), app_state.left.flat.as_ref().and_then(|flat| flat.feed.as_ref()).map(|feed| feed.len()));
    std::fs::remove_dir_all(&dir).unwrap();

    assert_eq!(chosen, 2, "both rows chosen");
    assert_eq!(note.as_deref(), Some("No duplicates found"));
    assert_eq!(feed, Some(2), "the panel left as Find left it, not fed the file as a copy of itself");
}
