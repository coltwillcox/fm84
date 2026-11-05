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

/// An editor on `text`, in a file with the name's extension, the options as
/// they are by default.
fn editing(name: &str, text: &str) -> (AppState, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("fm84-lines-{}-{name}", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.open_editor(path.clone()).unwrap();
    app_state.editor_viewport_height = 20;
    (app_state, path)
}

fn put(app_state: &mut AppState, line: usize, col: usize) {
    let state = app_state.editor_mut().unwrap();
    state.selection_anchor = None;
    state.cursor_line = line;
    state.cursor_col = col;
}

fn lines(app_state: &AppState) -> Vec<String> {
    app_state.editor().unwrap().lines.clone()
}

/// Enter carries the indent on, as far as the cursor, and leaves no line
/// of nothing but spaces behind it. One Ctrl+Z takes it back.
#[test]
fn enter_keeps_the_indent() {
    let (mut app_state, path) = editing("indent.rs", "fn main() {\n    let x = 1;\n\tif x {\n}\n");
    put(&mut app_state, 1, 14);
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(lines(&app_state)[2], "    ");
    assert_eq!(app_state.editor().unwrap().cursor_col, 4);
    // Typing on, and Enter again on the line of only an indent: the one left
    // behind is emptied, the new one indented.
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(&lines(&app_state)[1..4], ["    let x = 1;", "", "    "]);
    // Tabs are carried as tabs; in the middle of an indent, only that far.
    put(&mut app_state, 4, 6);
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(&lines(&app_state)[4..6], ["\tif x ", "\t{"]);
    put(&mut app_state, 1, 2);
    press(&mut app_state, KeyCode::Enter);
    assert_eq!(&lines(&app_state)[1..3], ["", "    let x = 1;"]);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(&lines(&app_state)[1..3], ["    let x = 1;", ""]);
    std::fs::remove_file(path).unwrap();
}

/// Tab on lines selected indents each of them, Shift+Tab brings them back
/// out - or the cursor's line - and the blank ones are left be.
#[test]
fn tab_and_shift_tab_indent_the_lines_selected() {
    let (mut app_state, path) = editing("tabs.py", "def f():\nreturn 1\n\nx = 2\ny = 3\n");
    // Lines 1 to 3, the selection ending at the start of line 4, which is
    // not taken.
    let state = app_state.editor_mut().unwrap();
    state.selection_anchor = Some((1, 3));
    state.cursor_line = 4;
    state.cursor_col = 0;
    press(&mut app_state, KeyCode::Tab);
    assert_eq!(lines(&app_state), ["def f():", "    return 1", "", "    x = 2", "y = 3", ""]);
    // Still selected, whole, for another press.
    assert_eq!(app_state.editor().unwrap().selection(), Some(((1, 0), (3, 9))));
    press(&mut app_state, KeyCode::Tab);
    assert_eq!(lines(&app_state)[1], "        return 1");
    handle_key(&mut app_state, KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(lines(&app_state)[3], "    x = 2");
    // Without a selection: the cursor's line, the cursor moving with it.
    put(&mut app_state, 1, 6);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(lines(&app_state)[1], "return 1");
    assert_eq!(app_state.editor().unwrap().cursor_col, 2);
    // Tab with nothing selected still types a tab.
    put(&mut app_state, 4, 0);
    press(&mut app_state, KeyCode::Tab);
    assert_eq!(lines(&app_state)[4], "\ty = 3");
    std::fs::remove_file(path).unwrap();

    // A file indented with tabs is indented with tabs.
    let (mut app_state, path) = editing("tabs.go", "func f() {\n\treturn\n}\n");
    let state = app_state.editor_mut().unwrap();
    state.selection_anchor = Some((0, 0));
    state.cursor_line = 2;
    state.cursor_col = 1;
    press(&mut app_state, KeyCode::Tab);
    assert_eq!(lines(&app_state), ["\tfunc f() {", "\t\treturn", "\t}", ""]);
    std::fs::remove_file(path).unwrap();
}

/// Ctrl+/ - as most terminals send it, Ctrl+7 - comments the lines with
/// the file's own mark, at the shallowest indent, and takes it out again.
#[test]
fn ctrl_slash_comments_and_uncomments() {
    let (mut app_state, path) = editing("comment.rs", "fn f() {\n    one();\n\n        two();\n}\n");
    let state = app_state.editor_mut().unwrap();
    state.selection_anchor = Some((1, 2));
    state.cursor_line = 3;
    state.cursor_col = 4;
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL));
    assert_eq!(&lines(&app_state)[1..4], ["    // one();", "", "    //     two();"]);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert_eq!(&lines(&app_state)[1..4], ["    one();", "", "        two();"]);
    // Some commented and some not: all are made comments.
    put(&mut app_state, 1, 4);
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert_eq!(lines(&app_state)[1], "    // one();");
    assert_eq!(app_state.editor().unwrap().cursor_col, 7, "the cursor stays on the code");
    std::fs::remove_file(path).unwrap();

    // And the mark is the file's own.
    for (name, mark) in [("x.py", "#"), ("x.lua", "--"), ("x.ini", ";"), ("Makefile", "#")] {
        let (mut app_state, path) = editing(name, "a\n");
        handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL));
        assert_eq!(lines(&app_state)[0], format!("{mark} a"), "{name}");
        std::fs::remove_file(path).unwrap();
    }
}

/// Alt+R in the prompt makes the term a regular expression: found and
/// replaced as one, $1 and the like taken from each match.
#[test]
fn find_and_replace_by_regular_expression() {
    let (mut app_state, path) = editing("regex.txt", "width = 10\nheight = 200\nname = x\n");
    let ctrl = |app_state: &mut AppState, c: char| handle_key(app_state, KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    let typed = |app_state: &mut AppState, text: &str| {
        let input = app_state.dialog_input().unwrap();
        input.set(text.to_string());
        press(app_state, KeyCode::Enter);
    };
    ctrl(&mut app_state, 'f');
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    assert!(app_state.find_regex);
    typed(&mut app_state, r"\d{3}");
    assert_eq!(app_state.editor().unwrap().selection(), Some(((1, 9), (1, 12))));

    // One that is not an expression is said so, and asked for again.
    ctrl(&mut app_state, 'f');
    typed(&mut app_state, "(unclosed");
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Not a regular expression")));
    assert!(matches!(app_state.dialog, Some(Dialog::Prompt(PromptKind::Find, _))));
    app_state.error = None;
    press(&mut app_state, KeyCode::Esc);

    // Replace all, the groups put back the other way round.
    put(&mut app_state, 0, 0);
    ctrl(&mut app_state, 'r');
    typed(&mut app_state, r"^(\w+) = (\d+)$");
    typed(&mut app_state, "$2 is $1");
    press(&mut app_state, KeyCode::Char('a'));
    assert_eq!(lines(&app_state), ["10 is width", "200 is height", "name = x", ""]);
    // As one step to undo.
    ctrl(&mut app_state, 'z');
    assert_eq!(lines(&app_state)[0], "width = 10");
    // Back to text: the same term finds nothing, as a regular expression
    // would have.
    ctrl(&mut app_state, 'f');
    handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    typed(&mut app_state, r"\d{3}");
    assert!(app_state.find_note.as_deref().is_some_and(|note| note.starts_with("Not found")));
    std::fs::remove_file(path).unwrap();
}
