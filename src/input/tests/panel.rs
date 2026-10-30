//! Keys in the panels: names to the clipboard, swapping, sorting, going places.

use super::*;

/// Ctrl+C copies full paths and Alt+C names: of the selection, of the
/// entry under the cursor, or on the parent entry of the panel itself.
#[test]
fn copy_paths_and_names_from_a_panel() {
    let dir = std::env::temp_dir().join(format!("fm84-copy-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "").unwrap();
    std::fs::write(dir.join("b.txt"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("b.txt".as_ref()));
    let chord = |app_state: &mut AppState, modifiers| handle_key(app_state, KeyEvent::new(KeyCode::Char('c'), modifiers));

    chord(&mut app_state, KeyModifiers::CONTROL);
    assert_eq!(app_state.clipboard, dir.join("b.txt").to_string_lossy());
    assert!(app_state.find_note.as_deref().unwrap().starts_with("Copied path: "));
    chord(&mut app_state, KeyModifiers::ALT);
    assert_eq!(app_state.clipboard, "b.txt");

    app_state.selected_left.insert("a.txt".into());
    app_state.selected_left.insert("b.txt".into());
    chord(&mut app_state, KeyModifiers::ALT);
    assert_eq!(app_state.clipboard, "a.txt\nb.txt");
    assert_eq!(app_state.find_note.as_deref(), Some("Copied 2 names"));
    // The note goes with the next key.
    press(&mut app_state, KeyCode::Down);
    assert!(app_state.find_note.is_none());

    app_state.selected_left.clear();
    app_state.state_left.select(Some(0));
    chord(&mut app_state, KeyModifiers::CONTROL);
    assert_eq!(app_state.clipboard, dir.to_string_lossy());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+U swaps the panels, tabs, cursors and selections with them, and
/// the focus goes along; Alt+I sends the other panel here.
#[test]
fn swap_the_panels_and_send_the_other_one_here() {
    let dir = std::env::temp_dir().join(format!("fm84-swap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["one", "two", "three"] {
        std::fs::create_dir_all(dir.join(name)).unwrap();
    }
    std::fs::write(dir.join("one").join("a.txt"), "").unwrap();
    std::fs::write(dir.join("one").join("b.txt"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.join("one"), Some("b.txt".as_ref()));
    app_state.selected_left.insert("a.txt".into());
    app_state.open_dir(false, dir.join("two"), None);
    app_state.is_left_active = false;
    app_state.new_tab();
    app_state.open_dir(false, dir.join("three"), None);
    app_state.is_left_active = true;
    let chord = |app_state: &mut AppState, c, modifiers| handle_key(app_state, KeyEvent::new(KeyCode::Char(c), modifiers));
    let cursor = |app_state: &AppState, is_left: bool| {
        let (children, state) = if is_left { (&app_state.children_left, &app_state.state_left) } else { (&app_state.children_right, &app_state.state_right) };
        children[state.selected().unwrap()].name_full.clone()
    };

    chord(&mut app_state, 'u', KeyModifiers::CONTROL);
    assert_eq!((app_state.dir_left.clone(), app_state.dir_right.clone()), (dir.join("three"), dir.join("one")));
    assert_eq!((app_state.tabs_left.list.len(), app_state.tabs_left.active, app_state.tabs_right.list.len()), (2, 1, 1));
    assert!(!app_state.is_left_active);
    assert_eq!(cursor(&app_state, false), "b.txt");
    assert!(app_state.selected_right.contains(std::ffi::OsStr::new("a.txt")) && app_state.selected_left.is_empty());
    // The other tab went across too, as it was.
    app_state.select_tab(true, 0);
    assert_eq!(app_state.dir_left, dir.join("two"));

    chord(&mut app_state, 'i', KeyModifiers::ALT);
    assert_eq!(app_state.dir_left, dir.join("one"));
    assert_eq!(cursor(&app_state, true), "b.txt");
    assert!(!app_state.is_left_active);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+F3 to Ctrl+F6 and a click on a heading sort both panels.
#[test]
fn sort_from_the_keys_and_the_headings() {
    use crate::options::{Options, SortKey};
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let dir = std::env::temp_dir().join(format!("fm84-sort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a long one").unwrap();
    std::fs::write(dir.join("b.rs"), "short").unwrap();
    std::fs::write(dir.join("c.md"), "mid size").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("b.rs".as_ref()));
    let names = |app_state: &AppState| app_state.children_left.iter().skip(1).map(|item| item.name_full.clone()).collect::<Vec<_>>();
    let control = |app_state: &mut AppState, n| handle_key(app_state, KeyEvent::new(KeyCode::F(n), KeyModifiers::CONTROL));

    // Ctrl+F3 sorts by name, and again turns it round, the cursor
    // staying on the entry it was on.
    control(&mut app_state, 3);
    assert_eq!(names(&app_state), ["a.txt", "b.rs", "c.md"]);
    control(&mut app_state, 3);
    assert!(app_state.options.sort_descending);
    assert_eq!(names(&app_state), ["c.md", "b.rs", "a.txt"]);
    control(&mut app_state, 4);
    assert_eq!((app_state.options.sort_key, app_state.options.sort_descending), (SortKey::Extension, false));
    assert_eq!(names(&app_state), ["c.md", "b.rs", "a.txt"]);
    let cursor = app_state.state_left.selected().unwrap();
    assert_eq!(app_state.children_left[cursor].name_full, "b.rs");

    // A click on the Size heading sorts by size, arrow and all.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let (cell, _) = *app_state.sort_headers.iter().find(|(_, key)| *key == SortKey::Size).unwrap();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: cell.x,
        row: cell.y,
        modifiers: KeyModifiers::NONE,
    };
    crate::input::handle_mouse(&mut app_state, click);
    assert_eq!(app_state.options.sort_key, SortKey::Size);
    assert_eq!(names(&app_state), ["b.rs", "c.md", "a.txt"]);
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let heading: String = (cell.x..cell.x + cell.width).map(|x| terminal.backend().buffer()[(x, cell.y)].symbol().to_string()).collect();
    assert_eq!(heading.trim_end(), "Size ↑");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+L goes where it is told - a directory, or a file to land on -
/// and Alt+Left and Alt+Right step back and forth through where the tab
/// has been, landing on the entry the cursor was on.
#[test]
fn go_to_a_path_and_back_and_forward() {
    let dir = std::env::temp_dir().join(format!("fm84-go-to-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
    std::fs::write(dir.join("a.txt"), "text").unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("sub".as_ref()));
    let alt = |app_state: &mut AppState, code: KeyCode| handle_key(app_state, KeyEvent::new(code, KeyModifiers::ALT));
    let ctrl_l = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    let cursor = |app_state: &AppState| {
        let index = app_state.state_left.selected().unwrap();
        app_state.children_left[index].name.clone()
    };
    let type_in = |app_state: &mut AppState, text: &str| {
        let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &mut app_state.dialog else { panic!("no prompt") };
        input.set(text.to_string());
    };

    // It opens on where the panel is, ready to have a name typed on.
    ctrl_l(&mut app_state);
    let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &app_state.dialog else { panic!("no prompt") };
    assert_eq!(input.text, format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR));
    // Tab fills out the name, and Enter goes there.
    press(&mut app_state, KeyCode::Char('s'));
    press(&mut app_state, KeyCode::Tab);
    press(&mut app_state, KeyCode::Char('d'));
    press(&mut app_state, KeyCode::Tab);
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

    // A path from the panel, `..` and all, to a file: landed on, in the
    // directory it is in.
    ctrl_l(&mut app_state);
    type_in(&mut app_state, "../../a.txt");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!((app_state.dir_left.clone(), cursor(&app_state)), (dir.clone(), "a".to_string()));

    // Somewhere that is not there is said to be, and the panel stays.
    ctrl_l(&mut app_state);
    type_in(&mut app_state, "nowhere");
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.take().unwrap().contains("nowhere"));
    assert_eq!(app_state.dir_left, dir);

    // Back the way it came, onto what the cursor was on, and forward.
    alt(&mut app_state, KeyCode::Left);
    assert_eq!(app_state.dir_left, dir.join("sub/deeper"));
    alt(&mut app_state, KeyCode::Left);
    assert_eq!((app_state.dir_left.clone(), cursor(&app_state)), (dir.clone(), "sub".to_string()));
    alt(&mut app_state, KeyCode::Right);
    assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

    // A step somewhere new drops what was ahead.
    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(app_state.dir_left, dir.join("sub"));
    alt(&mut app_state, KeyCode::Right);
    assert_eq!(app_state.dir_left, dir.join("sub"));
    alt(&mut app_state, KeyCode::Left);
    assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

    // A directory gone since is passed over, rather than stuck on: from
    // sub, back to deeper climbs out of it to sub again, and on to dir.
    std::fs::remove_dir(dir.join("sub/deeper")).unwrap();
    alt(&mut app_state, KeyCode::Right);
    assert_eq!(app_state.dir_left, dir.join("sub"));
    alt(&mut app_state, KeyCode::Left);
    assert_eq!(app_state.dir_left, dir, "the gone one was passed over");
    alt(&mut app_state, KeyCode::Right);
    assert_eq!(app_state.dir_left, dir.join("sub"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+Shift+Enter sizes every directory in the panel - Alt+Enter too -
/// a few at a time with the rest waiting their turn, and not through a link
/// to a directory, which could lead anywhere. Waiting shows as "…" in the
/// Size column until each is counted.
#[cfg(unix)]
#[test]
fn alt_shift_enter_sizes_every_directory() {
    let dir = std::env::temp_dir().join(format!("fm84-size-all-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let count = crate::constants::SIZE_WORKERS * 2 + 1;
    for n in 0..count {
        let sub = dir.join(format!("dir{n:02}"));
        std::fs::create_dir_all(sub.join("inner")).unwrap();
        std::fs::write(sub.join("a.bin"), vec![0u8; n]).unwrap();
        std::fs::write(sub.join("inner").join("b.bin"), vec![0u8; 100]).unwrap();
    }
    std::fs::write(dir.join("file.txt"), "not a directory").unwrap();
    std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();

    for modifiers in [KeyModifiers::ALT | KeyModifiers::SHIFT, KeyModifiers::ALT] {
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), None);
        handle_key(&mut app_state, KeyEvent::new(KeyCode::Enter, modifiers));
        assert_eq!(app_state.dir_left, dir, "it sizes, and does not go in");

        // A few at once, the rest waiting, and shown as waiting.
        assert_eq!(app_state.dir_sizing.len(), crate::constants::SIZE_WORKERS);
        assert_eq!(app_state.dir_size_queue.len(), count - crate::constants::SIZE_WORKERS);
        assert_eq!(app_state.dir_size_so_far(app_state.dir_size_queue.back().unwrap()), Some(0));

        let started = std::time::Instant::now();
        while !app_state.dir_sizing.is_empty() || !app_state.dir_size_queue.is_empty() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "sizing never finished");
            assert!(app_state.dir_sizing.len() <= crate::constants::SIZE_WORKERS);
            std::thread::sleep(std::time::Duration::from_millis(2));
            app_state.poll_dir_sizes();
        }
        for n in 0..count {
            assert_eq!(app_state.dir_sizes.get(&dir.join(format!("dir{n:02}"))), Some(&(n as u64 + 100)), "dir{n:02}");
        }
        assert_eq!(app_state.dir_sizes.len(), count, "neither the link nor the file");
    }

    // Esc stops what is running and what is waiting.
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT));
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dir_sizing.is_empty() && app_state.dir_size_queue.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Space on a directory while a panel's worth are waiting is sized next,
/// ahead of them, since it was asked for on its own.
#[test]
fn a_directory_sized_on_its_own_goes_ahead_of_those_waiting() {
    let mut app_state = AppState::new();
    // Every worker busy, so nothing more starts, and the order shows.
    for n in 0..crate::constants::SIZE_WORKERS {
        app_state.start_dir_size(std::path::PathBuf::from(format!("/nonexistent/fm84/busy{n}")));
    }
    let waiting: Vec<std::path::PathBuf> = (0..3).map(|n| std::path::PathBuf::from(format!("/nonexistent/fm84/{n}"))).collect();
    app_state.dir_size_queue.extend(waiting.iter().cloned());
    app_state.start_dir_size(waiting[2].clone());
    assert_eq!(app_state.dir_size_queue.front(), Some(&waiting[2]));
    assert_eq!(app_state.dir_size_queue.len(), 3, "moved, not added again");
    app_state.cancel_dir_sizes();
}

/// Leaving a directory stops sizing what is in it, running and waiting -
/// nothing shows those sizes any more - unless the other panel is in it too.
/// Sizes already counted are kept, for coming back.
#[test]
fn leaving_a_directory_stops_sizing_what_is_in_it() {
    let dir = std::env::temp_dir().join(format!("fm84-size-leave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for n in 0..crate::constants::SIZE_WORKERS * 2 {
        std::fs::create_dir_all(dir.join(format!("d{n}"))).unwrap();
    }
    let elsewhere = std::env::temp_dir();
    let size_all = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT));

    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    app_state.open_dir(false, elsewhere.clone(), None);
    app_state.dir_sizes.insert(dir.join("d0"), 42);
    size_all(&mut app_state);
    assert!(!app_state.dir_sizing.is_empty() && !app_state.dir_size_queue.is_empty());
    app_state.open_dir(true, elsewhere.clone(), None);
    assert!(app_state.dir_sizing.is_empty(), "running ones stopped");
    assert!(app_state.dir_size_queue.is_empty(), "waiting ones dropped");
    assert_eq!(app_state.dir_sizes.get(&dir.join("d0")), Some(&42), "a size counted before is kept");

    // The other panel still in it: they go on, for that panel to show.
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    app_state.open_dir(false, dir.clone(), None);
    size_all(&mut app_state);
    let asked = app_state.dir_sizing.len() + app_state.dir_size_queue.len();
    app_state.open_dir(true, elsewhere.clone(), None);
    assert_eq!(app_state.dir_sizing.len() + app_state.dir_size_queue.len(), asked);
    app_state.cancel_dir_sizes();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ctrl+S narrows the panel as the pattern is typed, keeping the parent
/// entry and letting go of what is selected but no longer shown. Esc puts
/// back the filter there was, Enter keeps it; a reread is narrowed too,
/// and going somewhere else drops it.
#[test]
fn ctrl_s_filters_the_panel_as_it_is_typed() {
    let dir = std::env::temp_dir().join(format!("fm84-filter-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("reports")).unwrap();
    for name in ["report.txt", "notes.md", "main.rs"] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("notes.md".as_ref()));
    app_state.selected_left.insert("main.rs".into());
    app_state.selected_left.insert("report.txt".into());
    // In order of name, whatever F11 sorts by.
    let names = |app_state: &AppState| {
        let mut names: Vec<String> = app_state.children_left.iter().map(|item| item.name_full.clone()).collect();
        names.sort();
        names
    };
    let shown = names(&app_state);
    let ctrl_s = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

    ctrl_s(&mut app_state);
    assert!(matches!(app_state.dialog, Some(Dialog::Prompt(PromptKind::Filter, _))));
    for c in "rep".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    assert_eq!(names(&app_state), ["..", "report.txt", "reports"]);
    // The cursor's entry went, so it is on the first match.
    assert_eq!(app_state.state_left.selected(), Some(1));
    assert!(!app_state.children_left[1].is_parent());
    assert_eq!(app_state.selected_left, std::collections::HashSet::from(["report.txt".into()]));

    // Esc puts back what there was before: nothing.
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && app_state.filter_left.is_none());
    assert_eq!(names(&app_state), shown);

    ctrl_s(&mut app_state);
    for c in "*.md;*.rs".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(names(&app_state), ["..", "main.rs", "notes.md"]);
    assert_eq!(app_state.filter_left.as_ref().map(|filter| filter.text.as_str()), Some("*.md;*.rs"));

    // Opened again, it starts from the filter in place, which Esc keeps.
    ctrl_s(&mut app_state);
    press(&mut app_state, KeyCode::Backspace);
    assert_eq!(names(&app_state), ["..", "notes.md"]);
    press(&mut app_state, KeyCode::Esc);
    assert_eq!(names(&app_state), ["..", "main.rs", "notes.md"]);

    // A file arriving is narrowed as everything else is.
    std::fs::write(dir.join("lib.rs"), "").unwrap();
    std::fs::write(dir.join("todo.txt"), "").unwrap();
    app_state.reload_panel(true, None);
    assert_eq!(names(&app_state), ["..", "lib.rs", "main.rs", "notes.md"]);

    // Somewhere else, and it is gone - also on coming back.
    app_state.open_dir(true, dir.join("reports"), None);
    assert!(app_state.filter_left.is_none());
    app_state.open_dir(true, dir.clone(), None);
    assert_eq!(app_state.children_left.len(), shown.len() + 2);

    std::fs::remove_dir_all(&dir).unwrap();
}
