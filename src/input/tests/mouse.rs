//! Clicks on the key bar and the path bar.

use super::*;

/// The key bar names what F8 does, and a click on a label is its key -
/// F10 quitting as the key does.
#[test]
fn a_click_on_the_key_bar_is_the_key() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let label = |terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, cell: ratatui::layout::Rect| -> String { (cell.x..cell.right()).map(|x| terminal.backend().buffer()[(x, cell.y)].symbol().to_string()).collect() };
    let hit = |app_state: &AppState, key: u8| app_state.fkey_hits.iter().find(|(_, k)| *k == key).map(|(cell, _)| *cell);
    let click = |app_state: &mut AppState, cell: ratatui::layout::Rect| {
        crate::input::handle_mouse(
            app_state,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: cell.x + 1,
                row: cell.y,
                modifiers: KeyModifiers::NONE,
            },
        )
    };

    assert_eq!(app_state.fkey_hits.len(), 12);
    assert_eq!(label(&terminal, hit(&app_state, 8).unwrap()), " F8 Trash ");
    app_state.options.delete_to_trash = false;
    crate::ui::render_ui(&mut terminal, &mut app_state);
    assert_eq!(label(&terminal, hit(&app_state, 8).unwrap()), " F8 Delete ");

    let (f7, f10) = (hit(&app_state, 7).unwrap(), hit(&app_state, 10).unwrap());
    assert!(click(&mut app_state, f7));
    assert!(matches!(app_state.dialog, Some(Dialog::Create { is_dir: true, .. })));
    // With a dialog up the bar is not a key; Esc puts it away.
    assert!(click(&mut app_state, f10));
    press(&mut app_state, KeyCode::Esc);
    assert!(!click(&mut app_state, f10));

    // The viewer offers F3 over text, where it finds the next match, and
    // not over a picture, where there is nothing to find.
    let dir = std::env::temp_dir().join(format!("fm84-key-bar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "text").unwrap();
    app_state.open_dir(true, dir.clone(), Some("a.txt".as_ref()));
    app_state.is_left_active = true;
    press(&mut app_state, KeyCode::F(3));
    assert!(app_state.is_viewing());
    let keys = |terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>, app_state: &mut AppState| {
        crate::ui::render_ui(terminal, app_state);
        app_state.fkey_hits.iter().map(|(_, key)| *key).collect::<Vec<_>>()
    };
    assert_eq!(keys(&mut terminal, &mut app_state), [3, 10]);
    app_state.viewer_mut().unwrap().mode = crate::viewer::ViewMode::Image;
    assert_eq!(keys(&mut terminal, &mut app_state), [10]);
    app_state.viewer_mut().unwrap().mode = crate::viewer::ViewMode::Text;
    press(&mut app_state, KeyCode::Esc);
    std::fs::remove_dir_all(&dir).unwrap();

    // Narrow, labels are dropped whole from the end.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let shown = app_state.fkey_hits.len();
    assert!((1..12).contains(&shown));
    let (last, _) = app_state.fkey_hits[shown - 1];
    assert!(label(&terminal, last).ends_with(' ') && last.right() < 60);
}

/// A click on a panel's path edits it as Ctrl+L does, in that panel.
#[test]
fn a_click_on_a_path_edits_it() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let bar = app_state.path_bar_area;
    let click_at = |app_state: &mut AppState, column, row| {
        crate::input::handle_mouse(
            app_state,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    };
    let click = |app_state: &mut AppState, column| click_at(app_state, column, bar.y);
    let going_to = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Prompt(PromptKind::GoTo, input)) => Some(input.text.clone()),
        _ => None,
    };

    click(&mut app_state, bar.x + bar.width - 10);
    assert!(!app_state.is_left_active);
    assert!(going_to(&app_state).unwrap().starts_with(&*app_state.dir_right.to_string_lossy()));
    // Typed into, a click on the same path leaves it be.
    press(&mut app_state, KeyCode::Char('x'));
    let typed = going_to(&app_state);
    click(&mut app_state, bar.x + bar.width - 10);
    assert_eq!(going_to(&app_state), typed);
    // The other one starts again, there.
    click(&mut app_state, bar.x + 5);
    assert!(app_state.is_left_active);
    assert!(going_to(&app_state).unwrap().starts_with(&*app_state.dir_left.to_string_lossy()));

    // A click anywhere else puts it away and is a click there too: on a
    // row of the other panel, that panel becomes the active one.
    let right = app_state.table_area_right;
    click_at(&mut app_state, right.x + 3, right.y + 1);
    assert!(going_to(&app_state).is_none());
    assert!(!app_state.is_left_active);
    // As does one on the top bar, which has nothing of its own there.
    click(&mut app_state, bar.x + bar.width - 10);
    click_at(&mut app_state, bar.x + 1, 0);
    assert!(going_to(&app_state).is_none());
}

