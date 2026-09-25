use crate::app::AppState;
use crate::fs_ops::{copy_path, create_directory, create_file, delete_path, move_path, path_exists, rename_path};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crate::constants::TAB_SPACES;
use ratatui::layout::Position;
use ratatui::widgets::TableState;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn handle_input(app_state: &mut AppState) -> Result<bool> {
    if event::poll(Duration::from_millis(100))? {
        match event::read()? {
            // Windows reports a press and a release for every key. Acting on the
            // release too would run each action twice, and toggles would cancel
            // themselves out. Repeat is kept so held keys still work.
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                // A character carrying Ctrl or Alt is a chord, not text. Only
                // Ctrl+S is bound, so drop the rest instead of letting them fall
                // through as typed characters - they would otherwise land in the
                // file being edited, in a filename, or on a yes/no prompt.
                // AltGr reports as Ctrl+Alt on Windows and does produce text
                // (@, EUR, ...), so a chord is a lone Ctrl or a lone Alt.
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                if let KeyCode::Char(c) = key.code
                    && control != alt
                {
                    let in_editor = app_state.is_f4_displayed && !app_state.is_editor_save_prompt;
                    if control {
                        match c {
                            's' if in_editor => {
                                if let Err(e) = app_state.editor_save() {
                                    app_state.display_error(e);
                                }
                            }
                            'a' if in_editor => app_state.editor_select_all(),
                            // Ctrl+Z undoes; Ctrl+Shift+Z and Ctrl+Y put it back.
                            'z' if in_editor && !key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app_state.editor_undo()
                            }
                            'z' | 'Z' if in_editor => app_state.editor_redo(),
                            'y' | 'Y' if in_editor => app_state.editor_redo(),
                            'c' if in_editor => app_state.editor_copy(),
                            'c' if app_state.is_f3_displayed => app_state.viewer_copy(),
                            'x' if in_editor => app_state.editor_cut(),
                            'v' if in_editor => app_state.editor_paste(),
                            // Ctrl+R rereads both panels from disk.
                            'r' if !app_state.is_modal_open() => {
                                app_state.reload_panel(true, None);
                                app_state.reload_panel(false, None);
                            }
                            _ => {}
                        }
                    }
                    return Ok(true);
                }

                if app_state.is_f2_displayed {
                    match key.code {
                        KeyCode::Esc => handle_esc(app_state),
                        KeyCode::F(2) => toggle_rename(app_state),
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Enter => handle_rename(app_state),
                        KeyCode::Char(to_insert) => app_state.rename_input.insert(to_insert),
                        KeyCode::Backspace => app_state.rename_input.backspace(),
                        KeyCode::Delete => app_state.rename_input.delete_forward(),
                        KeyCode::Left => app_state.rename_input.move_left(),
                        KeyCode::Right => app_state.rename_input.move_right(),
                        _ => {}
                    }
                } else if app_state.is_f1_displayed {
                    match key.code {
                        KeyCode::Esc => handle_esc(app_state),
                        KeyCode::F(1) => toggle_help(app_state),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.drive_picker.is_some() {
                    match key.code {
                        KeyCode::Esc => app_state.drive_picker = None,
                        KeyCode::Enter => app_state.confirm_drive_picker(),
                        KeyCode::F(1) | KeyCode::F(2) | KeyCode::Right | KeyCode::Down => {
                            app_state.move_drive_picker(true)
                        }
                        KeyCode::Left | KeyCode::Up => app_state.move_drive_picker(false),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.is_f11_displayed {
                    match key.code {
                        KeyCode::Esc | KeyCode::F(11) => app_state.is_f11_displayed = false,
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.is_f8_displayed {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => handle_esc(app_state),
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => handle_delete_confirm(app_state),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.is_f7_displayed {
                    match key.code {
                        KeyCode::Esc => handle_esc(app_state),
                        KeyCode::F(7) => toggle_create(app_state, true),
                        KeyCode::F(4) if key.modifiers.contains(KeyModifiers::SHIFT) => toggle_create(app_state, false),
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Enter => handle_create_confirm(app_state),
                        KeyCode::Char(to_insert) => app_state.create_input.insert(to_insert),
                        KeyCode::Backspace => app_state.create_input.backspace(),
                        KeyCode::Delete => app_state.create_input.delete_forward(),
                        KeyCode::Left => app_state.create_input.move_left(),
                        KeyCode::Right => app_state.create_input.move_right(),
                        _ => {}
                    }
                } else if app_state.is_f3_displayed {
                    match key.code {
                        KeyCode::Esc => handle_esc(app_state),
                        // Each key closes only what it opened: F3 the viewer,
                        // F4 the notice it raises on a binary. Esc closes either.
                        KeyCode::F(3) => {
                            if app_state.viewer_state.as_ref().is_some_and(|state| !state.from_edit) {
                                app_state.close_viewer();
                            }
                        }
                        KeyCode::F(4) => {
                            if app_state.viewer_state.as_ref().is_some_and(|state| state.from_edit) {
                                app_state.close_viewer();
                            }
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') => app_state.viewer_next_mode(),
                        KeyCode::Char('f') | KeyCode::Char('F') => app_state.viewer_toggle_fill(),
                        // Zoom a picture. The unshifted keys count too, so it is
                        // one key either way on a numeric keypad or a main row.
                        KeyCode::Char('+') | KeyCode::Char('=') => app_state.viewer_zoom(true),
                        KeyCode::Char('-') | KeyCode::Char('_') => app_state.viewer_zoom(false),
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Down => app_state.viewer_scroll_down(),
                        KeyCode::Up => app_state.viewer_scroll_up(),
                        KeyCode::Left => app_state.viewer_scroll_left(),
                        KeyCode::Right => app_state.viewer_scroll_right(),
                        KeyCode::PageDown => app_state.viewer_page_down(),
                        KeyCode::PageUp => app_state.viewer_page_up(),
                        KeyCode::Home => app_state.viewer_home(),
                        KeyCode::End => app_state.viewer_end(),
                        _ => {}
                    }
                } else if app_state.large_file.is_some() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app_state.reset_large_file(),
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app_state.confirm_large_file(),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.is_editor_save_prompt {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            // Save and close
                            if let Err(e) = app_state.editor_save() {
                                app_state.display_error(e);
                            }
                            app_state.is_editor_save_prompt = false;
                            app_state.close_editor();
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            // Discard and close
                            app_state.is_editor_save_prompt = false;
                            app_state.close_editor();
                        }
                        KeyCode::Esc => {
                            // Cancel, return to editor
                            app_state.is_editor_save_prompt = false;
                        }
                        _ => {}
                    }
                } else if app_state.is_f4_displayed {
                    if let Some(state) = &mut app_state.editor_state {
                        state.auto_scroll = true;
                    }
                    // Shift turns a cursor move into a selection; without it the
                    // selection is dropped.
                    let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                    match key.code {
                        KeyCode::Esc | KeyCode::F(4) => {
                            if app_state.editor_is_modified() {
                                app_state.is_editor_save_prompt = true;
                            } else {
                                app_state.close_editor();
                            }
                        }
                        KeyCode::F(2) => {
                            // Save file
                            if let Err(e) = app_state.editor_save() {
                                app_state.display_error(e);
                            }
                        }
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Up => { app_state.editor_prepare_move(extend); app_state.editor_cursor_up(); }
                        KeyCode::Down => { app_state.editor_prepare_move(extend); app_state.editor_cursor_down(); }
                        KeyCode::Left => { app_state.editor_prepare_move(extend); app_state.editor_cursor_left(); }
                        KeyCode::Right => { app_state.editor_prepare_move(extend); app_state.editor_cursor_right(); }
                        KeyCode::Home => { app_state.editor_prepare_move(extend); app_state.editor_home(); }
                        KeyCode::End => { app_state.editor_prepare_move(extend); app_state.editor_end(); }
                        KeyCode::PageUp => { app_state.editor_prepare_move(extend); app_state.editor_page_up(); }
                        KeyCode::PageDown => { app_state.editor_prepare_move(extend); app_state.editor_page_down(); }
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
                } else if app_state.is_f5_displayed {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => handle_esc(app_state),
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => handle_copy_confirm(app_state),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else if app_state.is_f6_displayed {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => handle_esc(app_state),
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => handle_move_confirm(app_state),
                        KeyCode::F(10) => return Ok(false),
                        _ => {}
                    }
                } else {
                    let drive_chord = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
                    match key.code {
                        KeyCode::Esc => {
                            app_state.search_clear();
                            handle_esc(app_state);
                        }
                        // Alt+F1/F2 choose a drive per panel. Ctrl is an alias,
                        // because window managers commonly eat Alt+F1 and Alt+F2.
                        KeyCode::F(1) if drive_chord => app_state.open_drive_picker(true),
                        KeyCode::F(2) if drive_chord => app_state.open_drive_picker(false),
                        KeyCode::F(1) => toggle_help(app_state),
                        KeyCode::F(2) => toggle_rename(app_state),
                        KeyCode::F(3) => handle_f3_view(app_state),
                        // Shift+F4 creates an empty file, as MC does.
                        KeyCode::F(4) if key.modifiers.contains(KeyModifiers::SHIFT) => toggle_create(app_state, false),
                        KeyCode::F(4) => handle_f4_edit(app_state),
                        KeyCode::F(5) => toggle_copy(app_state),
                        KeyCode::F(6) => toggle_move(app_state),
                        KeyCode::F(7) => toggle_create(app_state, true),
                        KeyCode::F(8) | KeyCode::Delete => toggle_delete(app_state),
                        KeyCode::F(9) => open_terminal(app_state),
                        KeyCode::F(11) => toggle_options(app_state),
                        KeyCode::F(12) => toggle_preview(app_state),
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Char(' ') => {
                            // Space toggles selection and moves to next item
                            app_state.toggle_selection();
                        }
                        KeyCode::Insert => {
                            // Insert toggles selection without calculating directory size
                            app_state.toggle_selection_no_size();
                        }
                        KeyCode::Char(c) if c.is_alphanumeric() || ".-_".contains(c) => {
                            app_state.search_add_char(c);
                        }
                        KeyCode::Backspace => {
                            if app_state.search_input.is_empty() {
                                handle_navigate_up(app_state);
                            } else {
                                app_state.search_backspace();
                            }
                        }
                        KeyCode::Tab => handle_tab_switching(app_state),
                        KeyCode::Down => {
                            if !app_state.search_input.is_empty() {
                                app_state.jump_to_next_match();
                            } else {
                                handle_move_selection(app_state, |state, len| {
                                    state.select(state.selected().map_or(Some(0), |i| Some((i + 1).min(len.saturating_sub(1)))));
                                });
                            }
                        }
                        KeyCode::Up => {
                            if !app_state.search_input.is_empty() {
                                app_state.jump_to_prev_match();
                            } else {
                                handle_move_selection(app_state, |state, _len| {
                                    state.select(state.selected().map_or(Some(0), |i| Some(i.saturating_sub(1))));
                                });
                            }
                        }
                        KeyCode::PageDown => {
                            let page_size = app_state.page_size as usize;
                            handle_move_selection(app_state, |state, len| {
                                state.select(state.selected().map(|selected| (selected + page_size).min(len.saturating_sub(1))));
                            })
                        }
                        KeyCode::PageUp => {
                            let page_size = app_state.page_size as usize;
                            handle_move_selection(app_state, |state, _len| {
                                state.select(state.selected().map(|selected| selected.saturating_sub(page_size)));
                            })
                        }
                        KeyCode::Home => handle_move_selection(app_state, |state, _len| {
                            state.select(Some(0));
                        }),
                        KeyCode::End => handle_move_selection(app_state, |state, len| {
                            state.select(Some(len.saturating_sub(1)));
                        }),
                        KeyCode::Enter => handle_enter_directory(app_state),
                        _ => {}
                    }
                }
            }
            // Bracketed paste: the terminal hands over the system clipboard as
            // one event instead of a burst of keystrokes.
            Event::Paste(text) => {
                if app_state.is_f4_displayed && !app_state.is_editor_save_prompt {
                    app_state.editor_insert_text(&text);
                } else if app_state.is_f2_displayed {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        app_state.rename_input.insert(character);
                    }
                } else if app_state.is_f7_displayed {
                    for character in text.chars().filter(|character| !character.is_control()) {
                        app_state.create_input.insert(character);
                    }
                }
            }
            Event::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::Down(_btn) => {
                    if app_state.is_f4_displayed {
                        handle_editor_click(app_state, mouse_event.column, mouse_event.row, false);
                    } else if app_state.is_f3_displayed {
                        handle_viewer_click(app_state, mouse_event.column, mouse_event.row, false);
                    } else {
                        handle_mouse_click(app_state, mouse_event.column, mouse_event.row);
                    }
                }
                // Dragging extends whatever the press started.
                MouseEventKind::Drag(_btn) => {
                    if app_state.is_f4_displayed {
                        handle_editor_click(app_state, mouse_event.column, mouse_event.row, true);
                    } else if app_state.is_f3_displayed {
                        handle_viewer_click(app_state, mouse_event.column, mouse_event.row, true);
                    }
                }
                MouseEventKind::ScrollDown => {
                    if app_state.is_f3_displayed {
                        app_state.viewer_scroll_down();
                    } else if app_state.is_f4_displayed {
                        app_state.editor_scroll_down();
                    } else {
                        handle_move_selection(app_state, |state, len| {
                            state.select(state.selected().map_or(Some(0), |i| Some((i + 1).min(len.saturating_sub(1)))));
                        });
                    }
                }
                MouseEventKind::ScrollUp => {
                    if app_state.is_f3_displayed {
                        app_state.viewer_scroll_up();
                    } else if app_state.is_f4_displayed {
                        app_state.editor_scroll_up();
                    } else {
                        handle_move_selection(app_state, |state, _len| {
                            state.select(state.selected().map_or(Some(0), |i| Some(i.saturating_sub(1))));
                        });
                    }
                }
                MouseEventKind::ScrollLeft => {
                    if app_state.is_f3_displayed {
                        app_state.viewer_scroll_left();
                    } else if app_state.is_f4_displayed {
                        app_state.editor_scroll_left();
                    } else {
                        app_state.is_left_active = true;
                    }
                }
                MouseEventKind::ScrollRight => {
                    if app_state.is_f3_displayed {
                        app_state.viewer_scroll_right();
                    } else if app_state.is_f4_displayed {
                        app_state.editor_scroll_right();
                    } else {
                        app_state.is_left_active = false;
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }
    Ok(true)
}

fn toggle_help(app_state: &mut AppState) {
    if app_state.is_error_displayed {
        return;
    }
    app_state.is_f1_displayed = !app_state.is_f1_displayed;
}

fn toggle_preview(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }
    app_state.is_f12_displayed = !app_state.is_f12_displayed;
}

fn toggle_options(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }
    app_state.is_f11_displayed = !app_state.is_f11_displayed;
}

fn toggle_rename(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    if app_state.is_f2_displayed {
        app_state.reset_rename();
        return;
    }

    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let Some(item) = state.selected().and_then(|index| children.get(index)) else {
        return;
    };

    // Don't rename the parent entry - ".." resolves to the parent directory itself.
    if item.name == ".." {
        return;
    }

    app_state.rename_input.set(item.name_full.clone());
    app_state.is_f2_displayed = true;
}

/// One dialog serves both: F7 makes a directory, Shift+F4 an empty file.
fn toggle_create(app_state: &mut AppState, is_dir: bool) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    // The same key closes it again; the other one switches what is being made.
    if app_state.is_f7_displayed && app_state.create_is_dir == is_dir {
        app_state.reset_create();
        return;
    }

    app_state.is_f7_displayed = true;
    app_state.create_is_dir = is_dir;
    app_state.create_input.clear();
}

fn handle_rename(app_state: &mut AppState) {
    let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let selected_item = state.selected().and_then(|index| children.get(index).cloned());

    if let Some(item) = &selected_item {
        // ".." resolves to the parent directory - never rename through it.
        if item.name == ".." {
            app_state.reset_rename();
            return;
        }

        let new_name = app_state.rename_input.text.clone();
        // Nothing to rename to: treat it as a cancel, the way F7 treats an empty
        // name. Composing it would point at the parent directory instead.
        if new_name.is_empty() {
            app_state.reset_rename();
            return;
        }

        let mut original_path = parent_path.clone();
        original_path.push(item.name_full.clone());
        let mut new_path = parent_path.clone();
        new_path.push(&new_name);

        // rename() replaces the destination without a word. Equal paths mean the
        // name was left alone, which is a no-op rather than a collision.
        if new_path != original_path && path_exists(&new_path) {
            app_state.display_error(format!("Already exists: {}", new_name));
            app_state.reset_rename();
            return;
        }

        match rename_path(original_path, new_path) {
            Ok(_) => app_state.reload_panel(app_state.is_left_active, Some(&new_name)),
            Err(e) => app_state.display_error(e.to_string()),
        }

        app_state.reset_rename();
    }
}

fn handle_esc(app_state: &mut AppState) {
    app_state.reset_error();
    app_state.is_f1_displayed = false;
    app_state.is_f11_displayed = false;
    app_state.reset_rename();
    app_state.reset_create();
    app_state.reset_delete();
    app_state.reset_copy();
    app_state.reset_move();
    app_state.close_viewer();
    app_state.close_editor();
    app_state.reset_large_file();
}

fn handle_tab_switching(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }
    app_state.is_left_active = !app_state.is_left_active
}

fn handle_move_selection(app_state: &mut AppState, move_fn: impl Fn(&mut TableState, usize)) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }
    let (state, len) = if app_state.is_left_active {
        (&mut app_state.state_left, app_state.children_left.len())
    } else {
        (&mut app_state.state_right, app_state.children_right.len())
    };
    move_fn(state, len);
}

fn handle_navigate_up(app_state: &mut AppState) {
    handle_panel_operation(app_state, navigate_up_panel)
}

fn handle_enter_directory(app_state: &mut AppState) {
    handle_panel_operation(app_state, enter_directory_panel)
}

fn handle_panel_operation(app_state: &mut AppState, operation: impl FnOnce(&mut AppState)) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }
    operation(app_state);
}

