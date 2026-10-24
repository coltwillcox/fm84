use super::AppState;

fn editor_with(name: &str, text: &str) -> (AppState, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("fm84-{name}-{}.txt", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.open_editor(path.clone()).unwrap();
    (app_state, path)
}

/// Type into the prompt as a user would, and press Enter.
fn answer_prompt(app_state: &mut AppState, kind: super::PromptKind, text: &str) {
    app_state.open_prompt(kind);
    let input = app_state.dialog_input().unwrap();
    input.set(String::new());
    text.chars().for_each(|c| input.insert(c));
    app_state.confirm_prompt();
}

fn selected(app_state: &AppState) -> Option<((usize, usize), (usize, usize))> {
    app_state.editor().unwrap().selection()
}

#[test]
fn find_selects_each_match_in_turn_and_wraps() {
    let (mut app_state, path) = editor_with("find", "alpha beta\nBETA\ngamma beta\n");
    app_state.editor_viewport_height = 10;

    // F3 before anything was looked for asks what to look for.
    app_state.find(true, false);
    assert!(matches!(app_state.dialog, Some(super::Dialog::Prompt(super::PromptKind::Find, _))));
    app_state.dialog = None;

    answer_prompt(&mut app_state, super::PromptKind::Find, "beta");
    assert_eq!(selected(&app_state), Some(((0, 6), (0, 10))));
    assert!(app_state.find_shown);
    assert_eq!(app_state.find_note, None);

    app_state.find(true, false);
    assert_eq!(selected(&app_state), Some(((1, 0), (1, 4))), "lower case finds any case");
    app_state.find(true, false);
    assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
    app_state.find(true, false);
    assert_eq!(selected(&app_state), Some(((0, 6), (0, 10))));
    assert_eq!(app_state.find_note.as_deref(), Some("Wrapped to the top"));

    app_state.find(false, false);
    assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
    assert_eq!(app_state.find_note.as_deref(), Some("Wrapped to the bottom"));

    // The prompt offers the last term again, and a miss moves nothing.
    app_state.open_prompt(super::PromptKind::Find);
    assert_eq!(app_state.dialog_input().unwrap().text, "beta");
    app_state.dialog = None;
    answer_prompt(&mut app_state, super::PromptKind::Find, "BETAMAX");
    assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
    assert_eq!(app_state.find_note.as_deref(), Some("Not found: BETAMAX"));

    // Closing the editor takes the marks with it, and keeps the term.
    app_state.close_editor();
    assert!(!app_state.find_shown);
    assert_eq!(app_state.find_term, "BETAMAX");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn go_to_line_puts_the_cursor_on_it_and_keeps_inside_the_file() {
    let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    let (mut app_state, path) = editor_with("goto", &text);
    app_state.editor_viewport_height = 10;

    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "42");
    let state = app_state.editor().unwrap();
    assert_eq!((state.cursor_line, state.cursor_col), (41, 0));
    assert!(state.scroll_offset <= 41 && 41 < state.scroll_offset + 10, "the line is on screen");

    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "100000");
    assert_eq!(app_state.editor().unwrap().cursor_line, 100, "the last line");
    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "0");
    assert_eq!(app_state.editor().unwrap().cursor_line, 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_viewer_finds_and_goes_to_lines_too() {
    let path = std::env::temp_dir().join(format!("fm84-view-find-{}.txt", std::process::id()));
    let text: String = (1..=50).map(|n| if n == 30 { "\tthe needle\n".to_string() } else { format!("hay {n}\n") }).collect();
    std::fs::write(&path, text).unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.open_viewer(path.clone()).unwrap();
    app_state.viewer_viewport_height = 10;
    app_state.viewer_viewport_width = 40;

    answer_prompt(&mut app_state, super::PromptKind::Find, "needle");
    let state = app_state.viewer().unwrap();
    // Columns as drawn, with the tab opened out, so the selection and the
    // copy line up with the screen.
    let start = crate::display::tab_width() + 4;
    assert_eq!(state.selected_range(), Some(((29, start), (29, start + 6))));
    assert_eq!(state.selected_text().as_deref(), Some("needle"));
    assert!(state.scroll_offset <= 29 && 29 < state.scroll_offset + 10);

    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "5");
    assert_eq!(app_state.viewer().unwrap().scroll_offset, 4);
    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "50");
    // As far down as End goes, which leaves the last screenful showing.
    app_state.viewer_end();
    let end = app_state.viewer().unwrap().scroll_offset;
    app_state.viewer_home();
    answer_prompt(&mut app_state, super::PromptKind::GoToLine, "50");
    assert_eq!(app_state.viewer().unwrap().scroll_offset, end, "no further than the end allows");
    std::fs::remove_file(path).unwrap();
}