/// On a line of wide characters, End scrolls far enough to show the cursor,
/// and a click lands on the character drawn there. Both counted a character
/// as one column, where a CJK one is drawn in two: the cursor went off the
/// right edge, and a click picked a character twice as far along.
#[test]
fn wide_characters_take_their_columns_in_the_editor() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let path = std::env::temp_dir().join(format!("fm84-wide-editor-{}.txt", std::process::id()));
    std::fs::write(&path, "日".repeat(60)).unwrap();
    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.open_editor(path.clone()).unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();

    press(&mut app_state, KeyCode::End);
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let area = app_state.editor_content_area;
    let state = app_state.editor().unwrap();
    assert_eq!(state.cursor_col, 60);
    let cursor = crate::utils::display_column(&state.lines[0], state.cursor_col);
    assert!(
        cursor >= state.horizontal_offset && cursor < state.horizontal_offset + area.width as usize,
        "cursor at column {cursor}, showing from {} across {}",
        state.horizontal_offset,
        area.width
    );

    press(&mut app_state, KeyCode::Home);
    crate::ui::render_ui(&mut terminal, &mut app_state);
    assert_eq!(app_state.editor().unwrap().horizontal_offset, 0);
    let area = app_state.editor_content_area;
    crate::input::handle_mouse(
        &mut app_state,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 4,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    // Columns 0-1 are the first character, 2-3 the second.
    assert_eq!(app_state.editor().unwrap().cursor_col, 2);
    std::fs::remove_file(path).unwrap();
}

