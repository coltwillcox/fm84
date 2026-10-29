//! Archives as directories, from the keys.

use super::*;
use crate::archive::fixtures::{Put, scratch, tar, zip};
use std::path::PathBuf;

/// Put the active panel's cursor on `name`.
fn on(app_state: &mut AppState, name: &str) {
    let (children, state) = if app_state.is_left_active {
        (&app_state.children_left, &mut app_state.state_left)
    } else {
        (&app_state.children_right, &mut app_state.state_right)
    };
    let index = children.iter().position(|item| item.name_os == name).unwrap_or_else(|| panic!("no {name} in the panel"));
    state.select(Some(index));
}

fn shown(app_state: &AppState) -> Vec<String> {
    app_state.children_left.iter().map(|item| item.name_full.clone()).collect()
}

fn finish(app_state: &mut AppState) {
    let started = std::time::Instant::now();
    while app_state.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
}

/// An archive with a little of everything, beside a panel to copy out to.
fn setup(name: &str) -> (PathBuf, PathBuf, AppState) {
    let dir = scratch(name);
    let archive = dir.join("photos.zip");
    zip(&archive, &[("2024/june/beach.txt", Put::File(b"sand and sea")), ("2024/notes.txt", Put::File(b"notes")), ("readme.txt", Put::File(b"read me"))]);
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut app_state = AppState::new();
    app_state.open_dir(false, out, None);
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("photos.zip".as_ref()));
    (dir, archive, app_state)
}

/// Enter goes into an archive as into a directory, and into the directories
/// in it; Backspace comes back out the way it went in, landing on what it
/// left - the archive itself last.
#[test]
fn enter_goes_into_an_archive_and_backspace_comes_out() {
    let (dir, archive, mut app_state) = setup("in-and-out");

    press(&mut app_state, KeyCode::Enter);
    // As written, not only as compared: a separator on the end of a file's
    // path does not stat, and the next reread climbed out.
    assert_eq!(app_state.dir_left.as_os_str(), archive.as_os_str());
    assert_eq!(app_state.archive_left.as_ref(), Some(&archive));
    assert_eq!(shown(&app_state), ["..", "2024", "readme.txt"]);

    on(&mut app_state, "2024");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.dir_left, archive.join("2024"));
    assert_eq!(shown(&app_state), ["..", "june", "notes.txt"]);

    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(app_state.dir_left, archive);
    assert_eq!(app_state.children_left[app_state.state_left.selected().unwrap()].name_full, "2024");
    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(app_state.archive_left, None);
    assert_eq!(app_state.children_left[app_state.state_left.selected().unwrap()].name_full, "photos.zip");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F3 copies a file out to a scratch directory and shows it there; the
