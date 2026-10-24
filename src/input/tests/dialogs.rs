//! Keys reaching what is on top, and the dialogs they drive.

use super::*;

/// Keys go to whatever is on top - an error, then a dialog, then the
/// screen - and to nothing underneath it.
#[test]
fn a_key_reaches_only_what_is_on_top() {
    let dir = std::env::temp_dir().join(format!("fm84-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "text").unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("a.txt".as_ref()));

    // Space behind an error selects nothing, and Enter puts the error away.
    app_state.display_error("in the way".to_string());
    assert!(press(&mut app_state, KeyCode::Char(' ')));
    assert!(app_state.selected_left.is_empty() && app_state.error.is_some());
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.is_none());

    // F1 opens help over the panels; typing there searches nothing.
    press(&mut app_state, KeyCode::F(1));
    assert!(matches!(app_state.dialog, Some(Dialog::Help)));
    press(&mut app_state, KeyCode::Char('a'));
    assert!(app_state.search_input.is_empty());
    // Too long for the panels of a short terminal: it scrolls, as far as
    // its last line and no further.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let last = crate::ui::help_lines().len() - app_state.help_room;
    press(&mut app_state, KeyCode::End);
    assert_eq!(app_state.help_scroll, last);
    press(&mut app_state, KeyCode::Down);
    assert_eq!(app_state.help_scroll, last);
    press(&mut app_state, KeyCode::Home);
    press(&mut app_state, KeyCode::Up);
    assert_eq!(app_state.help_scroll, 0);
    press(&mut app_state, KeyCode::F(1));
    assert!(app_state.dialog.is_none());

    // F2 types into the rename field, and Esc leaves the name alone.
    press(&mut app_state, KeyCode::F(2));
    press(&mut app_state, KeyCode::Char('x'));
    assert_eq!(app_state.dialog_input().unwrap().text, "a.txtx");
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && dir.join("a.txt").exists());

    // In the editor, Esc over edits asks; Esc again goes back to them.
    press(&mut app_state, KeyCode::F(4));
    assert!(matches!(app_state.screen, Screen::Editor(_)));
    press(&mut app_state, KeyCode::Char('!'));
    press(&mut app_state, KeyCode::Esc);
    assert!(matches!(app_state.dialog, Some(Dialog::SaveChanges { then_quit: false })));
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && app_state.is_editing());
    // F10 asks too, and Discard then leaves.
    assert!(press(&mut app_state, KeyCode::F(10)));
    assert!(!press(&mut app_state, KeyCode::Char('n')));
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "text");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Moving about the Options dialog and typing into a text row, put
/// back with Esc. Nothing here changes a value, so nothing is saved over
/// the real config.
#[test]
fn the_options_dialog_moves_and_edits_from_the_keys() {
    use crate::options::{OPTION_ROWS, OptionRow};
    let mut app_state = AppState::new();
    press(&mut app_state, KeyCode::F(11));
    assert!(matches!(app_state.dialog, Some(Dialog::Options { editing: None })));

    press(&mut app_state, KeyCode::Up);
    assert_eq!(app_state.options_cursor, 0);
    press(&mut app_state, KeyCode::Down);
    press(&mut app_state, KeyCode::Down);
    assert_eq!(app_state.options_cursor, 2);
    for _ in 0..OPTION_ROWS.len() {
        press(&mut app_state, KeyCode::Down);
    }
    assert_eq!(app_state.options_cursor, OPTION_ROWS.len() - 1);

    // A text row opens for typing, with what it holds; Esc puts it back.
    app_state.options_cursor = OPTION_ROWS.iter().position(|&row| row == OptionRow::Terminal).unwrap();
    let before = app_state.options.text(OptionRow::Terminal).to_string();
    press(&mut app_state, KeyCode::Enter);
    let Some(Dialog::Options { editing: Some(input) }) = &app_state.dialog else { panic!("not editing") };
    assert_eq!(input.text, before);
    press(&mut app_state, KeyCode::Char('x'));
    // Up and Down belong to the text while it is being typed into.
    let row = app_state.options_cursor;
    press(&mut app_state, KeyCode::Down);
    assert_eq!(app_state.options_cursor, row);
    press(&mut app_state, KeyCode::Esc);
    assert!(matches!(app_state.dialog, Some(Dialog::Options { editing: None })));
    assert_eq!(app_state.options.text(OptionRow::Terminal), before);

    press(&mut app_state, KeyCode::F(11));
    assert!(app_state.dialog.is_none());
    press(&mut app_state, KeyCode::F(11));
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none());
}