/// A file that starts out as plain ASCII and has a byte that is not UTF-8
/// further in passes the check on its head. The viewer shows it anyway;
/// the editor, which would write replacement characters back, says why
/// it will not.
#[test]
fn a_stray_byte_past_the_head_still_views_and_is_refused_for_editing() {
    let path = std::env::temp_dir().join(format!("fm84-latin1-late-{}.txt", std::process::id()));
    let mut bytes = vec![b'a'; 600];
    bytes.extend_from_slice(b"\ncaf\xe9\n");
    std::fs::write(&path, bytes).unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();

    app_state.open_viewer(path.clone()).unwrap();
    let state = app_state.viewer().unwrap();
    assert_eq!(state.content_lines[1], "caf\u{fffd}");

    let refused = app_state.open_editor(path.clone()).unwrap_err();
    assert!(refused.contains("not UTF-8"), "{refused}");
    assert!(app_state.editor().is_none());
    std::fs::remove_file(path).unwrap();
}

/// Ask to replace `what` with `with`, as Ctrl+R and the two prompts do.
fn replace(app_state: &mut AppState, what: &str, with: &str) {
    app_state.open_replace();
    app_state.dialog_input().unwrap().set(what.to_string());
    app_state.confirm_prompt();
    app_state.dialog_input().unwrap().set(with.to_string());
    app_state.confirm_prompt();
}

fn text(app_state: &AppState) -> String {
    app_state.editor().unwrap().lines.join("\n")
}

fn asking(app_state: &AppState) -> bool {
    matches!(app_state.dialog, Some(super::Dialog::Replacing { .. }))
}

