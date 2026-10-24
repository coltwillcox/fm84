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
