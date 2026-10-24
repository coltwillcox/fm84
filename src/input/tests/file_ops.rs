//! Copying, moving, linking, comparing and changing attributes, from the keys.

use super::*;

/// Shift+F3 compares the file under the cursor with its namesake in the
/// other panel, or two selected in one; each side is the panel its file is
/// in. Files that are the same, or not text, are only spoken of.
#[test]
fn compare_files_from_the_keys() {
    let dir = std::env::temp_dir().join(format!("fm84-compare-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (one, two) = (dir.join("one"), dir.join("two"));
    std::fs::create_dir_all(one.join("sub")).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("a.txt"), "same\nleft\nsame\n").unwrap();
    std::fs::write(one.join("b.txt"), "same\nb\nsame\n").unwrap();
    std::fs::write(two.join("a.txt"), "same\nright\nsame\nextra\n").unwrap();
    std::fs::write(two.join("copy.txt"), "same\nleft\nsame\n").unwrap();
    let mut app_state = AppState::new();
    app_state.open_dir(false, two.clone(), Some("copy.txt".as_ref()));
    app_state.is_left_active = true;
    app_state.open_dir(true, one.clone(), Some("a.txt".as_ref()));
    let compare = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::F(3), KeyModifiers::SHIFT));
    let sides = |app_state: &AppState| match &app_state.screen {
        Screen::Diff(state) => (state.left_path.clone(), state.right_path.clone()),
        _ => panic!("not comparing"),
    };

    // The namesake, not the other panel's cursor.
    compare(&mut app_state);
    assert_eq!(sides(&app_state), (one.join("a.txt"), two.join("a.txt")));
    let Screen::Diff(state) = &app_state.screen else { unreachable!() };
    assert_eq!(state.hunks.len(), 2);

    // Drawn side by side, with the marks between, and its own keys.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    for shown in [" | ", " > ", "left", "right", "extra", "Difference 1/2", "F3 Next"] {
        assert!(screen.contains(shown), "{shown} not drawn");
    }
    press(&mut app_state, KeyCode::F(3));
    let Screen::Diff(state) = &app_state.screen else { unreachable!() };
    assert_eq!(state.current, Some(1));
    press(&mut app_state, KeyCode::Esc);
    assert!(matches!(app_state.screen, Screen::Panels));

    // = does the same, for a terminal that cannot send Shift+F3.
    press(&mut app_state, KeyCode::Char('='));
    assert_eq!(sides(&app_state), (one.join("a.txt"), two.join("a.txt")));
    press(&mut app_state, KeyCode::Esc);

    // From the right panel, the left file is still on the left.
    app_state.is_left_active = false;
    app_state.open_dir(false, two.clone(), Some("a.txt".as_ref()));
    compare(&mut app_state);
    assert_eq!(sides(&app_state), (one.join("a.txt"), two.join("a.txt")));
    press(&mut app_state, KeyCode::Esc);

    // No namesake: the other panel's cursor, here a file the same.
    app_state.open_dir(false, two.clone(), Some("copy.txt".as_ref()));
    compare(&mut app_state);
    assert!(matches!(app_state.screen, Screen::Panels));
    assert_eq!(app_state.find_note.as_deref(), Some("The files are identical"));

    // Two selected in one panel.
    app_state.is_left_active = true;
    app_state.selected_left.insert("a.txt".into());
    app_state.selected_left.insert("b.txt".into());
    compare(&mut app_state);
    assert_eq!(sides(&app_state), (one.join("a.txt"), one.join("b.txt")));
    press(&mut app_state, KeyCode::Esc);
    app_state.selected_left.clear();

    // A directory is not a file to compare.
    app_state.open_dir(true, one.clone(), Some("sub".as_ref()));
    compare(&mut app_state);
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Put the cursor on a file")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ctrl+Shift+F5 links the entry under the cursor into the other panel,
/// under its own name or one typed; several selected get one each, and one
/// already there is named while the rest are still made.
#[cfg(unix)]
#[test]
fn symlinks_from_the_keys() {
    let dir = std::env::temp_dir().join(format!("fm84-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (src, dst) = (dir.join("src"), dir.join("dst"));
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(src.join(name), name).unwrap();
    }
    let mut app_state = AppState::new();
    app_state.open_dir(false, dst.clone(), None);
    app_state.is_left_active = true;
    app_state.open_dir(true, src.clone(), Some("a.txt".as_ref()));
    let link = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::F(5), KeyModifiers::CONTROL | KeyModifiers::SHIFT));
    let typed = |app_state: &mut AppState| app_state.dialog_input().unwrap().text.clone();

    link(&mut app_state);
    assert_eq!(typed(&mut app_state), dst.join("a.txt").to_string_lossy());
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.dialog.is_none() && app_state.error.is_none());
    assert_eq!(std::fs::read_link(dst.join("a.txt")).unwrap(), src.join("a.txt"));
    let cursor = app_state.state_right.selected().map(|index| app_state.children_right[index].name_os.clone());
    assert_eq!(cursor.as_deref(), Some("a.txt".as_ref()), "the other panel lands on the new link");

    // Again: refused, nothing written over.
    link(&mut app_state);
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Cannot link") && error.contains("already exists")));
    app_state.error = None;

    // A bare name is made beside the original.
    link(&mut app_state);
    app_state.dialog_input().unwrap().set("alias".to_string());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(std::fs::read_link(src.join("alias")).unwrap(), src.join("a.txt"));

    // A lone directory is named, not gone into.
    app_state.open_dir(true, src.clone(), Some("sub".as_ref()));
    link(&mut app_state);
    assert_eq!(typed(&mut app_state), dst.join("sub").to_string_lossy());
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(std::fs::read_link(dst.join("sub")).unwrap(), src.join("sub"));

    // Several: into the directory, one already there passed over.
    app_state.selected_left.insert("a.txt".into());
    app_state.selected_left.insert("b.txt".into());
    link(&mut app_state);
    assert!(typed(&mut app_state).ends_with(std::path::MAIN_SEPARATOR));
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Could not link 1 of 2")));
    assert_eq!(std::fs::read_link(dst.join("b.txt")).unwrap(), src.join("b.txt"));

    // Esc leaves it be.
    app_state.error = None;
    app_state.selected_left.clear();
    app_state.open_dir(true, src.clone(), Some("b.txt".as_ref()));
    link(&mut app_state);
    app_state.dialog_input().unwrap().set("never".to_string());
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && !crate::fs_ops::path_exists(&src.join("never")));

    // Alt+F5 does the same.
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(5), KeyModifiers::ALT));
    assert!(matches!(app_state.dialog, Some(Dialog::Symlink { .. })));
    press(&mut app_state, KeyCode::Esc);

    // Ctrl+F5 alone still sorts.
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(5), KeyModifiers::CONTROL));
    assert!(app_state.dialog.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Alt+A opens on what the entries have in common; what is typed over it
/// is applied to each, and what cannot be read leaves the form open.
#[cfg(unix)]
#[test]
fn attributes_change_from_the_keys() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fm84-alt-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, mode) in [("a.txt", 0o644), ("b.txt", 0o600)] {
        std::fs::write(dir.join(name), "").unwrap();
        std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(mode)).unwrap();
    }
    let mode = |name: &str| std::fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o7777;
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("a.txt".as_ref()));
    let alt_a = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT));
    let fields = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Attributes(form)) => form.fields.iter().map(|field| field.input.text.clone()).collect::<Vec<_>>(),
        _ => panic!("not open"),
    };
    let retype = |app_state: &mut AppState, text: &str| {
        app_state.dialog_input().unwrap().set(text.to_string());
    };

    alt_a(&mut app_state);
    let opened = fields(&app_state);
    assert_eq!(opened[0], "644");
    assert_eq!(opened[1], crate::fs_ops::user_name(unsafe { libc::getuid() }));
    retype(&mut app_state, "u+x");
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.dialog.is_none() && app_state.error.is_none());
    assert_eq!(mode("a.txt"), 0o744);

    // Two that differ: the mode opens empty, and stays as it is.
    app_state.selected_left.insert("a.txt".into());
    app_state.selected_left.insert("b.txt".into());
    alt_a(&mut app_state);
    assert_eq!(fields(&app_state)[0], "");
    press(&mut app_state, KeyCode::BackTab);
    retype(&mut app_state, "2001-02-03 04:05:06");
    press(&mut app_state, KeyCode::Enter);
    let then = crate::attributes::parse_time("2001-02-03 04:05:06").unwrap();
    for name in ["a.txt", "b.txt"] {
        assert_eq!(std::fs::metadata(dir.join(name)).unwrap().modified().unwrap(), then);
    }
    assert_eq!((mode("a.txt"), mode("b.txt")), (0o744, 0o600));

    // A click on a field takes the keys there, the cursor where it
    // landed; one on the rest of the popup does nothing.
    alt_a(&mut app_state);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let click = |app_state: &mut AppState, column, row| {
        crate::input::mouse_to(
            app_state,
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    };
    let (group_row, text_x) = app_state.attribute_rows[2];
    click(&mut app_state, text_x + 1, group_row.y);
    let focus = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Attributes(form)) => (form.focus, form.fields[form.focus].input.cursor),
        _ => panic!("not open"),
    };
    assert_eq!(focus(&app_state), (2, 1));
    let (modified_row, _) = app_state.attribute_rows[3];
    click(&mut app_state, modified_row.right() - 1, modified_row.y);
    assert_eq!(focus(&app_state), (3, "2001-02-03 04:05:06".len()));
    // Above the first field: the line naming what is changed, or a blank
    // row, whichever the popup's width leaves there.
    let (mode_row, _) = app_state.attribute_rows[0];
    click(&mut app_state, mode_row.x, mode_row.y - 1);
    assert_eq!(focus(&app_state).0, 3);
    press(&mut app_state, KeyCode::Esc);

    // Not a mode: said so, and the form is still there to fix.
    alt_a(&mut app_state);
    retype(&mut app_state, "u+q");
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.is_some() && matches!(app_state.dialog, Some(Dialog::Attributes(_))));
    press(&mut app_state, KeyCode::Esc);
    // Drawn, then put away.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_typed_path_starts_from_the_panel_or_from_home() {
    let base = std::path::Path::new("/panel/dir");
    let home = std::env::home_dir().unwrap_or_default();
    assert_eq!(crate::input::typed_path("/abs/x", base), std::path::Path::new("/abs/x"));
    assert_eq!(crate::input::typed_path("x", base), base.join("x"));
    assert_eq!(crate::input::typed_path("sub/x", base), base.join("sub/x"));
    assert_eq!(crate::input::typed_path("~", base), home);
    assert_eq!(crate::input::typed_path("~/x", base), home.join("x"));
    // Only a `~` that stands for home: one starting a name is the name.
    assert_eq!(crate::input::typed_path("~x", base), base.join("~x"));
}