#[test]
fn replace_asks_about_each_match_and_stops_where_it_began() {
    let (mut app_state, path) = editor_with("replace-each", "a b a\nb a");
    replace(&mut app_state, "a", "xy");
    // The first match is selected and asked about.
    assert_eq!(selected(&app_state), Some(((0, 0), (0, 1))));
    assert!(app_state.replace_step(true));
    assert!(app_state.replace_step(false));
    // The last one; after it, round the end and back to the start, where
    // the one left alone is not asked about again.
    assert!(!app_state.replace_step(true));
    assert!(!asking(&app_state));
    assert_eq!(text(&app_state), "xy b a\nb xy");
    assert_eq!(app_state.find_note.as_deref(), Some("Replaced 2"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replace_all_is_one_step_to_undo_and_never_chases_its_own_text() {
    let (mut app_state, path) = editor_with("replace-all", "a a\na");
    // Started on the last line, it goes round to cover the lines above.
    app_state.editor_state_cursor(1, 0);
    replace(&mut app_state, "a", "aa");
    app_state.replace_all();
    assert_eq!(text(&app_state), "aa aa\naa");
    assert_eq!(app_state.find_note.as_deref(), Some("Replaced 3"));
    app_state.editor_undo();
    assert_eq!(text(&app_state), "a a\na");
    app_state.editor_redo();
    assert_eq!(text(&app_state), "aa aa\naa");
    std::fs::remove_file(path).unwrap();
}

/// Started past the last match, with a replacement that holds what it
/// replaces: once round, every match is done, and it stops - rather than
/// going round again into the text it put in, for ever.
#[test]
fn replace_stops_after_once_round_even_into_its_own_text() {
    let (mut app_state, path) = editor_with("replace-once-round", "let x;\nlet y; z");
    app_state.editor_state_cursor(1, 8);
    replace(&mut app_state, "let", "/* let");
    // Stepped, with a limit, so a regression fails rather than hangs.
    let mut steps = 1;
    while app_state.replace_step(true) {
        steps += 1;
        assert!(steps < 10, "still replacing: {}", text(&app_state));
    }
    assert_eq!(text(&app_state), "/* let x;\n/* let y; z");
    assert_eq!(app_state.find_note.as_deref(), Some("Replaced 2"));
    std::fs::remove_file(path).unwrap();
}

/// Replace all highlights once, at the end, and comes out coloured just
/// as the file would be read afresh - including a change that turns the
/// rest of the file into a comment.
#[test]
fn replace_all_highlights_as_a_fresh_read_would() {
    let text: String = (0..40).map(|n| format!("let foo_{n} = foo(\"foo\"); // foo\n")).collect::<String>() + "fn tail() {}\n";
    let path = std::env::temp_dir().join(format!("fm84-replace-highlight-{}.rs", std::process::id()));
    std::fs::write(&path, &text).unwrap();
    let mut app_state = AppState::new();
    app_state.open_editor(path.clone()).unwrap();
    let shape = |spans: &[Vec<ratatui::text::Span<'static>>]| -> Vec<Vec<(String, Option<ratatui::style::Color>)>> { spans.iter().map(|line| line.iter().map(|span| (span.content.to_string(), span.style.fg)).collect()).collect() };

    for (what, with) in [("foo", "bar"), ("let", "/* let")] {
        replace(&mut app_state, what, with);
        app_state.replace_all();
        let state = app_state.editor().unwrap();
        assert!(!state.line_states.is_empty(), "highlighting back on");
        let (fresh, _) = crate::viewer::highlight_all(&state.lines, "rs");
        assert_eq!(shape(&state.highlighted_lines), shape(&fresh), "after {what} -> {with}");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn replacing_with_nothing_deletes_and_lower_case_matches_any_case() {
    let (mut app_state, path) = editor_with("replace-nothing", "Foo bar foo");
    replace(&mut app_state, "foo", "");
    app_state.replace_all();
    assert_eq!(text(&app_state), " bar ");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn going_round_moves_the_stopping_point_with_the_text_before_it() {
    // From the middle of a line, round to its start: what goes in before
    // the starting point pushes it along, and every match is done once.
    let (mut app_state, path) = editor_with("replace-shift", "ab ab ab");
    app_state.editor_state_cursor(0, 4);
    replace(&mut app_state, "ab", "abab");
    app_state.replace_all();
    assert_eq!(text(&app_state), "abab abab abab");
    assert_eq!(app_state.find_note.as_deref(), Some("Replaced 3"));
    // Nothing there: said so, and nothing is asked.
    replace(&mut app_state, "zzz", "y");
    assert!(!asking(&app_state));
    assert_eq!(app_state.find_note.as_deref(), Some("Not found: zzz"));
    std::fs::remove_file(path).unwrap();
}

/// W wraps long lines in the viewer: scrolling goes by rows, while
/// finding, selecting, copying and going to a line go by lines.
#[test]
fn the_viewer_wraps_long_lines_onto_rows() {
    let path = std::env::temp_dir().join(format!("fm84-wrap-{}.txt", std::process::id()));
    std::fs::write(&path, "short\none two three four five six seven needle eight\nlast").unwrap();
    let mut app_state = AppState::new();
    app_state.open_viewer(path.clone()).unwrap();
    (app_state.viewer_viewport_width, app_state.viewer_viewport_height) = (10, 3);

    app_state.viewer_toggle_wrap();
    let state = app_state.viewer().unwrap();
    assert!(state.wrapping());
    assert_eq!(state.line_count(), 3);
    // The long line takes six rows of ten.
    assert_eq!(state.total_lines, 8);
    assert_eq!(state.row(1), (1, 0, 8));

    // Found on the long line's fifth row, which scrolls into view.
    app_state.find_term = "needle".to_string();
    app_state.find(true, true);
    let state = app_state.viewer().unwrap();
    assert_eq!(state.selected_text().as_deref(), Some("needle"));
    let row = state.row_of(1, 38);
    assert!(state.scroll_offset <= row && row < state.scroll_offset + 3, "row {row} off screen from {}", state.scroll_offset);

    // Copied across a break that is only on screen: no newline in it.
    let state = app_state.viewer_mut().unwrap();
    state.selection = Some(((1, 4), (1, 13)));
    assert_eq!(state.selected_text().as_deref(), Some("two three"));
    // A click on the second row of the long line lands in that line.
    state.scroll_offset = 0;
    assert_eq!(state.position_at(2, 3), (1, 11));

    // Go to line 3: the row it starts on, as far down as the end allows.
    app_state.go_to_line(3);
    assert_eq!(app_state.viewer().unwrap().scroll_offset, 5);

    // X to a hexdump is a row a line again; back to text, wrapped again.
    app_state.viewer_next_mode();
    assert!(app_state.viewer().unwrap().rows.is_empty());
    app_state.viewer_next_mode();
    assert!(app_state.fit_viewer_wrap());
    assert_eq!(app_state.viewer().unwrap().total_lines, 8);

    // W again: a row a line, with the line that was at the top still there.
    app_state.viewer_mut().unwrap().scroll_offset = 3;
    app_state.viewer_toggle_wrap();
    let state = app_state.viewer().unwrap();
    assert!(!state.wrapping() && state.rows.is_empty());
    assert_eq!((state.scroll_offset, state.total_lines), (1, 3));

    // The next file opened takes up W as it was left.
    app_state.viewer_toggle_wrap();
    app_state.close_viewer();
    app_state.open_viewer(path.clone()).unwrap();
    assert!(app_state.viewer().unwrap().wrap);
    std::fs::remove_file(path).unwrap();
}

fn stacks(app_state: &AppState) -> (usize, usize) {
    let state = app_state.editor().unwrap();
    (state.undo_stack.len(), state.redo_stack.len())
}

#[test]
fn saving_updates_the_size_the_panels_show() {
    let dir = std::env::temp_dir().join(format!("fm84-save-size-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "ab").unwrap();

    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.dir_left = dir.clone();
    app_state.dir_right = dir.clone();
    app_state.reload_panel(true, None);
    app_state.reload_panel(false, None);
    let size = |children: &[super::Item]| children.iter().find(|item| item.name_full == "notes.txt").unwrap().size_bytes;
    assert_eq!(size(&app_state.children_left), 2);

    app_state.open_editor(path.clone()).unwrap();
    app_state.editor_insert_text("more text");
    app_state.editor_save().unwrap();
    assert_eq!(size(&app_state.children_left), 11);
    assert_eq!(size(&app_state.children_right), 11);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_edit_that_changes_nothing_keeps_the_redo_history() {
    let (mut app_state, path) = editor_with("noop", "ab\ncd");
    // One real edit, undone, so there is something to redo.
    app_state.editor_end();
    app_state.editor_insert_char('x');
    app_state.editor_undo();
    assert_eq!(stacks(&app_state), (0, 1));

    // Backspace at the very start, Delete at the very end, and Ctrl+X with
    // nothing selected: none of them change the text.
    app_state.editor_mut().unwrap().cursor_col = 0;
    app_state.editor_backspace();
    app_state.editor_cut();
    {
        let state = app_state.editor_mut().unwrap();
        state.cursor_line = 1;
        state.cursor_col = 2;
    }
    app_state.editor_delete();
    assert_eq!(stacks(&app_state), (0, 1));
    app_state.editor_redo();
    assert_eq!(app_state.editor().unwrap().lines, ["abx", "cd"]);

    // A real edit still forks the history.
    app_state.editor_undo();
    app_state.editor_insert_char('y');
    assert_eq!(stacks(&app_state), (1, 0));
    std::fs::remove_file(path).unwrap();
}

/// An edit that rewrites several lines without changing how many there are,
/// such as a paste over a selection as long as itself, or undoing Replace
/// all, shows every one of them as it now is. The parse could stop at the line
/// after the first, where the state matched the cache, and the editor went on
/// drawing the old text of the rest.
#[test]
fn every_line_an_edit_rewrites_is_highlighted_again() {
    let text = "let a = 1;\nlet b = 2;\nlet c = 3;\nfn tail() {}\n";
    let path = std::env::temp_dir().join(format!("fm84-rehighlight-{}.rs", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let mut app_state = AppState::new();
    app_state.open_editor(path.clone()).unwrap();
    let as_drawn = |app_state: &AppState| -> Vec<String> {
        let state = app_state.editor().unwrap();
        assert!(!state.highlighted_lines.is_empty(), "highlighting is on");
        state.highlighted_lines.iter().map(|line| line.iter().map(|span| span.content.as_ref()).collect()).collect()
    };
    let lines = |app_state: &AppState| app_state.editor().unwrap().lines.clone();

    if let Some(state) = app_state.editor_mut() {
        state.selection_anchor = Some((0, 0));
        state.cursor_line = 2;
        state.cursor_col = 10;
    }
    app_state.editor_insert_text("let x = 7;\nlet y = 8;\nlet z = 9;");
    assert_eq!(as_drawn(&app_state), lines(&app_state), "after the paste");
    app_state.editor_undo();
    assert_eq!(as_drawn(&app_state), lines(&app_state), "after undoing it");
    app_state.editor_redo();
    assert_eq!(as_drawn(&app_state), lines(&app_state), "after redoing it");

    replace(&mut app_state, "let", "const");
    app_state.replace_all();
    app_state.editor_undo();
    assert_eq!(lines(&app_state)[..3], ["let x = 7;", "let y = 8;", "let z = 9;"]);
    assert_eq!(as_drawn(&app_state), lines(&app_state), "after undoing Replace all");
    std::fs::remove_file(path).unwrap();
}
