//! Archives as directories, from the keys.

use super::*;
use crate::archive::fixtures::{Put, scratch, tar, zip};
use std::path::PathBuf;

/// Put the active panel's cursor on `name`.
fn on(app_state: &mut AppState, name: &str) {
    let panel = app_state.panel_mut(app_state.active_side);
    let (children, state) = (&panel.children, &mut panel.state);
    let index = children.iter().position(|item| item.name_os == name).unwrap_or_else(|| panic!("no {name} in the panel"));
    state.select(Some(index));
}

fn shown(app_state: &AppState) -> Vec<String> {
    app_state.left.children.iter().map(|item| item.name_full.clone()).collect()
}

fn finish(app_state: &mut AppState) {
    let started = std::time::Instant::now();
    while app_state.workers.job.is_some() {
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
    app_state.open_dir(Side::Right, out, None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("photos.zip".as_ref()));
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
    assert_eq!(app_state.left.dir.as_os_str(), archive.as_os_str());
    assert_eq!(app_state.left.archive.as_ref(), Some(&archive));
    assert_eq!(shown(&app_state), ["..", "2024", "readme.txt"]);

    on(&mut app_state, "2024");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.left.dir, archive.join("2024"));
    assert_eq!(shown(&app_state), ["..", "june", "notes.txt"]);

    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(app_state.left.dir, archive);
    assert_eq!(app_state.left.children[app_state.left.state.selected().unwrap()].name_full, "2024");
    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(app_state.left.dir, dir);
    assert_eq!(app_state.left.archive, None);
    assert_eq!(app_state.left.children[app_state.left.state.selected().unwrap()].name_full, "photos.zip");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F3 copies a file out to a scratch directory and shows it there; the
/// panels are left as they were.
#[test]
fn f3_views_a_file_inside_an_archive() {
    let (dir, archive, mut app_state) = setup("view");
    app_state.open_dir(Side::Left, archive.join("2024/june"), Some("beach.txt".as_ref()));
    press(&mut app_state, KeyCode::F(3));
    finish(&mut app_state);
    let Screen::Viewer(state) = &app_state.screen else {
        panic!("not viewing: {:?}", app_state.error);
    };
    assert_eq!(state.content_lines, ["sand and sea"]);
    assert_eq!(state.file_path.file_name().unwrap(), "beach.txt");
    assert!(!state.file_path.starts_with(&dir));
    assert_eq!(app_state.left.dir, archive.join("2024/june"));
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
    app_state.open_dir(Side::Left, archive.clone(), Some("2024".as_ref()));

    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(out.join("2024/june/beach.txt")).unwrap(), "sand and sea");
    assert_eq!(std::fs::read_to_string(out.join("2024/notes.txt")).unwrap(), "notes");

    on(&mut app_state, "readme.txt");
    press(&mut app_state, KeyCode::F(6));
    assert_eq!(app_state.error.as_deref(), Some(crate::app::read_only()));
    assert!(app_state.dialog.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What would change an entry where it is - renaming it, editing it, its
/// attributes, a link to it - says the archive is read-only, and leaves it
/// as it was.
#[test]
fn what_changes_an_entry_in_place_is_refused() {
    let (dir, archive, mut app_state) = setup("read-only");
    let before = std::fs::read(&archive).unwrap();
    app_state.open_dir(Side::Left, archive.clone(), Some("readme.txt".as_ref()));
    let keys = [(KeyCode::F(2), KeyModifiers::NONE), (KeyCode::F(4), KeyModifiers::NONE), (KeyCode::F(5), KeyModifiers::ALT), (KeyCode::Char('a'), KeyModifiers::ALT)];
    for (code, modifiers) in keys {
        handle_key(&mut app_state, KeyEvent::new(code, modifiers));
        assert_eq!(app_state.error.as_deref(), Some(crate::app::read_only()), "{code:?} {modifiers:?}");
        assert!(app_state.dialog.is_none() && app_state.workers.job.is_none() && matches!(app_state.screen, Screen::Panels), "{code:?} {modifiers:?}");
        app_state.reset_error();
    }
    assert_eq!(std::fs::read(&archive).unwrap(), before);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// F5 and F6 into an archive in the other panel put what is chosen in it,
/// a move taking it off the disk; one already there is asked about. F7
/// and Shift+F4 make a directory and a file in it, landed on; F8 deletes
/// from it, asked first whatever F11 says. From inside one archive into
/// another is refused: what goes in has to be on the disk.
#[test]
fn the_keys_write_into_an_archive() {
    let (dir, archive, mut app_state) = setup("writing");
    let names = |app_state: &AppState| {
        let mut names = shown(app_state);
        names.sort();
        names
    };
    let out = dir.join("out");
    std::fs::write(out.join("new.txt"), "new").unwrap();
    std::fs::create_dir_all(out.join("folder")).unwrap();
    std::fs::write(out.join("folder/inside.txt"), "inside").unwrap();
    app_state.open_dir(Side::Left, archive.join("2024"), None);
    app_state.open_dir(Side::Right, out.clone(), Some("new.txt".as_ref()));
    app_state.active_side = Side::Right;

    press(&mut app_state, KeyCode::F(5));
    assert_eq!(app_state.dialog_input().unwrap().text, archive.join("2024/new.txt").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert!(out.join("new.txt").exists());
    assert_eq!(names(&app_state), ["..", "june", "new.txt", "notes.txt"]);

    // Again: asked about, and left.
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    let started = std::time::Instant::now();
    while app_state.workers.job.as_ref().is_some_and(|job| job.clash.is_none()) {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never asked");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    press(&mut app_state, KeyCode::Char('n'));
    finish(&mut app_state);
    assert!(app_state.error.as_deref().unwrap_or_default().ends_with("1 entry skipped"), "{:?}", app_state.error);
    app_state.reset_error();

    // A directory moved in: in the archive, off the disk.
    on(&mut app_state, "folder");
    press(&mut app_state, KeyCode::F(6));
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert!(!out.join("folder").exists());
    assert_eq!(names(&app_state), ["..", "folder", "june", "new.txt", "notes.txt"]);

    // Made inside, and landed on.
    app_state.active_side = Side::Left;
    press(&mut app_state, KeyCode::F(7));
    for c in "made".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    let at = app_state.left.state.selected().unwrap();
    assert_eq!(app_state.left.children[at].name_full, "made");
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT));
    for c in "blank.txt".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(names(&app_state), ["..", "blank.txt", "folder", "june", "made", "new.txt", "notes.txt"]);
    // A name it has is refused before anything is written.
    press(&mut app_state, KeyCode::F(7));
    for c in "made".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Already exists"), "{:?}", app_state.error);
    assert!(app_state.workers.job.is_none());
    app_state.reset_error();

    // Deleted from it: asked, even with Confirm delete off.
    app_state.options.confirm_delete = false;
    on(&mut app_state, "june");
    press(&mut app_state, KeyCode::F(8));
    assert!(matches!(app_state.dialog, Some(crate::app::Dialog::Delete { to_trash: false, .. })));
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(names(&app_state), ["..", "blank.txt", "folder", "made", "new.txt", "notes.txt"]);
    let index = crate::archive::index(&archive).unwrap();
    assert!(index.find(std::path::Path::new("2024/june/beach.txt")).is_none());
    assert!(index.find(std::path::Path::new("2024/folder/inside.txt")).is_some());
    assert!(index.find(std::path::Path::new("readme.txt")).is_some());

    // From one archive into another.
    let other = dir.join("other.zip");
    zip(&other, &[("x.txt", Put::File(b"x"))]);
    app_state.open_dir(Side::Right, other.clone(), Some("x.txt".as_ref()));
    app_state.active_side = Side::Right;
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Copy it out of the archive first"), "{:?}", app_state.error);
    assert!(app_state.workers.job.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What is not an archive, named like one, says why it cannot be gone into
/// and the panel stays where it was.
#[test]
fn a_damaged_archive_is_not_gone_into() {
    let dir = scratch("damaged");
    std::fs::write(dir.join("broken.zip"), "not a zip at all").unwrap();
    let mut app_state = AppState::new();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("broken.zip".as_ref()));
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.left.dir, dir);
    assert_eq!(app_state.left.archive, None);
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
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, archive.join("src"), None);
    assert_eq!(shown(&app_state), ["..", "lib.rs", "main.rs"]);

    tar(&archive, &[(b"docs/readme", Put::File(b"r"))]);
    app_state.reload_panel(Side::Left, None);
    assert_eq!(app_state.left.dir, archive);
    assert_eq!(shown(&app_state), ["..", "docs"]);

    std::fs::remove_file(&archive).unwrap();
    app_state.reload_panel(Side::Left, None);
    assert_eq!(app_state.left.dir, dir);
    assert_eq!(app_state.left.archive, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The preview and the detail lines read an entry from its archive, and
/// Space sizes a directory there from the index.
#[test]
fn preview_detail_and_size_inside_an_archive() {
    let (dir, archive, mut app_state) = setup("preview");
    app_state.show_preview = true;
    app_state.open_dir(Side::Left, archive.clone(), Some("readme.txt".as_ref()));
    app_state.refresh_cursor_detail();
    app_state.refresh_preview();
    assert_eq!(app_state.preview.as_ref().unwrap().lines, ["read me"]);
    assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "7 bytes");

    on(&mut app_state, "2024");
    press(&mut app_state, KeyCode::Char(' '));
    let started = std::time::Instant::now();
    while !app_state.workers.dir_sizing.is_empty() {
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
    assert_eq!(app_state.left.dir, archive.join("2024/june"));
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
    app_state.open_dir(Side::Left, archive.join("2024"), None);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::CONTROL));
    app_state.dialog_input().unwrap().set("again".to_string());
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(std::fs::read_to_string(dir.join("again/readme.txt")).unwrap(), "read me");

    // Not onto an archive, nor into one.
    app_state.open_dir(Side::Left, dir.clone(), Some("out".as_ref()));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert_eq!(app_state.error.as_deref(), Some("Put the cursor on an archive to extract it"));
    app_state.reset_error();
    on(&mut app_state, "photos.zip");
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    app_state.dialog_input().unwrap().set(archive.join("2024").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.error.as_deref(), Some(crate::app::read_only()));
    assert!(app_state.workers.job.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Shift+F3 compares a file inside an archive with its namesake on the disk,
/// and with one inside another archive, copying each out first; the diff is
/// headed with the paths the panels know them by.
#[test]
fn shift_f3_compares_files_inside_archives() {
    let (dir, archive, mut app_state) = setup("compare");
    let sides = |app_state: &AppState| match &app_state.screen {
        Screen::Diff(state) => (state.left_path.clone(), state.right_path.clone(), state.hunks.len()),
        _ => panic!("not comparing: {:?}", app_state.error),
    };
    let compare = |app_state: &mut AppState| {
        handle_key(app_state, KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
        finish(app_state);
    };

    // Inside on the left, on the disk on the right.
    std::fs::write(dir.join("out/readme.txt"), "read me\nand more\n").unwrap();
    app_state.open_dir(Side::Right, dir.join("out"), Some("readme.txt".as_ref()));
    app_state.open_dir(Side::Left, archive.clone(), Some("readme.txt".as_ref()));
    compare(&mut app_state);
    assert_eq!(sides(&app_state), (archive.join("readme.txt"), dir.join("out/readme.txt"), 1));

    // Inside two archives, one a panel each: two jobs, one after the other.
    app_state.screen = Screen::Panels;
    let other = dir.join("other.tar.gz");
    tar(&other, &[(b"readme.txt", Put::File(b"read me too"))]);
    app_state.open_dir(Side::Right, other.clone(), Some("readme.txt".as_ref()));
    compare(&mut app_state);
    assert_eq!(sides(&app_state), (archive.join("readme.txt"), other.join("readme.txt"), 1));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+C copies an entry's path as it runs through its archive, as the
/// panel shows it.
#[test]
fn ctrl_c_copies_a_path_through_an_archive() {
    let (dir, archive, mut app_state) = setup("paths");
    app_state.open_dir(Side::Left, archive.join("2024"), Some("notes.txt".as_ref()));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(PathBuf::from(&app_state.clipboard), archive.join("2024/notes.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+P packs what is chosen into an archive in the other panel, named
/// after it, and lands there on what it made; Tab goes through the formats
/// by the ending of the name.
#[test]
fn alt_p_packs_into_the_other_panel() {
    let dir = scratch("pack-keys");
    let (here, out) = (dir.join("here"), dir.join("out"));
    std::fs::create_dir_all(here.join("photos")).unwrap();
    std::fs::write(here.join("photos/beach.txt"), "sand").unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut app_state = AppState::new();
    app_state.open_dir(Side::Right, out.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, here.clone(), Some("photos".as_ref()));
    let alt_p = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));
    let typed = |app_state: &mut AppState| PathBuf::from(&app_state.dialog_input().unwrap().text);

    alt_p(&mut app_state);
    assert_eq!(typed(&mut app_state), out.join("photos.zip"));
    let mut seen = Vec::new();
    for _ in 0..7 {
        press(&mut app_state, KeyCode::Tab);
        seen.push(typed(&mut app_state).file_name().unwrap().to_string_lossy().into_owned());
    }
    assert_eq!(seen, ["photos.7z", "photos.tar.gz", "photos.tar.xz", "photos.tar.bz2", "photos.tar.zst", "photos.tar", "photos.zip"]);
    press(&mut app_state, KeyCode::Tab);
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    let index = crate::archive::index(&out.join("photos.7z")).unwrap();
    assert!(index.find(std::path::Path::new("photos/beach.txt")).is_some());
    let landed = &app_state.right.children[app_state.right.state.selected().unwrap()];
    assert_eq!(landed.name_full, "photos.7z");

    // Not again over it, unless F11 says to overwrite.
    alt_p(&mut app_state);
    app_state.dialog_input().unwrap().set(out.join("photos.7z").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Already exists"), "{:?}", app_state.error);
    assert!(app_state.workers.job.is_none());
    app_state.reset_error();

    // With F11 on Keep both, beside it.
    app_state.options.on_existing = crate::options::OnExisting::KeepBoth;
    alt_p(&mut app_state);
    app_state.dialog_input().unwrap().set(out.join("photos.7z").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert!(out.join("photos (2).7z").is_file());
    app_state.options.on_existing = crate::options::OnExisting::Ask;

    // Not a name an archive has, nor into an archive, nor from inside one.
    alt_p(&mut app_state);
    app_state.dialog_input().unwrap().set(out.join("photos.rar").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Name it as an archive"), "{:?}", app_state.error);
    app_state.reset_error();
    alt_p(&mut app_state);
    app_state.dialog_input().unwrap().set(out.join("photos.7z/inside.zip").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.error.as_deref(), Some(crate::app::read_only()));
    app_state.reset_error();
    app_state.open_dir(Side::Left, out.join("photos.7z"), None);
    alt_p(&mut app_state);
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Copy it out of the archive first"), "{:?}", app_state.error);
    assert!(app_state.dialog.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The pack dialog's options: Down goes to them and Space or the arrows
/// change them, the level kept for next time. One for each turns the name
/// to `*` and back; packed so, each gets an archive named after it, two of
/// one name numbered, and moved, what went in is gone.
#[test]
fn alt_p_packs_one_for_each_and_moves() {
    use crate::app::{Dialog, PACK_LEVEL_ROW, PACK_MOVE_ROW, PACK_SEPARATE_ROW};
    use crate::fs_ops::PackLevel;
    let dir = scratch("pack-options");
    let (here, out) = (dir.join("here"), dir.join("out"));
    std::fs::create_dir_all(here.join("photos")).unwrap();
    std::fs::write(here.join("photos/beach.txt"), "sand").unwrap();
    std::fs::write(here.join("notes.txt"), "notes").unwrap();
    std::fs::write(here.join("notes.md"), "# notes").unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut app_state = AppState::new();
    app_state.open_dir(Side::Right, out.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, here.clone(), None);
    for name in ["photos", "notes.txt", "notes.md"] {
        on(&mut app_state, name);
        press(&mut app_state, KeyCode::Insert);
    }
    let alt_p = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));
    let typed = |app_state: &mut AppState| match &app_state.dialog {
        Some(Dialog::Pack(form)) => PathBuf::from(&form.input.text),
        _ => panic!("no pack dialog"),
    };
    let form = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Pack(form)) => (form.focus, form.level, form.separate, form.move_after),
        _ => panic!("no pack dialog"),
    };

    alt_p(&mut app_state);
    assert_eq!(typed(&mut app_state), out.join("here.zip"));
    // On the name, Space types and the arrows move the cursor.
    press(&mut app_state, KeyCode::Left);
    assert_eq!(form(&app_state), (0, PackLevel::Normal, false, false));
    press(&mut app_state, KeyCode::Down);
    press(&mut app_state, KeyCode::Right);
    assert_eq!(form(&app_state), (PACK_LEVEL_ROW, PackLevel::Best, false, false));
    press(&mut app_state, KeyCode::Down);
    press(&mut app_state, KeyCode::Char(' '));
    assert_eq!(form(&app_state), (PACK_SEPARATE_ROW, PackLevel::Best, true, false));
    assert_eq!(typed(&mut app_state), out.join("*.zip"));
    press(&mut app_state, KeyCode::Tab);
    assert_eq!(typed(&mut app_state), out.join("*.7z"));
    press(&mut app_state, KeyCode::Char(' '));
    assert_eq!(typed(&mut app_state), out.join("here.7z"));
    press(&mut app_state, KeyCode::Char(' '));
    press(&mut app_state, KeyCode::Tab);
    press(&mut app_state, KeyCode::Down);
    press(&mut app_state, KeyCode::Char(' '));
    assert_eq!(form(&app_state), (PACK_MOVE_ROW, PackLevel::Best, true, true));
    // Round past the last row to the name.
    for _ in 0..4 {
        press(&mut app_state, KeyCode::Down);
    }
    assert_eq!(form(&app_state).0, 0);
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);

    let made = |name: &str| crate::archive::index(&out.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(made("photos.tar.gz").find(std::path::Path::new("photos/beach.txt")).is_some());
    let notes = [made("notes.tar.gz"), made("notes (2).tar.gz")];
    let mut names: Vec<String> = notes.iter().map(|index| index.children(std::path::Path::new("")).next().unwrap().name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["notes.md", "notes.txt"]);
    let left: Vec<_> = std::fs::read_dir(&here).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");

    // Best is offered again; one for each and moving are not.
    alt_p(&mut app_state);
    assert!(app_state.dialog.is_none(), "nothing left to pack");
    app_state.open_dir(Side::Left, out.clone(), Some("photos.tar.gz".as_ref()));
    alt_p(&mut app_state);
    assert_eq!(form(&app_state), (0, PackLevel::Best, false, false));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Checksums are worked out of files on the disk: inside an archive, Alt+H
/// says so.
#[test]
fn alt_h_inside_an_archive_says_to_copy_out_first() {
    let (dir, archive, mut app_state) = setup("checksum-inside");
    app_state.open_dir(Side::Left, archive.clone(), Some("readme.txt".as_ref()));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT));
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Copy it out of the archive first"), "{:?}", app_state.error);
    assert!(app_state.workers.job.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+B lists every file in an archive at once, by its path from where
/// the panel is, and F3 opens one from there as from its own directory.
#[test]
fn ctrl_b_lists_every_file_in_an_archive() {
    let (dir, archive, mut app_state) = setup("flat");
    press(&mut app_state, KeyCode::Enter);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert_eq!(app_state.left.flat.as_ref().map(|flat| &flat.dir), Some(&archive));
    let mut names = shown(&app_state);
    names[1..].sort();
    assert_eq!(names, ["..", "2024/june/beach.txt", "2024/notes.txt", "readme.txt"]);

    on(&mut app_state, "2024/june/beach.txt");
    press(&mut app_state, KeyCode::F(3));
    finish(&mut app_state);
    let Screen::Viewer(state) = &app_state.screen else {
        panic!("not viewing: {:?}", app_state.error);
    };
    assert_eq!(state.content_lines, ["sand and sea"]);
    let scratch = state.file_path.parent().unwrap().to_path_buf();
    std::fs::remove_dir_all(&scratch).unwrap();
    let _ = std::fs::remove_dir(scratch.parent().unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Make an archive of `files` with `password`, as Alt+P would.
fn locked(dest: &std::path::Path, files: &[(PathBuf, bool)], password: &str) {
    let format = crate::fs_ops::PackFormat::of(dest).unwrap();
    let mut ask = |_: crate::fs_ops::Step<'_>| true;
    let plan = crate::fs_ops::plan_pack(files, dest, format, None, &mut ask).unwrap().unwrap();
    crate::fs_ops::pack(&plan, dest, format, crate::fs_ops::PackLevel::Fast, Some(password), false, &mut ask).unwrap();
    crate::archive::forget_password(dest);
}

fn type_in(app_state: &mut AppState, text: &str) {
    for c in text.chars() {
        press(app_state, KeyCode::Char(c));
    }
}

/// A 7z with its names encrypted asks for its password as it is gone into,
/// again when the one given is wrong, and goes in with the right one. A
/// zip's files ask as they are copied out. The pack dialog makes one with
/// a password typed the same twice, and only a zip or a 7z.
#[test]
fn a_password_is_asked_for_and_given() {
    use crate::app::Dialog;
    let dir = scratch("passwords");
    let (here, out) = (dir.join("here"), dir.join("out"));
    std::fs::create_dir_all(&here).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(here.join("secret.txt"), "hush").unwrap();
    locked(&here.join("box.7z"), &[(here.join("secret.txt"), false)], "sesame");
    locked(&here.join("box.zip"), &[(here.join("secret.txt"), false)], "sesame");
    let mut app_state = AppState::new();
    app_state.open_dir(Side::Right, out.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, here.clone(), Some("box.7z".as_ref()));
    let asked = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Password(ask)) => Some(ask.wrong),
        _ => None,
    };

    press(&mut app_state, KeyCode::Enter);
    assert_eq!(asked(&app_state), Some(false), "{:?}", app_state.error);
    assert_eq!(app_state.left.dir, here);
    type_in(&mut app_state, "open up");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(asked(&app_state), Some(true));
    type_in(&mut app_state, "sesame");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.error, None);
    assert_eq!(app_state.left.dir, here.join("box.7z"));
    assert!(shown(&app_state).contains(&"secret.txt".to_string()));

    // The zip lists its names, and asks as a file is copied out.
    app_state.open_dir(Side::Left, here.join("box.zip"), Some("secret.txt".as_ref()));
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(asked(&app_state), Some(false));
    assert!(app_state.workers.job.is_none());
    type_in(&mut app_state, "sesame");
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(out.join("secret.txt")).unwrap(), "hush");

    // Packed with one: typed the same twice, and into what can hold one.
    app_state.open_dir(Side::Left, here.clone(), Some("secret.txt".as_ref()));
    let fill = |app_state: &mut AppState, name: &str, password: &str, again: &str| {
        handle_key(app_state, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));
        let Some(Dialog::Pack(form)) = &mut app_state.dialog else { panic!("no pack dialog") };
        form.input.set(out.join(name).display().to_string());
        form.password.set(password.to_string());
        form.again.set(again.to_string());
        press(app_state, KeyCode::Enter);
    };
    fill(&mut app_state, "new.7z", "one", "two");
    assert!(app_state.error.as_deref().unwrap_or_default().contains("not the same"), "{:?}", app_state.error);
    app_state.reset_error();
    // Typed only the second time, it is not left out without a word.
    fill(&mut app_state, "new.7z", "", "one");
    assert!(app_state.error.as_deref().unwrap_or_default().contains("not the same"), "{:?}", app_state.error);
    assert!(app_state.workers.job.is_none());
    app_state.reset_error();
    fill(&mut app_state, "new.tar.gz", "one", "one");
    assert!(app_state.error.as_deref().unwrap_or_default().contains(".zip or a .7z"), "{:?}", app_state.error);
    app_state.reset_error();
    fill(&mut app_state, "new.7z", "one", "one");
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    // Kept for it: gone into without being asked.
    app_state.open_dir(Side::Left, out.join("new.7z"), None);
    assert_eq!(asked(&app_state), None);
    assert!(shown(&app_state).contains(&"secret.txt".to_string()));
    crate::archive::forget_password(&out.join("new.7z"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What is packed is never the archive it goes into: Overwrite on, both
/// panels in one directory, a zip packed under its own name is refused -
/// rather than written over with an archive of nothing, the walk leaving
/// out the archive being made.
#[test]
fn an_archive_is_not_packed_into_itself() {
    let dir = scratch("pack-into-itself");
    zip(&dir.join("box.zip"), &[("a.txt", Put::File(b"a"))]);
    let before = std::fs::read(dir.join("box.zip")).unwrap();
    let mut app_state = AppState::new();
    app_state.options.on_existing = crate::options::OnExisting::Overwrite;
    app_state.open_dir(Side::Right, dir.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("box.zip".as_ref()));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));
    assert_eq!(app_state.dialog_input().unwrap().text, dir.join("box.zip").display().to_string());
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert!(app_state.error.as_deref().unwrap_or_default().contains("into itself"), "{:?}", app_state.error);
    assert_eq!(std::fs::read(dir.join("box.zip")).unwrap(), before);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A 7z with its names encrypted is asked for its password before it is
/// extracted or written into from outside - not only when gone into -
/// and the work goes ahead once it is given.
#[test]
fn a_locked_7z_asks_from_outside_too() {
    use crate::app::Dialog;
    let dir = scratch("locked-outside");
    let (here, out) = (dir.join("here"), dir.join("out"));
    std::fs::create_dir_all(&here).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(here.join("secret.txt"), "hush").unwrap();
    std::fs::write(here.join("more.txt"), "more").unwrap();
    locked(&here.join("box.7z"), &[(here.join("secret.txt"), false)], "sesame");
    let mut app_state = AppState::new();
    app_state.open_dir(Side::Right, out.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, here.clone(), Some("box.7z".as_ref()));
    let asked = |app_state: &AppState| matches!(app_state.dialog, Some(Dialog::Password(_)));

    // Alt+F9 on it, from the directory it is in.
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    press(&mut app_state, KeyCode::Enter);
    assert!(asked(&app_state), "{:?}", app_state.error);
    type_in(&mut app_state, "sesame");
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read_to_string(out.join("secret.txt")).unwrap(), "hush");

    // F5 into it, from the other panel, before it was ever gone into.
    crate::archive::forget_password(&here.join("box.7z"));
    crate::archive::forget_index(&here.join("box.7z"));
    app_state.open_dir(Side::Right, here.join("box.7z"), None);
    assert!(asked(&app_state));
    app_state.dialog = None;
    app_state.open_dir(Side::Left, here.clone(), Some("more.txt".as_ref()));
    let typed = here.join("box.7z").display().to_string();
    press(&mut app_state, KeyCode::F(5));
    app_state.dialog_input().unwrap().set(typed);
    press(&mut app_state, KeyCode::Enter);
    assert!(asked(&app_state), "{:?}", app_state.error);
    type_in(&mut app_state, "sesame");
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(app_state.error, None);
    let index = crate::archive::index(&here.join("box.7z")).unwrap();
    assert!(index.find(std::path::Path::new("more.txt")).is_some());
    crate::archive::forget_password(&here.join("box.7z"));
    std::fs::remove_dir_all(&dir).unwrap();
}
