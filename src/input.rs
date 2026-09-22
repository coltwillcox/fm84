use crate::app::{AppState, Item};
use crate::fs_ops::{copy_path, create_directory, delete_path, load_directory_rows, move_path, path_exists, rename_path};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::widgets::TableState;
use std::io::Result;
use std::path::PathBuf;
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
                    let is_save = control && c == 's' && app_state.is_f4_displayed && !app_state.is_editor_save_prompt;
                    if is_save && let Err(e) = app_state.editor_save() {
                        app_state.display_error(e);
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
                        KeyCode::F(7) => toggle_create(app_state),
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
                        KeyCode::F(3) => app_state.close_viewer(),
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
                        KeyCode::Up => app_state.editor_cursor_up(),
                        KeyCode::Down => app_state.editor_cursor_down(),
                        KeyCode::Left => app_state.editor_cursor_left(),
                        KeyCode::Right => app_state.editor_cursor_right(),
                        KeyCode::Home => app_state.editor_home(),
                        KeyCode::End => app_state.editor_end(),
                        KeyCode::PageUp => app_state.editor_page_up(),
                        KeyCode::PageDown => app_state.editor_page_down(),
                        KeyCode::Enter => app_state.editor_enter(),
                        KeyCode::Backspace => app_state.editor_backspace(),
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
                    match key.code {
                        KeyCode::Esc => {
                            app_state.search_clear();
                            handle_esc(app_state);
                        }
                        KeyCode::F(1) => toggle_help(app_state),
                        KeyCode::F(2) => toggle_rename(app_state),
                        KeyCode::F(3) => handle_f3_view(app_state),
                        KeyCode::F(4) => handle_f4_edit(app_state),
                        KeyCode::F(5) => toggle_copy(app_state),
                        KeyCode::F(6) => toggle_move(app_state),
                        KeyCode::F(7) => toggle_create(app_state),
                        KeyCode::F(8) | KeyCode::Delete => toggle_delete(app_state),
                        KeyCode::F(9) => open_terminal(app_state),
                        KeyCode::F(10) => return Ok(false),
                        KeyCode::Char('q') => return Ok(false), // Temp debug
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
            Event::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::Down(_btn) => {
                    if app_state.is_f4_displayed {
                        handle_editor_click(app_state, mouse_event.column, mouse_event.row);
                    } else {
                        handle_mouse_click(app_state, mouse_event.column, mouse_event.row);
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

fn toggle_rename(app_state: &mut AppState) {
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

fn toggle_create(app_state: &mut AppState) {
    app_state.is_f7_displayed = !app_state.is_f7_displayed;
    if app_state.is_f7_displayed {
        // Opening dialog - clear input fields only
        app_state.create_input.clear();
    }
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
            Ok(_) => {
                // Only reload the active panel
                let current_dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
                match load_directory_rows( current_dir) {
                    Ok(items) => {
                        if app_state.is_left_active {
                            app_state.children_left = items;
                        } else {
                            app_state.children_right = items;
                        }
                    }
                    Err(e) => app_state.display_error(e.to_string()),
                }
            }
            Err(e) => app_state.display_error(e.to_string()),
        }

        app_state.reset_rename();
    }
}

fn handle_esc(app_state: &mut AppState) {
    app_state.reset_error();
    app_state.is_f1_displayed = false;
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
    let dir_new: std::path::PathBuf;
    let name_current: String;

    {
        let dir = if app_state.is_left_active { &mut app_state.dir_left } else { &mut app_state.dir_right };
        name_current = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(parent) = dir.parent() {
            dir_new = parent.to_path_buf();
        } else {
            return;
        }
    }

    let result = load_directory_rows( &dir_new);
    match result {
        Ok(children_new) => {
            let dir = if app_state.is_left_active { &mut app_state.dir_left } else { &mut app_state.dir_right };
            let children = if app_state.is_left_active { &mut app_state.children_left } else { &mut app_state.children_right };
            let state = if app_state.is_left_active { &mut app_state.state_left } else { &mut app_state.state_right };

            *dir = dir_new;
            *children = children_new;
            let selected_new = children.iter().position(|item| item.name == name_current).unwrap_or(0);
            state.select(Some(selected_new));
            app_state.search_clear();
            app_state.clear_active_selections();
        }
        Err(e) => app_state.display_error(e.to_string()),
    }
}

fn enter_directory_panel(app_state: &mut AppState) {
    let selected_item: Option<Item>;
    let mut parent_dir_new: Option<std::path::PathBuf>;
    let current_dir_name: Option<String>;
    let mut enter_subdir: Option<std::path::PathBuf>;

    {
        let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
        let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
        let dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };

        selected_item = state.selected().and_then(|index| children.get(index).cloned());
        parent_dir_new = None;
        current_dir_name = dir.file_name().map(|n| n.to_string_lossy().to_string());
        enter_subdir = None;

        if let Some(item) = &selected_item {
            if item.name == ".." {
                if let Some(parent) = dir.parent() {
                    parent_dir_new = Some(parent.to_path_buf());
                }
            } else if item.is_dir {
                let mut dir_new = dir.clone();
                dir_new.push(item.name.clone());
                enter_subdir = Some(dir_new);
            }
        }
    }

    if let Some(dir_new) = parent_dir_new {
        let result = load_directory_rows( &dir_new);
        match result {
            Ok(children_new) => {
                let dir = if app_state.is_left_active { &mut app_state.dir_left } else { &mut app_state.dir_right };
                let children_mut = if app_state.is_left_active { &mut app_state.children_left } else { &mut app_state.children_right };
                let state = if app_state.is_left_active { &mut app_state.state_left } else { &mut app_state.state_right };

                *dir = dir_new;
                *children_mut = children_new;
                let selected_new = children_mut.iter().position(|item| Some(&item.name) == current_dir_name.as_ref()).unwrap_or(0);
                state.select(Some(selected_new));
                app_state.search_clear();
                app_state.clear_active_selections();
            }
            Err(e) => app_state.display_error(e.to_string()),
        }
    } else if let Some(item) = &selected_item {
        if !item.is_dir {
            let dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
            let file_path = dir.join(&item.name_full);
            if let Err(e) = open_with_default(&file_path) {
                app_state.display_error(format!("Cannot open file: {}", e));
            }
            return;
        }
    }

    if let Some(dir_new) = enter_subdir {
        let result = load_directory_rows( &dir_new);
        match result {
            Ok(children_new) => {
                let dir = if app_state.is_left_active { &mut app_state.dir_left } else { &mut app_state.dir_right };
                let children = if app_state.is_left_active { &mut app_state.children_left } else { &mut app_state.children_right };
                let state = if app_state.is_left_active { &mut app_state.state_left } else { &mut app_state.state_right };

                *dir = dir_new;
                *children = children_new;
                state.select(Some(0));
                app_state.search_clear();
                app_state.clear_active_selections();
            }
            Err(e) => app_state.display_error(e.to_string()),
        }
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

    // Reload the directory
    match load_directory_rows(&parent_path) {
        Ok(new_items) => {
            if app_state.is_left_active {
                app_state.children_left = new_items;
                let len = app_state.children_left.len();
                if let Some(selected) = app_state.state_left.selected() {
                    if selected >= len {
                        app_state.state_left.select(Some(len.saturating_sub(1)));
                    }
                }
            } else {
                app_state.children_right = new_items;
                let len = app_state.children_right.len();
                if let Some(selected) = app_state.state_right.selected() {
                    if selected >= len {
                        app_state.state_right.select(Some(len.saturating_sub(1)));
                    }
                }
            }
        }
        Err(e) => app_state.display_error(e.to_string()),
    }

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

    match create_directory(new_dir_path) {
        Ok(_) => {
            // Reload the directory
            let current_dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };

            match load_directory_rows(current_dir) {
                Ok(items) => {
                    if app_state.is_left_active {
                        app_state.children_left = items;
                        // Select the newly created directory
                        if let Some(index) = app_state.children_left.iter().position(|item| item.name == app_state.create_input.text) {
                            app_state.state_left.select(Some(index));
                        }
                    } else {
                        app_state.children_right = items;
                        if let Some(index) = app_state.children_right.iter().position(|item| item.name == app_state.create_input.text) {
                            app_state.state_right.select(Some(index));
                        }
                    }
                }
                Err(e) => app_state.display_error(e.to_string()),
            }
        }
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
    let dest_dir = if app_state.is_left_active { app_state.dir_right.clone() } else { app_state.dir_left.clone() };
    match load_directory_rows(&dest_dir) {
        Ok(new_items) => {
            if app_state.is_left_active {
                app_state.children_right = new_items;
            } else {
                app_state.children_left = new_items;
            }
        }
        Err(e) => app_state.display_error(e.to_string()),
    }

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
    let source_dir = if app_state.is_left_active { app_state.dir_left.clone() } else { app_state.dir_right.clone() };
    let dest_dir = if app_state.is_left_active { app_state.dir_right.clone() } else { app_state.dir_left.clone() };

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

    // Reload source panel
    match load_directory_rows(&source_dir) {
        Ok(new_items) => {
            if app_state.is_left_active {
                app_state.children_left = new_items;
                let len = app_state.children_left.len();
                if let Some(selected) = app_state.state_left.selected() {
                    if selected >= len {
                        app_state.state_left.select(Some(len.saturating_sub(1)));
                    }
                }
            } else {
                app_state.children_right = new_items;
                let len = app_state.children_right.len();
                if let Some(selected) = app_state.state_right.selected() {
                    if selected >= len {
                        app_state.state_right.select(Some(len.saturating_sub(1)));
                    }
                }
            }
        }
        Err(e) => app_state.display_error(e.to_string()),
    }

    // Reload destination panel
    match load_directory_rows(&dest_dir) {
        Ok(new_items) => {
            if app_state.is_left_active {
                app_state.children_right = new_items;
            } else {
                app_state.children_left = new_items;
            }
        }
        Err(e) => app_state.display_error(e.to_string()),
    }

    app_state.clear_active_selections();
    app_state.reset_move();
}

fn handle_mouse_click(app_state: &mut AppState, column: u16, row: u16) {
    // Don't handle clicks during modal dialogs (except F2 rename which gets canceled)
    if app_state.is_error_displayed
        || app_state.is_f1_displayed
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

    // Get terminal size
    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));

    // Layout: top panel (3) + path bar (1) + file tables + bottom panel (1) + f-key bar (3)
    // File tables start at row 4, with header row at 4, data starts at row 5
    let table_start_row = 4u16;
    let table_end_row = term_height.saturating_sub(4); // Bottom panel (1) + f-key bar (3)

    // Check if click is within file table area
    if row < table_start_row || row >= table_end_row {
        return;
    }

    // Calculate which row in the table was clicked (accounting for header)
    let header_row = table_start_row;
    if row <= header_row {
        return; // Clicked on header
    }

    let clicked_table_row = (row - header_row - 1) as usize;

    // Determine which panel was clicked (left half or right half)
    let panel_width = term_width / 2;
    let clicked_left = column < panel_width;

    // Set active panel
    app_state.is_left_active = clicked_left;

    // Get the viewport offset for the clicked panel to calculate actual index
    let children = if clicked_left {
        &app_state.children_left
    } else {
        &app_state.children_right
    };

    let total = children.len();
    if total == 0 {
        return;
    }

    // Calculate viewport offset (same logic as in ui.rs build_viewport_rows)
    let viewport_height = (table_end_row - table_start_row - 1) as usize;
    let state = if clicked_left {
        &app_state.state_left
    } else {
        &app_state.state_right
    };
    let selected = state.selected().unwrap_or(0);
    let half_view = viewport_height / 2;

    let start = if selected <= half_view {
        0
    } else if selected + half_view >= total {
        total.saturating_sub(viewport_height)
    } else {
        selected.saturating_sub(half_view)
    };

    // Calculate actual index from clicked row
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

fn handle_editor_click(app_state: &mut AppState, column: u16, row: u16) {
    let (term_width, term_height) = crossterm::terminal::size().unwrap_or((80, 24));

    if let Some(state) = &mut app_state.editor_state {
        // Editor area matches chunks_main[2] from render_ui layout:
        // top panel (3) + path bar (1) = 4, plus 1 for border
        let editor_top = 5u16;
        // bottom panel (1) + fkey bar (3) = 4, plus 1 for border
        let editor_bottom = term_height.saturating_sub(5);

        let total_lines = state.lines.len();
        let line_num_width = (total_lines.to_string().len() as u16).max(3) + 2;
        // left border (1) + gutter
        let content_left = 1 + line_num_width;
        // right border
        let content_right = term_width.saturating_sub(1);

        if row < editor_top || row >= editor_bottom || column < content_left || column >= content_right {
            return;
        }

        // Map to line
        let visual_row = (row - editor_top) as usize;
        let target_line = (state.scroll_offset + visual_row).min(total_lines.saturating_sub(1));

        // Map visual column to char column (accounting for horizontal scroll and tabs)
        let visual_col = (column - content_left) as usize + state.horizontal_offset;
        let line = &state.lines[target_line];
        let mut char_col = 0;
        let mut current_visual = 0;
        for ch in line.chars() {
            if current_visual >= visual_col {
                break;
            }
            current_visual += if ch == '\t' { 4 } else { 1 };
            char_col += 1;
        }

        state.cursor_line = target_line;
        state.cursor_col = char_col;
        state.auto_scroll = true;
    }
}