fn navigate_up_panel(app_state: &mut AppState) {
    let is_left = app_state.is_left_active;
    let dir = if is_left { &app_state.dir_left } else { &app_state.dir_right };

    // Land on the directory we just came out of.
    let leaving = dir.file_name().map(|name| name.to_string_lossy().into_owned());
    let Some(parent) = dir.parent().map(Path::to_path_buf) else {
        return;
    };

    app_state.open_dir(is_left, parent, leaving.as_deref());
}

fn enter_directory_panel(app_state: &mut AppState) {
    let is_left = app_state.is_left_active;
    let (state, children, dir) = if is_left {
        (&app_state.state_left, &app_state.children_left, &app_state.dir_left)
    } else {
        (&app_state.state_right, &app_state.children_right, &app_state.dir_right)
    };

    let Some(item) = state.selected().and_then(|index| children.get(index)).cloned() else {
        return;
    };

    if item.name == ".." {
        navigate_up_panel(app_state);
        return;
    }

    if item.is_dir {
        let target = dir.join(&item.name);
        app_state.open_dir(is_left, target, None);
        return;
    }

    let file_path = dir.join(&item.name_full);
    if let Err(e) = open_with_default(&file_path) {
        app_state.display_error(format!("Cannot open file: {}", e));
    }
}

