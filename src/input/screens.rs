//! Keys for what takes the panels' place: the viewer, the diff and the editor.

use super::*;

pub(super) fn viewer_key(app_state: &mut AppState, key: KeyEvent) {
    match key.code {
        // Esc is the one way out, of the viewer and of the notice F4 raises
        // on a binary alike.
        KeyCode::Esc => app_state.close_viewer(),
        // F3 and Shift+F3 step through the matches, as in most editors - so
        // F3 here cannot also close the viewer.
        KeyCode::F(3) => app_state.find(!key.modifiers.contains(KeyModifiers::SHIFT), false),
        KeyCode::Char('x') | KeyCode::Char('X') => app_state.viewer_next_mode(),
        KeyCode::Char('f') | KeyCode::Char('F') => app_state.viewer_toggle_fill(),
        // Wrap long lines, or scroll them sideways.
        KeyCode::Char('w') | KeyCode::Char('W') => app_state.viewer_toggle_wrap(),
        // Zoom a picture. The unshifted keys count too, so it is one key
        // either way on a numeric keypad or a main row.
        KeyCode::Char('+') | KeyCode::Char('=') => app_state.viewer_zoom(true),
        KeyCode::Char('-') | KeyCode::Char('_') => app_state.viewer_zoom(false),
        KeyCode::Down => app_state.viewer_scroll_down(),
        KeyCode::Up => app_state.viewer_scroll_up(),
        KeyCode::Left => app_state.viewer_scroll_left(),
        KeyCode::Right => app_state.viewer_scroll_right(),
        // On a picture these step through the folder; there is nothing to
        // page down when the whole of it is on screen, and the arrows and the
        // mouse still move it when not.
        KeyCode::PageDown if app_state.viewer_shows_image() => app_state.viewer_step_image(true),
        KeyCode::PageUp if app_state.viewer_shows_image() => app_state.viewer_step_image(false),
        KeyCode::PageDown => app_state.viewer_page_down(),
        KeyCode::PageUp => app_state.viewer_page_up(),
        KeyCode::Home => app_state.viewer_home(),
        KeyCode::End => app_state.viewer_end(),
        _ => {}
    }
}

/// Two files side by side. F3 and Shift+F3 go to the next and the previous
/// difference, as they go to the next and previous match in the viewer; N and
/// P do too.
pub(super) fn diff_key(app_state: &mut AppState, key: KeyEvent) {
    let page = app_state.diff_viewport_height.max(1) as isize;
    match key.code {
        KeyCode::Esc => app_state.close_diff(),
        KeyCode::F(3) => {
            let forward = !key.modifiers.contains(KeyModifiers::SHIFT);
            if let Some(state) = app_state.diff_mut() {
                state.step(forward);
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            if let Some(state) = app_state.diff_mut() {
                state.step(true);
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if let Some(state) = app_state.diff_mut() {
                state.step(false);
            }
        }
        KeyCode::Down => app_state.diff_scroll(1, 0),
        KeyCode::Up => app_state.diff_scroll(-1, 0),
        KeyCode::PageDown => app_state.diff_scroll(page, 0),
        KeyCode::PageUp => app_state.diff_scroll(-page, 0),
        KeyCode::Home => app_state.diff_scroll(isize::MIN, isize::MIN),
        KeyCode::End => app_state.diff_scroll(isize::MAX, 0),
        KeyCode::Left => app_state.diff_scroll(0, -4),
        KeyCode::Right => app_state.diff_scroll(0, 4),
        _ => {}
    }
}

pub(super) fn editor_key(app_state: &mut AppState, key: KeyEvent) {
    if let Some(state) = app_state.editor_mut() {
        state.auto_scroll = true;
    }
    // Shift turns a cursor move into a selection; without it the selection
    // is dropped.
    let extend = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Esc => {
            if app_state.editor_is_modified() {
                app_state.dialog = Some(Dialog::SaveChanges { then_quit: false });
            } else {
                app_state.close_editor();
            }
        }
        KeyCode::F(2) => {
            save_then(app_state, AfterSave::Stay);
        }
        KeyCode::F(3) => app_state.find(!extend, false),
        // F4 replaces, as in MC's editor; Ctrl+R does too.
        KeyCode::F(4) => app_state.open_replace(),
        KeyCode::Up => {
            app_state.editor_prepare_move(extend);
            app_state.editor_cursor_up();
        }
        KeyCode::Down => {
            app_state.editor_prepare_move(extend);
            app_state.editor_cursor_down();
        }
        KeyCode::Left => {
            app_state.editor_prepare_move(extend);
            app_state.editor_cursor_left();
        }
        KeyCode::Right => {
            app_state.editor_prepare_move(extend);
            app_state.editor_cursor_right();
        }
        KeyCode::Home => {
            app_state.editor_prepare_move(extend);
            app_state.editor_home();
        }
        KeyCode::End => {
            app_state.editor_prepare_move(extend);
            app_state.editor_end();
        }
        KeyCode::PageUp => {
            app_state.editor_prepare_move(extend);
            app_state.editor_page_up();
        }
        KeyCode::PageDown => {
            app_state.editor_prepare_move(extend);
            app_state.editor_page_down();
        }
        KeyCode::Enter => app_state.editor_enter(),
        KeyCode::Backspace => app_state.editor_backspace(),
        // CUA aliases, which bypass the Ctrl-chord gate entirely.
        KeyCode::Insert if key.modifiers.contains(KeyModifiers::CONTROL) => app_state.editor_copy(),
        KeyCode::Insert if extend => app_state.editor_paste(),
        KeyCode::Delete if extend => app_state.editor_cut(),
        KeyCode::Delete => app_state.editor_delete(),
        KeyCode::Tab => app_state.editor_insert_char('\t'),
        KeyCode::Char(c) => app_state.editor_insert_char(c),
        _ => {}
    }
}
