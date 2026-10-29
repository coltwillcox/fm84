//! Reading directories into the panels, tabs and their history, and what
//! arrives from the threads behind them.

use super::*;

/// A file name is bytes, and need not be valid UTF-8. The strings the
/// interface shows come from to_string_lossy, so joining one back together
/// names a file that is not there - every path has to come from the name
/// the filesystem gave.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_still_names_its_file() {
    use std::os::unix::ffi::OsStrExt;

    let dir = std::env::temp_dir().join(format!("fm84-latin1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // "café.txt" as Latin-1 writes it, which is not valid UTF-8.
    let raw = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    std::fs::write(dir.join(raw), "contents").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(true, dir.clone(), None);
    let item = app_state.children_left.iter().find(|item| item.name_full != "..").unwrap().clone();
    // Blank until the stem was read lossily too, which left the column empty.
    assert!(!item.name.is_empty());

    let index = app_state.children_left.iter().position(|row| row.name_full == item.name_full).unwrap();
    app_state.state_left.select(Some(index));
    let (path, _, _) = app_state.cursor_target().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "contents");
    // What the name alone would have named, and what every operation used
    // to be handed: a path with a replacement character in it, and nothing
    // of that name on disk.
    assert!(!dir.join(&item.name_full).exists());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file called `...` has `..` for its stem. Told apart from the parent
/// row by that, it was entered as the way up, and could not be selected,
/// renamed, copied or deleted.
#[test]
fn a_file_called_three_dots_is_not_the_parent_entry() {
    let dir = std::env::temp_dir().join(format!("fm84-three-dots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("..."), "dots").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(true, dir.clone(), Some("...".as_ref()));
    let item = &app_state.children_left[app_state.state_left.selected().unwrap()];
    assert_eq!(item.name, "..");
    assert!(!item.is_parent());
    assert!(app_state.children_left[0].is_parent());

    let (path, _, _) = app_state.cursor_target().unwrap();
    assert_eq!(path, dir.join("..."));
    app_state.toggle_selection_no_size();
    assert!(app_state.selected_left.contains(std::ffi::OsStr::new("...")));

    std::fs::remove_dir_all(&dir).unwrap();
}

/// The preview and the detail lines hold what they gathered until the
/// cursor moves, so a file that changes under a still cursor has to be
/// picked up by the reread instead.
#[test]
fn a_reread_gathers_the_preview_and_the_detail_again() {
    let dir = std::env::temp_dir().join(format!("fm84-reread-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.show_preview = true;
    app_state.open_dir(true, dir.clone(), Some("notes.txt".as_ref()));
    app_state.refresh_cursor_detail();
    app_state.refresh_preview();
    assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "5 bytes");
    assert_eq!(app_state.preview.as_ref().unwrap().lines, ["hello"]);

    // The file grows where it stands, and the panel rereads - which is
    // what saving in the editor, or a copy landing on it, comes to.
    std::fs::write(dir.join("notes.txt"), "hello, a longer file now").unwrap();
    app_state.reload_panel(true, None);
    app_state.refresh_cursor_detail();
    app_state.refresh_preview();
    assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "24 bytes");
    assert_eq!(app_state.preview.as_ref().unwrap().lines, ["hello, a longer file now"]);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// A directory that cannot be read is refused, and the panel stays where
/// it was rather than moving there with nothing to show. The error is said
/// once: the refresh does not raise it again after it is closed.
#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_refused_once() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fm84-unreadable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let locked = dir.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(dir.join("here.txt"), "").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything, so there is nothing to refuse.
    if std::fs::read_dir(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        return std::fs::remove_dir_all(&dir).unwrap();
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    // Watchers that look every few milliseconds, so the ticks the
    // refresh would answer come round inside the test.
    app_state.watch_interval = std::time::Duration::from_millis(5);
    app_state.open_dir(true, dir.clone(), None);
    let listed = app_state.children_left.len();

    app_state.open_dir(true, locked.clone(), None);
    assert!(app_state.error.is_some());
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(app_state.children_left.len(), listed);

    // Closed, and a refresh comes round: it stays closed.
    app_state.reset_error();
    std::thread::sleep(std::time::Duration::from_millis(50));
    app_state.poll_watchers();
    app_state.refresh_stale_panels();
    assert!(app_state.error.is_none());

    // Gone unreadable while the panel is in it: said once, not every tick.
    app_state.open_dir(true, locked.parent().unwrap().to_path_buf(), None);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    app_state.open_dir(true, locked.clone(), None);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    app_state.reload_panel(true, None);
    assert!(app_state.error.is_some());
    app_state.reset_error();
    std::thread::sleep(std::time::Duration::from_millis(50));
    app_state.poll_watchers();
    app_state.refresh_stale_panels();
    assert!(app_state.error.is_none());

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Each tab has a history of its own: a new one starts with none, and one
/// switched away from and back still has its own.
#[test]
fn each_tab_keeps_its_own_way_back() {
    let dir = std::env::temp_dir().join(format!("fm84-tab-history-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.join("a"), None);
    app_state.open_dir(true, dir.join("b"), None);

    app_state.new_tab();
    app_state.travel(true);
    assert_eq!(app_state.dir_left, dir.join("b"), "a new tab has nowhere to go back to");
    app_state.open_dir(true, dir.clone(), None);

    app_state.select_tab(true, 0);
    app_state.travel(true);
    assert_eq!(app_state.dir_left, dir.join("a"));
    app_state.select_tab(true, 1);
    app_state.travel(true);
    assert_eq!(app_state.dir_left, dir.join("b"));
    // Switching tabs is not a step of either's.
    app_state.travel(true);
    assert_eq!(app_state.dir_left, dir.join("b"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn ctrl_arrows_send_the_entry_under_the_cursor_to_a_panel() {
    let dir = std::env::temp_dir().join(format!("fm84-open-in-panel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    std::fs::write(dir.join("note.txt"), "").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(true, dir.clone(), None);
    app_state.open_dir(false, std::env::temp_dir(), None);
    app_state.is_left_active = true;
    let at = |app_state: &AppState, name: &str| app_state.children_left.iter().position(|item| item.name_full == name);
    let cursor_right = |app_state: &AppState| app_state.state_right.selected().and_then(|index| app_state.children_right.get(index)).map(|item| item.name_full.clone());

    // A directory: the other panel goes into it, and the focus stays.
    app_state.state_left.select(at(&app_state, "inner"));
    app_state.open_in_panel(false);
    assert_eq!(app_state.dir_right, dir.join("inner"));
    assert!(app_state.is_left_active);
    assert_eq!(app_state.dir_left, dir);

    // A file: the directory it is in, on the file.
    app_state.state_left.select(at(&app_state, "note.txt"));
    app_state.open_in_panel(false);
    assert_eq!(app_state.dir_right, dir);
    assert_eq!(cursor_right(&app_state).as_deref(), Some("note.txt"));

    // The parent entry: the parent, on the directory left.
    app_state.state_left.select(at(&app_state, ".."));
    app_state.open_in_panel(false);
    assert_eq!(app_state.dir_right, std::env::temp_dir());
    assert_eq!(cursor_right(&app_state), dir.file_name().map(|name| name.to_string_lossy().into_owned()));

    // Pointed at the active panel, it goes in there.
    app_state.state_left.select(at(&app_state, "inner"));
    app_state.open_in_panel(true);
    assert_eq!(app_state.dir_left, dir.join("inner"));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tabs_keep_where_each_was_left() {
    let dir = std::env::temp_dir().join(format!("fm84-tabs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    for name in ["one.txt", "two.txt"] {
        std::fs::write(dir.join(name), "").unwrap();
        std::fs::write(dir.join("inner").join(name), "").unwrap();
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    let at = |app_state: &AppState, name: &str| app_state.children_left.iter().position(|item| item.name_full == name);
    let cursor = |app_state: &AppState| app_state.state_left.selected().and_then(|index| app_state.children_left.get(index)).map(|item| item.name_full.clone());

    // Something selected and the cursor on two.txt in the first tab.
    app_state.selected_left.insert("one.txt".into());
    app_state.state_left.select(at(&app_state, "two.txt"));

    // A new tab: same place, nothing selected, and the first keeps its own.
    app_state.new_tab();
    assert_eq!((app_state.tabs_left.list.len(), app_state.tabs_left.active), (2, 1));
    assert_eq!(app_state.dir_left, dir);
    assert!(app_state.selected_left.is_empty());
    assert_eq!(cursor(&app_state).as_deref(), Some("two.txt"));

    // Off somewhere else in the second tab.
    app_state.open_dir(true, dir.join("inner"), None);
    app_state.selected_left.insert("two.txt".into());
    assert_eq!(app_state.tab_dirs(true), (vec![dir.clone(), dir.join("inner")], 1));

    // Back to the first: its directory, cursor and selection.
    app_state.select_tab(true, 0);
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(cursor(&app_state).as_deref(), Some("two.txt"));
    assert_eq!(app_state.selected_left.iter().collect::<Vec<_>>(), [&std::ffi::OsString::from("one.txt")]);

    // Round to the second, and its own come back.
    app_state.cycle_tab(true);
    assert_eq!(app_state.dir_left, dir.join("inner"));
    assert_eq!(app_state.selected_left.iter().collect::<Vec<_>>(), [&std::ffi::OsString::from("two.txt")]);
    app_state.cycle_tab(true);
    assert_eq!(app_state.tabs_left.active, 0);

    // Closing one not shown leaves the panel be and keeps the right one active.
    app_state.select_tab(true, 1);
    app_state.close_tab(true, 0);
    assert_eq!((app_state.tabs_left.list.len(), app_state.tabs_left.active), (1, 0));
    assert_eq!(app_state.dir_left, dir.join("inner"));

    // The last one stays.
    app_state.close_tab(true, 0);
    assert_eq!(app_state.tabs_left.list.len(), 1);

    // Closing the one shown moves to its neighbour.
    app_state.new_tab();
    app_state.open_dir(true, dir.clone(), None);
    app_state.close_tab(true, 1);
    assert_eq!(app_state.dir_left, dir.join("inner"));
    // The other panel's tabs are its own.
    assert_eq!(app_state.tabs_right.list.len(), 1);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tab whose directory has gone unreadable is refused: the panel, its
/// tabs and its selection stay as they were, rather than the panel staying
/// put under the other tab's name and taking that tab over.
#[cfg(unix)]
#[test]
fn a_tab_that_cannot_be_read_is_refused_and_kept() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fm84-tab-locked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let locked = dir.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(dir.join("here.txt"), "").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    app_state.new_tab();
    app_state.open_dir(true, locked.clone(), None);
    app_state.select_tab(true, 0);
    app_state.selected_left.insert("here.txt".into());

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything, so there is nothing to refuse.
    if std::fs::read_dir(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        return std::fs::remove_dir_all(&dir).unwrap();
    }

    app_state.select_tab(true, 1);
    assert!(app_state.error.is_some());
    assert_eq!(app_state.tabs_left.active, 0);
    assert_eq!(app_state.dir_left, dir);
    assert_eq!(app_state.selected_left.len(), 1);
    assert_eq!(app_state.tab_dirs(true), (vec![dir.clone(), locked.clone()], 0));

    // Closing the tab shown would move to the locked one, so it stays too.
    app_state.reset_error();
    app_state.close_tab(true, 0);
    assert!(app_state.error.is_some());
    assert_eq!(app_state.tab_dirs(true), (vec![dir.clone(), locked.clone()], 0));
    assert_eq!(app_state.selected_left.len(), 1);

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A directory that has not answered yet leaves the panel where it was,
/// busy, with nothing on it acting on the rows about to be replaced; it
/// is taken in when it lands. Built by hand, so the test decides when.
#[test]
fn a_slow_directory_leaves_the_panel_as_it_was_until_it_lands() {
    let dir = std::env::temp_dir().join(format!("fm84-slow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("far")).unwrap();
    std::fs::write(dir.join("far").join("there.txt"), "").unwrap();

    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    let listed = app_state.children_left.len();

    let (sender, answer) = std::sync::mpsc::channel();
    app_state.listing_left = Some(crate::app::PendingListing {
        dir: dir.join("far"),
        navigate: true,
        select: None,
        tab: None,
        travel: crate::app::Travel::Go,
        answer,
    });
    assert!(app_state.panel_busy(true));
    // The panel is as it was, and its tabs cannot be touched.
    app_state.new_tab();
    assert_eq!(app_state.tabs_left.list.len(), 1);
    assert_eq!((app_state.dir_left.clone(), app_state.children_left.len()), (dir.clone(), listed));
    // Nor does a reread of where it was get in the way of where it is going.
    app_state.reload_panel(true, None);
    assert!(app_state.listing_left.as_ref().is_some_and(|pending| pending.dir == dir.join("far")));
    app_state.poll_listings();
    assert!(app_state.panel_busy(true), "nothing has landed yet");

    sender.send(crate::app::read_listing(&dir.join("far"), &app_state.options)).unwrap();
    app_state.poll_listings();
    assert!(!app_state.panel_busy(true));
    assert_eq!(app_state.dir_left, dir.join("far"));
    assert!(app_state.children_left.iter().any(|item| item.name_full == "there.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tab switched to whose directory turns out, late, to be unreadable is
/// switched back - as it is when Esc stops waiting for it.
#[test]
fn a_late_refusal_or_esc_puts_the_tab_back() {
    let dir = std::env::temp_dir().join(format!("fm84-late-tab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut app_state = AppState::new();
    app_state.open_dir(true, dir.clone(), None);
    app_state.tabs_left = crate::app::Tabs::new(vec![dir.clone(), dir.join("locked")], 0);

    for refused in [true, false] {
        let (sender, answer) = std::sync::mpsc::channel();
        app_state.tabs_left.active = 1;
        let tab = crate::app::TabSwitch {
            selected: HashSet::new(),
            undo: crate::app::TabUndo::Active(0),
        };
        app_state.listing_left = Some(crate::app::PendingListing {
            dir: dir.join("locked"),
            navigate: true,
            select: None,
            tab: Some(tab),
            travel: crate::app::Travel::Stay,
            answer,
        });
        if refused {
            let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
            sender
                .send(crate::app::Listed {
                    dir: Some(dir.join("locked")),
                    items: Err(denied),
                    branch: None,
                    stamp: None,
                    disk: None,
                })
                .unwrap();
            app_state.poll_listings();
            assert!(app_state.error.is_some());
            app_state.reset_error();
        } else {
            app_state.cancel_listing(true);
        }
        assert!(!app_state.panel_busy(true));
        assert_eq!(app_state.tabs_left.active, 0);
        assert_eq!(app_state.dir_left, dir);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The watcher sees a directory change and the panel reads it again; the
/// detail lines and the preview arrive from their threads, the name first.
#[test]
fn changes_and_details_arrive_from_their_threads() {
    let dir = std::env::temp_dir().join(format!("fm84-watched-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();

    let mut app_state = AppState::new();
    app_state.watch_interval = std::time::Duration::from_millis(5);
    app_state.open_dir(true, dir.clone(), Some("notes.txt".as_ref()));

    // Made behind fm84's back, and found without anything being pressed.
    std::fs::write(dir.join("new.txt"), "").unwrap();
    let started = std::time::Instant::now();
    while !app_state.children_left.iter().any(|item| item.name_full == "new.txt") {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the change was never seen");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_watchers();
        app_state.refresh_stale_panels();
    }

    // Without waiting on them: the name at once, the rest when it comes.
    app_state.detail_wait = std::time::Duration::ZERO;
    app_state.show_preview = true;
    let at = app_state.children_left.iter().position(|item| item.name_full == "notes.txt");
    app_state.state_left.select(at);
    app_state.cursor_detail = None;
    app_state.refresh_cursor_detail();
    assert_eq!(app_state.cursor_detail.as_ref().unwrap().name, "notes.txt");
    let started = std::time::Instant::now();
    loop {
        app_state.refresh_cursor_detail();
        app_state.refresh_preview();
        let detail = app_state.cursor_detail.as_ref().unwrap();
        let preview = app_state.preview.as_ref().unwrap();
        if !detail.awaiting && !preview.awaiting {
            assert_eq!(detail.size, "5 bytes");
            assert_eq!(preview.lines, ["hello"]);
            break;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the details never came");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_click_lands_on_the_icon_it_looks_like() {
    // Seven icons, each a glyph and the space after it, past the space the
    // strip opens with - the layout drive_strip actually produces.
    let slots: Vec<(u16, u16)> = (0..7).map(|i| (1 + i * 2, 2)).collect();

    assert_eq!(slot_at(&slots, 0), None); // the opening space
    assert_eq!(slot_at(&slots, 1), Some(0)); // first glyph
    assert_eq!(slot_at(&slots, 2), Some(0)); // the space that goes with it
    assert_eq!(slot_at(&slots, 3), Some(1));
    assert_eq!(slot_at(&slots, 13), Some(6)); // last icon
    assert_eq!(slot_at(&slots, 14), Some(6));
    assert_eq!(slot_at(&slots, 15), None); // past the end, where the label goes
    assert_eq!(slot_at(&slots, 200), None);

    // Every column of every slot maps back to that slot, and they never
    // overlap - a gap or an overlap here is a click landing one icon over.
    for (index, &(start, width)) in slots.iter().enumerate() {
        for offset in start..start + width {
            assert_eq!(slot_at(&slots, offset), Some(index), "offset {offset}");
        }
    }

    assert_eq!(slot_at(&[], 1), None);
}

/// A reread still on its way when a newer one comes straight back is
/// dropped. Left waiting, it landed afterwards with the directory as it
/// was - a file a copy had just put there vanished from the panel, and the
/// watcher, which had already said the directory changed, never said so
/// again.
#[test]
fn a_slower_older_listing_does_not_land_over_a_newer_one() {
    use crate::app::listing::{PendingListing, Travel, read_listing};

    let dir = std::env::temp_dir().join(format!("fm84-older-listing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("old"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.open_dir(true, dir.clone(), None);

    // A reread held up on a slow disk, which saw only "old".
    let older = read_listing(&dir, &app_state.options);
    let (sender, answer) = std::sync::mpsc::channel();
    app_state.listing_left = Some(PendingListing {
        dir: dir.clone(),
        navigate: false,
        select: None,
        tab: None,
        travel: Travel::Stay,
        answer,
    });

    std::fs::write(dir.join("new"), "").unwrap();
    app_state.reload_panel(true, None);
    assert!(app_state.listing_left.is_none(), "the older one is no longer waited for");

    let _ = sender.send(older);
    app_state.poll_listings();
    let names: Vec<&str> = app_state.children_left.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["..", "new", "old"]);
    std::fs::remove_dir_all(&dir).unwrap();
}