#[cfg(target_os = "macos")]
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("open").arg(path)
        .stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("cmd").args(["/C", "start", "", &path.to_string_lossy()])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("xdg-open").arg(path)
        .stdout(Stdio::null()).stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

fn toggle_delete(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    app_state.is_f8_displayed = !app_state.is_f8_displayed;

    if app_state.is_f8_displayed {
        let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
        let selected_set = if app_state.is_left_active { &app_state.selected_left } else { &app_state.selected_right };

        let items: Vec<(String, bool)> = if !selected_set.is_empty() {
            children.iter()
                .filter(|item| item.name != ".." && selected_set.contains(&item.name_full))
                .map(|item| (item.name_full.clone(), item.is_dir))
                .collect()
        } else {
            let selected_index = if app_state.is_left_active { app_state.state_left.selected().unwrap_or(0) } else { app_state.state_right.selected().unwrap_or(0) };
            if selected_index < children.len() {
                let item = &children[selected_index];
                if item.name == ".." {
                    app_state.is_f8_displayed = false;
                    return;
                }
                vec![(item.name_full.clone(), item.is_dir)]
            } else {
                app_state.is_f8_displayed = false;
                return;
            }
        };

        if items.is_empty() {
            app_state.is_f8_displayed = false;
            return;
        }

        app_state.delete_items = items;
    } else {
        app_state.reset_delete();
    }
}