/// Ctrl+D's list: A keeps the panel's directory once, Shift+arrows move
/// one, D forgets it, and Enter or a number goes there.
#[test]
fn bookmarks_are_kept_moved_forgotten_and_gone_to() {
    let dir = std::env::temp_dir().join(format!("fm84-bookmarks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("two")).unwrap();
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.join("one"), None);
    // Opened on an empty list rather than the bookmarks of whoever runs this.
    app_state.dialog = Some(Dialog::Bookmarks { list: Vec::new(), index: 0 });
    let list = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Bookmarks { list, index }) => (list.clone(), *index),
        _ => panic!("not open"),
    };

    press(&mut app_state, KeyCode::Char('a'));
    press(&mut app_state, KeyCode::Char('a'));
    assert_eq!(list(&app_state), (vec![dir.join("one")], 0));
    app_state.dialog = None;
    app_state.open_dir(true, dir.join("two"), None);
    app_state.dialog = Some(Dialog::Bookmarks { list: vec![dir.join("one")], index: 0 });
    press(&mut app_state, KeyCode::Insert);
    assert_eq!(list(&app_state), (vec![dir.join("one"), dir.join("two")], 1));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
    assert_eq!(list(&app_state), (vec![dir.join("two"), dir.join("one")], 0));

    // 2 goes to the second, and the dialog closes.
    press(&mut app_state, KeyCode::Char('2'));
    assert!(app_state.dialog.is_none());
    assert_eq!(app_state.dir_left, dir.join("one"));

    // Gone since: said so, and the panel stays put.
    app_state.dialog = Some(Dialog::Bookmarks {
        list: vec![dir.join("gone"), dir.join("two")],
        index: 0,
    });
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.error.is_some());
    assert_eq!(app_state.dir_left, dir.join("one"));
    press(&mut app_state, KeyCode::Esc);

    app_state.dialog = Some(Dialog::Bookmarks {
        list: vec![dir.join("gone"), dir.join("one")],
        index: 1,
    });
    // Drawn numbered, the panel's own directory marked.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("1   ") && screen.contains("2 • "), "{screen}");
    press(&mut app_state, KeyCode::Delete);
    assert_eq!(list(&app_state), (vec![dir.join("gone")], 0));
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(app_state.dialog.is_none());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+F7 asks what to look for, offering the last search; Enter looks,
/// and Enter on a result takes the panel to it, cursor and all.
#[test]
fn find_files_finds_and_goes_there() {
    let dir = std::env::temp_dir().join(format!("fm84-find-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a").join("deep")).unwrap();
    std::fs::write(dir.join("a").join("deep").join("notes.md"), "the needle is here").unwrap();
    std::fs::write(dir.join("a").join("other.md"), "nothing").unwrap();
    std::fs::write(dir.join("top.txt"), "needle").unwrap();

    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), None);
    let alt_f7 = KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT);
    handle_key(&mut app_state, alt_f7);
    assert_eq!(app_state.dialog_input().unwrap().text, "*", "everything, the first time");

    // *.md, then Tab to the text.
    app_state.dialog_input().unwrap().set("*.md".to_string());
    press(&mut app_state, KeyCode::Tab);
    for c in "needle".chars() {
        press(&mut app_state, KeyCode::Char(c));
    }
    press(&mut app_state, KeyCode::Enter);
    let started = std::time::Instant::now();
    let found = loop {
        app_state.poll_find_files();
        if let Some(Dialog::FindFiles(find)) = &app_state.dialog
            && let FindFiles::Found(search) = find.as_ref()
            && search.done
        {
            break search.results.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>();
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the search never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(found, [dir.join("a").join("deep").join("notes.md")]);

    // Enter goes there: its directory, with the cursor on it.
    press(&mut app_state, KeyCode::Enter);
    assert!(app_state.dialog.is_none());
    assert_eq!(app_state.dir_left, dir.join("a").join("deep"));
    let on = app_state.state_left.selected().and_then(|index| app_state.children_left.get(index)).map(|item| item.name_full.clone());
    assert_eq!(on.as_deref(), Some("notes.md"));

    // The next time, the last search is offered again.
    handle_key(&mut app_state, KeyEvent::new(KeyCode::F(7), KeyModifiers::CONTROL));
    assert_eq!(app_state.dialog_input().unwrap().text, "*.md");
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// F10 leaves at once, unless the editor holds edits nobody has saved:
/// then the save prompt comes up first, over anything else in the way.
#[test]
fn quitting_asks_about_unsaved_edits_first() {
    let path = std::env::temp_dir().join(format!("fm84-quit-{}.txt", std::process::id()));
    std::fs::write(&path, "text").unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    assert!(request_quit(&mut app_state));

    app_state.open_editor(path.clone()).unwrap();
    assert!(request_quit(&mut app_state), "nothing to lose yet");

    app_state.editor_insert_char('x');
    app_state.open_prompt(crate::app::PromptKind::Find);
    app_state.display_error("in the way".to_string());
    assert!(!request_quit(&mut app_state));
    // The save prompt takes the find prompt's place, and the error goes.
    assert!(matches!(app_state.dialog, Some(Dialog::SaveChanges { then_quit: true })));
    assert!(app_state.error.is_none());
    std::fs::remove_file(path).unwrap();
}