/// F5 and F6 go where the field says. A single file opens on its path in
/// the other panel, anything else on the other panel's directory, and
/// either goes there when it is left as it opened. A name edited in
/// renames it on the way, and a bare one puts it beside the original. Y
/// and N are typed, not taken for an answer.
#[test]
fn copy_and_move_go_where_the_destination_says() {
    let dir = std::env::temp_dir().join(format!("fm84-dest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (left, right) = (dir.join("left"), dir.join("right"));
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("a.txt"), "a").unwrap();
    std::fs::write(left.join("z.txt"), "z").unwrap();
    std::fs::create_dir(left.join("sub")).unwrap();

    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(false, right.clone(), None);
    let on_a = |app_state: &mut AppState| {
        app_state.open_dir(true, left.clone(), Some("a.txt".as_ref()));
    };
    let finish = |app_state: &mut AppState| {
        let started = std::time::Instant::now();
        while app_state.job.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
    };
    let type_in = |app_state: &mut AppState, text: &str| {
        app_state.dialog_input().unwrap().set(text.to_string());
        press(app_state, KeyCode::Enter);
    };

    // A single file opens on its path in the other panel, and Enter alone
    // copies it there.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(5));
    assert_eq!(std::path::PathBuf::from(&app_state.dialog_input().unwrap().text), right.join("a.txt"));
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert_eq!(std::fs::read_to_string(right.join("a.txt")).unwrap(), "a");

    // Again, onto the copy now there: the job stops to ask about it, and
    // Esc leaves it as it was.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    let started = std::time::Instant::now();
    while !app_state.job.as_ref().is_some_and(|job| job.clash.is_some()) {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never asked");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    press(&mut app_state, KeyCode::Esc);
    finish(&mut app_state);
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Copy cancelled")), "{:?}", app_state.error);
    assert_eq!(std::fs::read_to_string(right.join("a.txt")).unwrap(), "a");
    app_state.reset_error();

    // The name edited, one key at a time.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(5));
    for _ in "a.txt".chars() {
        press(&mut app_state, KeyCode::Backspace);
    }
    for c in "b.txt".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    finish(&mut app_state);
    assert!(right.join("b.txt").exists());

    // N is typed into the field, and Esc leaves it.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Char('n'));
    assert!(app_state.dialog_input().unwrap().text.ends_with('n'));
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && app_state.job.is_none());

    // A bare name is beside the original.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(5));
    type_in(&mut app_state, "dup.txt");
    finish(&mut app_state);
    assert_eq!(std::fs::read_to_string(left.join("dup.txt")).unwrap(), "a");

    // F6 to a new name in the same place is a rename.
    on_a(&mut app_state);
    press(&mut app_state, KeyCode::F(6));
    type_in(&mut app_state, "moved.txt");
    finish(&mut app_state);
    assert!(!left.join("a.txt").exists() && left.join("moved.txt").exists());

    // A directory in the way of a file is refused, not copied into, when
    // the field is left as it opened.
    std::fs::create_dir(right.join("z.txt")).unwrap();
    app_state.open_dir(true, left.clone(), Some("z.txt".as_ref()));
    press(&mut app_state, KeyCode::F(5));
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.as_deref().is_some_and(|error| error.contains("Cannot replace directory")), "{:?}", app_state.error);
    assert!(!right.join("z.txt").join("z.txt").exists());
    app_state.reset_error();

    // A directory, and several entries, open on the other panel's
    // directory alone.
    let opened_on_directory = |app_state: &mut AppState| {
        let text = app_state.dialog_input().unwrap().text.clone();
        std::path::Path::new(&text) == right && text.ends_with(std::path::MAIN_SEPARATOR)
    };
    app_state.open_dir(true, left.clone(), Some("sub".as_ref()));
    press(&mut app_state, KeyCode::F(5));
    assert!(opened_on_directory(&mut app_state));
    press(&mut app_state, KeyCode::Esc);
    app_state.open_dir(true, left.clone(), None);
    app_state.selected_left.extend(["dup.txt".into(), "z.txt".into()]);
    press(&mut app_state, KeyCode::F(5));
    assert!(opened_on_directory(&mut app_state));

    // And several cannot share one name.
    type_in(&mut app_state, "one.txt");
    assert!(app_state.error.as_deref().is_some_and(|error| error.contains("2 items need a directory")), "{:?}", app_state.error);
    assert!(!left.join("one.txt").exists());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// The other panel on its way to a directory slow to read: F5 offers the
/// one it is going to, not the one it is leaving.
#[test]
fn a_copy_goes_where_the_other_panel_is_going() {
    let dir = std::env::temp_dir().join(format!("fm84-copy-pending-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["from", "leaving", "going"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    std::fs::write(dir.join("from").join("a.txt"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.join("from"), Some("a.txt".as_ref()));
    app_state.open_dir(false, dir.join("leaving"), None);
    app_state.listing_right = Some(crate::app::PendingListing::going_to(dir.join("going")));

    press(&mut app_state, KeyCode::F(5));
    assert_eq!(std::path::PathBuf::from(&app_state.dialog_input().unwrap().text), dir.join("going").join("a.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}