fn handle_delete_confirm(app_state: &mut AppState) {
    let parent_path = if app_state.is_left_active { app_state.dir_left.clone() } else { app_state.dir_right.clone() };
    let items = std::mem::take(&mut app_state.delete_items);

    for (name, is_dir) in &items {
        let item_path = parent_path.join(name);
        if let Err(e) = delete_path(item_path, *is_dir) {
            app_state.display_error(e.to_string());
            app_state.reset_delete();
            return;
        }
    }

    app_state.reload_panel(app_state.is_left_active, None);
    app_state.clear_active_selections();
    app_state.reset_delete();
}

fn handle_create_confirm(app_state: &mut AppState) {
    if app_state.create_input.text.is_empty() {
        app_state.reset_create();
        return;
    }

    let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };

    let mut new_dir_path = parent_path.clone();
    new_dir_path.push(&app_state.create_input.text);

    let created = app_state.create_input.text.clone();
    let result = if app_state.create_is_dir {
        create_directory(new_dir_path)
    } else {
        create_file(new_dir_path)
    };
    match result {
        Ok(_) => app_state.reload_panel(app_state.is_left_active, Some(&created)),
        Err(e) => app_state.display_error(e.to_string()),
    }

    app_state.reset_create();
}

fn handle_f3_view(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    // Get selected item from active panel
    let state = if app_state.is_left_active {
        &app_state.state_left
    } else {
        &app_state.state_right
    };
    let children = if app_state.is_left_active {
        &app_state.children_left
    } else {
        &app_state.children_right
    };
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't open directories or parent entry
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = if app_state.is_left_active {
            &app_state.dir_left
        } else {
            &app_state.dir_right
        };
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_full);

        // Open viewer
        app_state.request_open(file_path, false);
    }
}