/// The scroll bars are the mouse's too, in the viewer and the editor alike:
/// a press on the track pages towards it, and a thumb taken hold of follows
/// the pointer - off the track as well - until the button comes up. Nothing
/// under a bar is clicked through it.
#[test]
fn scroll_bars_page_when_clicked_and_follow_a_drag() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let dir = std::env::temp_dir().join(format!("fm84-bar-mouse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.txt");
    std::fs::write(&path, (1..=200).map(|n| format!("{n} {}\n", "x".repeat(300))).collect::<String>()).unwrap();
    let mouse = |app_state: &mut AppState, kind: MouseEventKind, (column, row): (u16, u16)| {
        crate::input::handle_mouse(
            app_state,
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
        );
    };
    let (down, drag, up) = (MouseEventKind::Down(MouseButton::Left), MouseEventKind::Drag(MouseButton::Left), MouseEventKind::Up(MouseButton::Left));

    for edit in [false, true] {
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        if edit {
            app_state.open_editor(path.clone()).unwrap();
        } else {
            app_state.open_viewer(path.clone()).unwrap();
        }
        let mut draw = |app_state: &mut AppState| crate::ui::render_ui(&mut terminal, app_state);
        let offsets = |app_state: &AppState| match &app_state.screen {
            Screen::Editor(state) => (state.scroll_offset, state.horizontal_offset),
            Screen::Viewer(state) => (state.scroll_offset, state.horizontal_offset),
            _ => unreachable!(),
        };
        let bar = |app_state: &AppState, vertical: bool| *app_state.scroll_bars.iter().find(|bar| bar.vertical == vertical).expect("a bar");
        let thumb_cell = |bar: crate::scroll_bar::ScrollBar| {
            let (start, _) = crate::scroll_bar::thumb(if bar.vertical { bar.track.height } else { bar.track.width } as usize, bar.extent).unwrap();
            if bar.vertical { (bar.track.x, bar.track.y + start as u16) } else { (bar.track.x + start as u16, bar.track.y) }
        };
        draw(&mut app_state);
        let vertical = bar(&app_state, true);
        let view = vertical.extent.view;

        // The track below the thumb: a page down, and the cursor left alone.
        mouse(&mut app_state, down, (vertical.track.x, vertical.track.bottom() - 1));
        mouse(&mut app_state, up, (vertical.track.x, vertical.track.bottom() - 1));
        draw(&mut app_state);
        assert_eq!(offsets(&app_state).0, view, "edit: {edit}");
        if edit {
            assert_eq!(app_state.editor().unwrap().cursor_line, 0, "the click went to the bar, not the text");
        } else {
            assert_eq!(app_state.viewer().unwrap().selection, None, "the click went to the bar, not the text");
        }

        // The thumb, taken hold of and dragged past the bottom, then let go.
        let vertical = bar(&app_state, true);
        let grabbed = thumb_cell(vertical);
        mouse(&mut app_state, down, grabbed);
        mouse(&mut app_state, drag, (grabbed.0 + 5, 200));
        draw(&mut app_state);
        let furthest = vertical.extent.content - view;
        assert_eq!(offsets(&app_state).0, furthest, "edit: {edit}");
        mouse(&mut app_state, drag, (grabbed.0, grabbed.1));
        assert_eq!(offsets(&app_state).0, vertical.extent.offset, "followed back up");
        mouse(&mut app_state, up, (grabbed.0, grabbed.1));
        mouse(&mut app_state, drag, (grabbed.0, 200));
        assert_eq!(offsets(&app_state).0, vertical.extent.offset, "let go, it stays");
        draw(&mut app_state);

        // Along the bottom: dragged to the far end, and kept there.
        let horizontal = bar(&app_state, false);
        let grabbed = thumb_cell(horizontal);
        mouse(&mut app_state, down, grabbed);
        mouse(&mut app_state, drag, (horizontal.track.right() + 10, grabbed.1));
        mouse(&mut app_state, up, (horizontal.track.right() + 10, grabbed.1));
        draw(&mut app_state);
        assert_eq!(offsets(&app_state).1, horizontal.extent.content - horizontal.extent.view, "edit: {edit}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A click on a panel's git branch lists the repository's branches, the one
/// checked out highlighted. A click picks another and a second one on it
/// switches there, and the path bar follows. A switch git refuses - it
/// would lose edits not yet committed - says why and leaves the branch as
/// it was.
#[test]
fn a_click_on_the_branch_lists_branches_to_switch_to() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let dir = std::env::temp_dir().join(format!("fm84-branch-click-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    // Nothing to test without git to run.
    if !git(&["init", "--quiet", "--initial-branch=main"]) {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    }
    std::fs::write(dir.join("file.txt"), "main\n").unwrap();
    assert!(git(&["add", "file.txt"]) && git(&["commit", "--quiet", "-m", "first"]));
    assert!(git(&["switch", "--quiet", "-c", "feature"]));
    std::fs::write(dir.join("file.txt"), "feature\n").unwrap();
    assert!(git(&["commit", "--quiet", "-am", "second"]));
    assert!(git(&["switch", "--quiet", "main"]));

    let mut app_state = AppState::new();
    app_state.options = crate::options::Options::default();
    app_state.open_dir(true, dir.clone(), None);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
    let mut draw = |app_state: &mut AppState| crate::ui::render_ui(&mut terminal, app_state);
    let click = |app_state: &mut AppState, cell: ratatui::layout::Rect| {
        crate::input::mouse_to(
            app_state,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: cell.x + 1,
                row: cell.y,
                modifiers: KeyModifiers::NONE,
            },
        );
    };
    let listed = |app_state: &AppState| match &app_state.dialog {
        Some(Dialog::Branches(list)) => Some((list.names.clone(), list.current, list.index)),
        _ => None,
    };
    draw(&mut app_state);
    assert_eq!(app_state.branch_left.as_deref(), Some("main"));

    // The branch on the path bar, clicked: the list, on the one checked out.
    let branch = app_state.branch_hits[0].expect("the branch is drawn");
    click(&mut app_state, branch);
    assert_eq!(listed(&app_state), Some((vec!["feature".to_string(), "main".to_string()], Some(1), 1)));

    // The other row: highlighted, then switched to.
    draw(&mut app_state);
    let feature = app_state.branch_rows.iter().find(|(_, index)| *index == 0).unwrap().0;
    click(&mut app_state, feature);
    assert_eq!(listed(&app_state).map(|(_, _, index)| index), Some(0));
    draw(&mut app_state);
    click(&mut app_state, feature);
    assert!(app_state.dialog.is_none());
    assert_eq!(app_state.error, None);
    assert_eq!(app_state.branch_left.as_deref(), Some("feature"));
    assert_eq!(std::fs::read_to_string(dir.join("file.txt")).unwrap(), "feature\n");

    // An edit that the switch would lose: git refuses, and says so.
    std::fs::write(dir.join("file.txt"), "not committed\n").unwrap();
    draw(&mut app_state);
    let branch = app_state.branch_hits[0].unwrap();
    click(&mut app_state, branch);
    press(&mut app_state, KeyCode::Down);
    assert_eq!(listed(&app_state).map(|(_, _, index)| index), Some(1));
    press(&mut app_state, KeyCode::Enter);
    let refused = app_state.error.clone().expect("git said why");
    assert!(refused.contains("file.txt"), "{refused}");
    assert_eq!(app_state.branch_left.as_deref(), Some("feature"));
    std::fs::remove_dir_all(&dir).unwrap();
}
