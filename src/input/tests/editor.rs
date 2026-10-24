//! Saving and replacing in the editor, from the keys.

use super::*;

/// A save asks first when something else has written the file since the
/// editor read it: O writes over it, R reads it again, Esc goes back. A
/// save of its own never asks, and a file gone is said to be.
#[test]
fn a_save_asks_before_writing_over_changes_made_elsewhere() {
    let path = std::env::temp_dir().join(format!("fm84-disk-changed-{}.txt", std::process::id()));
    std::fs::write(&path, "one\n").unwrap();
    let mut app_state = AppState::new();
    app_state.open_editor(path.clone()).unwrap();
    let ctrl_s = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let asked = |app_state: &AppState| matches!(app_state.dialog, Some(Dialog::DiskChanged { .. }));

    // Its own saves, one after another, never ask.
    press(&mut app_state, KeyCode::Char('A'));
    ctrl_s(&mut app_state);
    press(&mut app_state, KeyCode::Char('B'));
    ctrl_s(&mut app_state);
    assert!(!asked(&app_state));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ABone\n");

    // Written elsewhere: the save asks, and writes nothing yet.
    std::fs::write(&path, "written elsewhere\n").unwrap();
    press(&mut app_state, KeyCode::Char('C'));
    ctrl_s(&mut app_state);
    assert!(asked(&app_state));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "written elsewhere\n");
    // Esc goes back to the edits, and the next save asks again.
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none() && app_state.editor_is_modified());
    press(&mut app_state, KeyCode::F(2));
    assert!(asked(&app_state));
    // O writes over it, and after that a save is its own again.
    press(&mut app_state, KeyCode::Char('o'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ABCone\n");
    press(&mut app_state, KeyCode::Char('D'));
    ctrl_s(&mut app_state);
    assert!(!asked(&app_state));

    // R reads it again, the edits gone.
    std::fs::write(&path, "theirs\nand more\n").unwrap();
    press(&mut app_state, KeyCode::Char('E'));
    ctrl_s(&mut app_state);
    press(&mut app_state, KeyCode::Char('r'));
    let state = app_state.editor().unwrap();
    assert_eq!(state.lines, ["theirs", "and more", ""]);
    assert!(!state.modified);

    // From the question on the way out: once answered, the editor closes.
    std::fs::write(&path, "again elsewhere\n").unwrap();
    press(&mut app_state, KeyCode::Char('F'));
    press(&mut app_state, KeyCode::Esc);
    press(&mut app_state, KeyCode::Char('y'));
    assert!(asked(&app_state) && app_state.is_editing());
    press(&mut app_state, KeyCode::Char('o'));
    assert!(!app_state.is_editing());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Ftheirs\nand more\n");

    // Gone: said so, with nothing to reload, and O puts it back.
    app_state.open_editor(path.clone()).unwrap();
    std::fs::remove_file(&path).unwrap();
    press(&mut app_state, KeyCode::Char('G'));
    ctrl_s(&mut app_state);
    assert!(matches!(app_state.dialog, Some(Dialog::DiskChanged { now: None, .. })));
    press(&mut app_state, KeyCode::Char('r'));
    assert!(asked(&app_state), "nothing there to reload");
    press(&mut app_state, KeyCode::Char('o'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "GFtheirs\nand more\n");
    std::fs::remove_file(&path).unwrap();
}

/// Ctrl+R and F4 replace in the editor; Y replaces, Enter does nothing,
/// and Esc stops, saying how many were done.
#[test]
fn replace_is_driven_from_the_keys() {
    let path = std::env::temp_dir().join(format!("fm84-replace-keys-{}.txt", std::process::id()));
    std::fs::write(&path, "cat cat cat").unwrap();
    let mut app_state = AppState::new();
    app_state.open_editor(path.clone()).unwrap();
    let answer = |app_state: &mut AppState, text: &str| {
        app_state.dialog_input().unwrap().set(text.to_string());
        press(app_state, KeyCode::Enter);
    };

    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    answer(&mut app_state, "cat");
    answer(&mut app_state, "dog");
    press(&mut app_state, KeyCode::Enter);
    assert!(matches!(app_state.dialog, Some(Dialog::Replacing { replaced: 0, .. })), "Enter is not an answer");
    press(&mut app_state, KeyCode::Char('y'));
    press(&mut app_state, KeyCode::Esc);
    assert!(app_state.dialog.is_none());
    assert_eq!(app_state.editor().unwrap().lines, ["dog cat cat"]);
    assert_eq!(app_state.find_note.as_deref(), Some("Replaced 1"));

    // F4 offers the last term and replacement again.
    press(&mut app_state, KeyCode::F(4));
    assert_eq!(app_state.dialog_input().unwrap().text, "cat");
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(app_state.dialog_input().unwrap().text, "dog");
    press(&mut app_state, KeyCode::Enter);
    press(&mut app_state, KeyCode::Char('a'));
    assert_eq!(app_state.editor().unwrap().lines, ["dog dog dog"]);
    std::fs::remove_file(path).unwrap();
}