fn handle_f4_edit(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    // Get selected item from active panel
    let state = if app_state.is_left_active {
        &app_state.state_left
    } else {
        &app_state.state_right
    };
    let children = if app_state.is_left_active {
        &app_state.children_left
    } else {
        &app_state.children_right
    };
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't edit directories
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = if app_state.is_left_active {
            &app_state.dir_left
        } else {
            &app_state.dir_right
        };
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_full);

        // Open internal editor
        app_state.request_open(file_path, true);
    }
}

fn open_terminal(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    let dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let result = spawn_detached_terminal(dir);
    if let Err(e) = result {
        app_state.display_error(format!("Cannot open terminal: {}", e));
    }
}

#[cfg(target_os = "macos")]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("open").arg("-a").arg("Terminal").arg(dir)
        .stdout(Stdio::null()).stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    Command::new("cmd").args(["/C", "start", "cmd"]).current_dir(dir)
        .stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    if let Ok(term) = std::env::var("TERMINAL") {
        Command::new(&term).current_dir(dir)
            .stdout(Stdio::null()).stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        return Ok(());
    }
    let emulators = [
        "xdg-terminal-emulator",
        "alacritty",
        "kitty",
        "foot",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ];
    for emu in &emulators {
        if Command::new(emu).current_dir(dir)
            .stdout(Stdio::null()).stderr(Stdio::null())
            .process_group(0)
            .spawn().is_ok()
        {
            return Ok(());
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No terminal emulator found. Set $TERMINAL."))
}

fn toggle_copy(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    app_state.is_f5_displayed = !app_state.is_f5_displayed;

    if app_state.is_f5_displayed {
        let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
        let selected_set = if app_state.is_left_active { &app_state.selected_left } else { &app_state.selected_right };
        let source_dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
        let dest_dir = if app_state.is_left_active { &app_state.dir_right } else { &app_state.dir_left };

        let items: Vec<(PathBuf, PathBuf, bool)> = if !selected_set.is_empty() {
            children.iter()
                .filter(|item| item.name != ".." && selected_set.contains(&item.name_full))
                .map(|item| (source_dir.join(&item.name_full), dest_dir.join(&item.name_full), item.is_dir))
                .collect()
        } else {
            let selected_index = if app_state.is_left_active { app_state.state_left.selected().unwrap_or(0) } else { app_state.state_right.selected().unwrap_or(0) };
            if selected_index < children.len() {
                let item = &children[selected_index];
                if item.name == ".." {
                    app_state.is_f5_displayed = false;
                    return;
                }
                vec![(source_dir.join(&item.name_full), dest_dir.join(&item.name_full), item.is_dir)]
            } else {
                app_state.is_f5_displayed = false;
                return;
            }
        };

        if items.is_empty() {
            app_state.is_f5_displayed = false;
            return;
        }

        app_state.copy_items = items;
    } else {
        app_state.reset_copy();
    }
}

fn handle_copy_confirm(app_state: &mut AppState) {
    let items = std::mem::take(&mut app_state.copy_items);

    // Check every destination before writing anything: bailing out partway
    // through would leave some items copied and the rest not.
    if let Some((_, dest, _)) = items.iter().find(|(_, dest, _)| path_exists(dest)) {
        app_state.display_error(format!("Destination already exists: {}", dest.display()));
        app_state.reset_copy();
        return;
    }

    for (source, dest, is_dir) in &items {
        if let Err(e) = copy_path(source.clone(), dest.clone(), *is_dir) {
            app_state.display_error(e.to_string());
            app_state.reset_copy();
            return;
        }
    }

    // Reload the destination panel (opposite of active)
    app_state.reload_panel(!app_state.is_left_active, None);

    app_state.clear_active_selections();
    app_state.reset_copy();
}

fn toggle_move(app_state: &mut AppState) {
    if app_state.is_error_displayed || app_state.is_f1_displayed {
        return;
    }

    app_state.is_f6_displayed = !app_state.is_f6_displayed;

    if app_state.is_f6_displayed {
        let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
        let selected_set = if app_state.is_left_active { &app_state.selected_left } else { &app_state.selected_right };
        let source_dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
        let dest_dir = if app_state.is_left_active { &app_state.dir_right } else { &app_state.dir_left };

        let items: Vec<(PathBuf, PathBuf, bool)> = if !selected_set.is_empty() {
            children.iter()
                .filter(|item| item.name != ".." && selected_set.contains(&item.name_full))
                .map(|item| (source_dir.join(&item.name_full), dest_dir.join(&item.name_full), item.is_dir))
                .collect()
        } else {
            let selected_index = if app_state.is_left_active { app_state.state_left.selected().unwrap_or(0) } else { app_state.state_right.selected().unwrap_or(0) };
            if selected_index < children.len() {
                let item = &children[selected_index];
                if item.name == ".." {
                    app_state.is_f6_displayed = false;
                    return;
                }
                vec![(source_dir.join(&item.name_full), dest_dir.join(&item.name_full), item.is_dir)]
            } else {
                app_state.is_f6_displayed = false;
                return;
            }
        };

        if items.is_empty() {
            app_state.is_f6_displayed = false;
            return;
        }

        app_state.move_items = items;
    } else {
        app_state.reset_move();
    }
}

fn handle_move_confirm(app_state: &mut AppState) {
    let items = std::mem::take(&mut app_state.move_items);

    // Check every destination before writing anything: bailing out partway
    // through would leave some items moved and the rest not.
    if let Some((_, dest, _)) = items.iter().find(|(_, dest, _)| path_exists(dest)) {
        app_state.display_error(format!("Destination already exists: {}", dest.display()));
        app_state.reset_move();
        return;
    }

    for (source, dest, is_dir) in &items {
        if let Err(e) = move_path(source.clone(), dest.clone(), *is_dir) {
            app_state.display_error(e.to_string());
            app_state.reset_move();
            return;
        }
    }

    app_state.reload_panel(app_state.is_left_active, None);
    app_state.reload_panel(!app_state.is_left_active, None);

    app_state.clear_active_selections();
    app_state.reset_move();
}

fn handle_mouse_click(app_state: &mut AppState, column: u16, row: u16) {
    // Don't handle clicks during modal dialogs (except F2 rename which gets canceled)
    if app_state.is_error_displayed
        || app_state.is_f1_displayed
        || app_state.is_f11_displayed
        || app_state.is_f3_displayed
        || app_state.is_f4_displayed
        || app_state.is_f5_displayed
        || app_state.is_f6_displayed
        || app_state.is_f7_displayed
        || app_state.is_f8_displayed
    {
        return;
    }

    // Cancel F2 rename mode if active
    if app_state.is_f2_displayed {
        app_state.reset_rename();
    }

    // Check for double-click (same position within 500ms)
    let now = Instant::now();
    let is_double_click = if let Some(last_time) = app_state.last_click_time {
        let elapsed = now.duration_since(last_time);
        elapsed < Duration::from_millis(500) && app_state.last_click_pos == (column, row)
    } else {
        false
    };

    // Update last click tracking
    app_state.last_click_time = Some(now);
    app_state.last_click_pos = (column, row);

    // Clear all selections on mouse click
    app_state.clear_all_selections();

    // Hit-test the panels the last frame actually drew, rather than deriving
    // their position from the layout constants a second time.
    let position = Position::new(column, row);
    let clicked_left = app_state.table_area_left.contains(position);
    if !clicked_left && !app_state.table_area_right.contains(position) {
        return;
    }
    let area = if clicked_left { app_state.table_area_left } else { app_state.table_area_right };

    // The table draws its header on the first row of its area.
    if row <= area.y {
        return;
    }
    let clicked_table_row = (row - area.y - 1) as usize;

    app_state.is_left_active = clicked_left;

    let children = if clicked_left { &app_state.children_left } else { &app_state.children_right };
    let total = children.len();
    if total == 0 {
        return;
    }

    // The offset the panel was rendered with, not a second guess at it.
    let start = if clicked_left { app_state.viewport_start_left } else { app_state.viewport_start_right };
    let actual_index = start + clicked_table_row;

    // Select the row if within bounds
    if actual_index < total {
        let state = if clicked_left {
            &mut app_state.state_left
        } else {
            &mut app_state.state_right
        };
        state.select(Some(actual_index));

        // Double-click on directory: enter it
        if is_double_click {
            let children = if clicked_left {
                &app_state.children_left
            } else {
                &app_state.children_right
            };
            if actual_index < children.len() {
                if children[actual_index].is_dir {
                    enter_directory_panel(app_state);
                } else {
                    let dir = if clicked_left { &app_state.dir_left } else { &app_state.dir_right };
                    let file_path = dir.join(&children[actual_index].name_full);
                    if let Err(e) = open_with_default(&file_path) {
                        app_state.display_error(format!("Cannot open file: {}", e));
                    }
                }
            }
        }
    }
}

/// Selection in the viewer, in columns of the line as drawn, so hex and text
/// modes behave the same.
fn handle_viewer_click(app_state: &mut AppState, column: u16, row: u16, extend: bool) {
    let area = app_state.viewer_content_area;
    if !area.contains(Position::new(column, row)) {
        return;
    }

    if let Some(state) = &mut app_state.viewer_state {
        // The binary notice has nothing to select.
        if state.from_edit {
            return;
        }

        let line = (state.scroll_offset + (row - area.y) as usize).min(state.total_lines.saturating_sub(1));
        let width = state.line_text(line).chars().count();
        let position = (line, ((column - area.x) as usize + state.horizontal_offset).min(width));

        state.selection = match (extend, state.selection) {
            (true, Some((anchor, _))) => Some((anchor, position)),
            _ => Some((position, position)),
        };
    }
}

/// Place the cursor from a mouse position. `extend` is a drag, which keeps the
/// anchor where the press put it so the selection grows.
fn handle_editor_click(app_state: &mut AppState, column: u16, row: u16, extend: bool) {
    // The content area as drawn, so the border and gutter widths don't have to
    // be worked out again here.
    let area = app_state.editor_content_area;
    if !area.contains(Position::new(column, row)) {
        return;
    }

    if let Some(state) = &mut app_state.editor_state {
        let visual_row = (row - area.y) as usize;
        let target_line = (state.scroll_offset + visual_row).min(state.lines.len().saturating_sub(1));

        // Map the visual column back to a character index, past the horizontal
        // scroll and any expanded tabs.
        let visual_col = (column - area.x) as usize + state.horizontal_offset;
        let line = &state.lines[target_line];
        let mut char_col = 0;
        let mut current_visual = 0;
        for character in line.chars() {
            if current_visual >= visual_col {
                break;
            }
            current_visual += if character == '\t' { TAB_SPACES.len() } else { 1 };
            char_col += 1;
        }

        state.cursor_line = target_line;
        state.cursor_col = char_col;
        state.auto_scroll = true;
        if !extend {
            // A press anchors here; the selection stays empty until a drag moves
            // the cursor away, since an anchor equal to the cursor selects nothing.
            state.selection_anchor = Some((target_line, char_col));
        }
    }
}