/// panels are left as they were.
#[test]
fn f3_views_a_file_inside_an_archive() {
    let (dir, archive, mut app_state) = setup("view");
    app_state.open_dir(true, archive.join("2024/june"), Some("beach.txt".as_ref()));
    press(&mut app_state, KeyCode::F(3));
    finish(&mut app_state);
    let Screen::Viewer(state) = &app_state.screen else {
        panic!("not viewing: {:?}", app_state.error);
    };
    assert_eq!(state.content_lines, ["sand and sea"]);
    assert_eq!(state.file_path.file_name().unwrap(), "beach.txt");
    assert!(!state.file_path.starts_with(&dir));
    assert_eq!(app_state.dir_left, archive.join("2024/june"));
    // Its scratch directory, and the one they are all in if nothing else in
    // this run has one there too.
    let scratch = state.file_path.parent().unwrap().to_path_buf();
    std::fs::remove_dir_all(&scratch).unwrap();
    let _ = std::fs::remove_dir(scratch.parent().unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F5 copies out into the other panel, a directory whole; F6, which would
/// take entries out of the archive, is refused.
#[test]
fn f5_copies_out_and_f6_is_refused() {
    let (dir, archive, mut app_state) = setup("copy-out");
    let out = dir.join("out");
    app_state.open_dir(true, archive.clone(), Some("2024".as_ref()));

    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(out.join("2024/june/beach.txt")).unwrap(), "sand and sea");
    assert_eq!(std::fs::read_to_string(out.join("2024/notes.txt")).unwrap(), "notes");

    on(&mut app_state, "readme.txt");
    press(&mut app_state, KeyCode::F(6));
    assert_eq!(app_state.error.as_deref(), Some(crate::app::READ_ONLY));
    assert!(app_state.dialog.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Everything that would write into an archive says it is read-only and
/// leaves it as it was - from inside it, and from the other panel into it.
#[test]
fn nothing_writes_into_an_archive() {
    let (dir, archive, mut app_state) = setup("read-only");
    let before = std::fs::read(&archive).unwrap();
    app_state.open_dir(true, archive.clone(), Some("readme.txt".as_ref()));

    let keys = [
        (KeyCode::F(2), KeyModifiers::NONE),
        (KeyCode::F(4), KeyModifiers::NONE),
        (KeyCode::F(4), KeyModifiers::SHIFT),
        (KeyCode::F(5), KeyModifiers::ALT),
        (KeyCode::F(7), KeyModifiers::NONE),
        (KeyCode::F(8), KeyModifiers::NONE),
        (KeyCode::F(8), KeyModifiers::SHIFT),
        (KeyCode::Delete, KeyModifiers::NONE),
        (KeyCode::Char('a'), KeyModifiers::ALT),
    ];
    for (code, modifiers) in keys {
        handle_key(&mut app_state, KeyEvent::new(code, modifiers));
        assert_eq!(app_state.error.as_deref(), Some(crate::app::READ_ONLY), "{code:?} {modifiers:?}");
        assert!(app_state.dialog.is_none() && app_state.job.is_none() && matches!(app_state.screen, Screen::Panels), "{code:?} {modifiers:?}");
        app_state.reset_error();
    }

    // From the other side: a file copied or moved into the archive.
    std::fs::write(dir.join("out/new.txt"), "new").unwrap();
    app_state.open_dir(false, dir.join("out"), Some("new.txt".as_ref()));
    app_state.is_left_active = false;
    for code in [KeyCode::F(5), KeyCode::F(6)] {
        press(&mut app_state, code);
        press(&mut app_state, KeyCode::Enter);
        assert_eq!(app_state.error.as_deref(), Some(crate::app::READ_ONLY), "{code:?}");
        assert!(app_state.job.is_none());
        app_state.reset_error();
    }
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What is not an archive, named like one, says why it cannot be gone into
/// and the panel stays where it was.
#[test]
fn a_damaged_archive_is_not_gone_into() {
    let dir = scratch("damaged");
    std::fs::write(dir.join("broken.zip"), "not a zip at all").unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("broken.zip".as_ref()));
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(app_state.archive_left, None);
    assert!(app_state.error.as_deref().unwrap_or_default().contains("not readable as an archive"), "{:?}", app_state.error);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An archive written over while a panel is inside it is read again, and a
/// directory it no longer has is climbed out of; one deleted is left.
#[test]
fn a_changed_or_deleted_archive_is_followed() {
    let dir = scratch("changed");
    let archive = dir.join("src.tar.gz");
    tar(&archive, &[(b"src/main.rs", Put::File(b"fn main() {}")), (b"src/lib.rs", Put::File(b""))]);
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, archive.join("src"), None);
    assert_eq!(shown(&app_state), ["..", "lib.rs", "main.rs"]);

    tar(&archive, &[(b"docs/readme", Put::File(b"r"))]);
    app_state.reload_panel(true, None);
    assert_eq!(app_state.dir_left, archive);
    assert_eq!(shown(&app_state), ["..", "docs"]);

    std::fs::remove_file(&archive).unwrap();
    app_state.reload_panel(true, None);
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(app_state.archive_left, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The preview and the detail lines read an entry from its archive, and
/// Space sizes a directory there from the index.
#[test]
fn preview_detail_and_size_inside_an_archive() {
    let (dir, archive, mut app_state) = setup("preview");
    app_state.show_preview = true;
    app_state.open_dir(true, archive.clone(), Some("readme.txt".as_ref()));
    app_state.refresh_cursor_detail();
    app_state.refresh_preview();
    assert_eq!(app_state.preview.as_ref().unwrap().lines, ["read me"]);
    assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "7 bytes");

    on(&mut app_state, "2024");
    press(&mut app_state, KeyCode::Char(' '));
    let started = std::time::Instant::now();
    while !app_state.dir_sizing.is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never sized");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_dir_sizes();
    }
    assert_eq!(app_state.dir_sizes.get(&archive.join("2024")), Some(&17));
    assert_eq!(app_state.error, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A path through an archive is somewhere a tab can be, typed into Ctrl+L
/// or kept as a bookmark: going there goes in.
#[test]
fn a_path_through_an_archive_is_gone_to() {
    let (dir, archive, mut app_state) = setup("go-to");
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    app_state.dialog_input().unwrap().set(archive.join("2024/june").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.dir_left, archive.join("2024/june"));
    assert_eq!(shown(&app_state), ["..", "beach.txt"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+F9 extracts the whole of the archive under the cursor - or, Ctrl+F9,
/// of the one the panel is inside - into the other panel; on anything else
/// it says what it wants.
#[test]
fn alt_f9_extracts_a_whole_archive() {
    let (dir, archive, mut app_state) = setup("extract-all");
    let out = dir.join("out");

    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert!(matches!(app_state.dialog, Some(Dialog::Extract { .. })));
    assert_eq!(PathBuf::from(&app_state.dialog_input().unwrap().text), out);
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(out.join("2024/june/beach.txt")).unwrap(), "sand and sea");
    assert_eq!(std::fs::read_to_string(out.join("readme.txt")).unwrap(), "read me");

    // From inside it, into a directory typed: relative to where the archive is.
    std::fs::create_dir(dir.join("again")).unwrap();
    app_state.open_dir(true, archive.join("2024"), None);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::CONTROL));
    app_state.dialog_input().unwrap().set("again".to_string());
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(std::fs::read_to_string(dir.join("again/readme.txt")).unwrap(), "read me");

    // Not onto an archive, nor into one.
    app_state.open_dir(true, dir.clone(), Some("out".as_ref()));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert_eq!(app_state.error.as_deref(), Some("Put the cursor on an archive to extract it"));
    app_state.reset_error();
    on(&mut app_state, "photos.zip");
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    app_state.dialog_input().unwrap().set(archive.join("2024").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.error.as_deref(), Some(crate::app::READ_ONLY));
    assert!(app_state.job.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}
