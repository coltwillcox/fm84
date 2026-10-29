//! Keys in the panels: moving, selecting, opening, and the F-keys that act on
//! the entry under the cursor.

use super::*;

pub(super) fn panel_key(app_state: &mut AppState, key: KeyEvent) {
    // A panel still being read shows what it did before, and a key acting on
    // that would act on the wrong directory. Esc stops waiting for it, and Tab
    // goes over to the other panel, which works as ever.
    let is_left = app_state.is_left_active;
    if app_state.panel_busy(is_left) {
        match key.code {
            KeyCode::Esc => app_state.cancel_listing(is_left),
            KeyCode::Tab => app_state.is_left_active = !is_left,
            _ => {}
        }
        return;
    }
    let drive_chord = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app_state.search_clear();
            app_state.cancel_dir_sizes();
        }
        // Alt+F1/F2 choose a drive per panel. Ctrl is an alias, because
        // window managers commonly eat Alt+F1 and Alt+F2.
        KeyCode::F(1) if drive_chord => app_state.open_drive_picker(true),
        KeyCode::F(2) if drive_chord => app_state.open_drive_picker(false),
        KeyCode::F(1) => {
            app_state.help_scroll = 0;
            app_state.dialog = Some(Dialog::Help);
        }
        // Shift+F2 compares the panels, as in Total Commander.
        KeyCode::F(2) if key.modifiers.contains(KeyModifiers::SHIFT) => app_state.compare_dirs(),
        KeyCode::F(2) => open_rename(app_state),
        // Ctrl+F3 to Ctrl+F6 sort by name, extension, date and size, as in
        // Total Commander; the same one again turns the order round.
        // Ctrl+Shift+F5 makes a symbolic link, as in Total Commander - ahead
        // of Ctrl+F5, which would take it for a sort. Alt+F5 is an alias,
        // since kitty takes Ctrl+Shift+F5 for reloading its config.
        KeyCode::F(5) if key.modifiers.contains(KeyModifiers::ALT) || (control && key.modifiers.contains(KeyModifiers::SHIFT)) => open_symlink(app_state),
        KeyCode::F(3) if control => app_state.sort_panels_by(SortKey::Name),
        KeyCode::F(4) if control => app_state.sort_panels_by(SortKey::Extension),
        KeyCode::F(5) if control => app_state.sort_panels_by(SortKey::Modified),
        KeyCode::F(6) if control => app_state.sort_panels_by(SortKey::Size),
        // Shift+F3 compares two files, as F3 views one. = does too, for a
        // terminal that cannot tell Shift+F3 from a report of its cursor.
        KeyCode::F(3) if key.modifiers.contains(KeyModifiers::SHIFT) => open_compare(app_state),
        KeyCode::Char('=') => open_compare(app_state),
        KeyCode::F(3) => handle_f3_view(app_state),
        // Shift+F4 creates an empty file, as MC does.
        KeyCode::F(4) if key.modifiers.contains(KeyModifiers::SHIFT) => toggle_create(app_state, false),
        KeyCode::F(4) => handle_f4_edit(app_state),
        KeyCode::F(5) => open_transfer(app_state, true),
        KeyCode::F(6) => open_transfer(app_state, false),
        // Alt+F7 finds files, as in Total Commander and MC. Ctrl is an alias,
        // since desktops commonly take Alt+F7 for moving a window.
        KeyCode::F(7) if drive_chord => app_state.open_find_files(),
        KeyCode::F(7) => toggle_create(app_state, true),
        // Shift deletes for good, whatever F11 says F8 does.
        KeyCode::F(8) | KeyCode::Delete => open_delete(app_state, key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::F(9) => open_terminal(app_state),
        KeyCode::F(11) => app_state.dialog = Some(Dialog::Options { editing: None }),
        KeyCode::F(12) => app_state.show_preview = !app_state.show_preview,
        // Back and forward through where this tab has been, as a browser.
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => app_state.travel(true),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => app_state.travel(false),
        KeyCode::Left if control => app_state.open_in_panel(true),
        KeyCode::Right if control => app_state.open_in_panel(false),
        // Space toggles the selection and moves on, working out the size of a
        // directory; Insert does the same without.
        KeyCode::Char(' ') => app_state.toggle_selection(),
        KeyCode::Insert => app_state.toggle_selection_no_size(),
        // + and - select and deselect by a pattern, * inverts. A name can
        // start with -, so once a search is under way the key goes on into
        // that instead.
        KeyCode::Char('+') => app_state.open_select_prompt(true),
        KeyCode::Char('-') if app_state.search_input.is_empty() => app_state.open_select_prompt(false),
        KeyCode::Char('*') => app_state.invert_selection(false),
        KeyCode::Char(c) if c.is_alphanumeric() || ".-_".contains(c) => app_state.search_add_char(c),
        KeyCode::Backspace if app_state.search_input.is_empty() => navigate_up_panel(app_state),
        KeyCode::Backspace => app_state.search_backspace(),
        KeyCode::Tab => app_state.is_left_active = !app_state.is_left_active,
        KeyCode::Down if !app_state.search_input.is_empty() => app_state.jump_to_next_match(),
        KeyCode::Up if !app_state.search_input.is_empty() => app_state.jump_to_prev_match(),
        KeyCode::Down => move_cursor(app_state, |state, len| {
            state.select(state.selected().map_or(Some(0), |i| Some((i + 1).min(len.saturating_sub(1)))));
        }),
        KeyCode::Up => move_cursor(app_state, |state, _len| {
            state.select(state.selected().map_or(Some(0), |i| Some(i.saturating_sub(1))));
        }),
        KeyCode::PageDown if control => app_state.cycle_tab(true),
        KeyCode::PageUp if control => app_state.cycle_tab(false),
        KeyCode::PageDown => {
            let page_size = app_state.page_size as usize;
            move_cursor(app_state, |state, len| {
                state.select(state.selected().map(|selected| (selected + page_size).min(len.saturating_sub(1))));
            })
        }
        KeyCode::PageUp => {
            let page_size = app_state.page_size as usize;
            move_cursor(app_state, |state, _len| {
                state.select(state.selected().map(|selected| selected.saturating_sub(page_size)));
            })
        }
        KeyCode::Home => move_cursor(app_state, |state, _len| state.select(Some(0))),
        KeyCode::End => move_cursor(app_state, |state, len| state.select(Some(len.saturating_sub(1)))),
        // Alt+Shift+Enter sizes every directory, as in Total Commander. Alt
        // alone too, for a terminal that cannot report Shift with Enter.
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => app_state.size_all_dirs(),
        KeyCode::Enter => enter_directory_panel(app_state),
        _ => {}
    }
}

/// Move the active panel's cursor - not over one still being read, whose
/// rows are about to be replaced.
pub(super) fn move_cursor(app_state: &mut AppState, move_fn: impl Fn(&mut TableState, usize)) {
    if app_state.panel_busy(app_state.is_left_active) {
        return;
    }
    let (state, len) = if app_state.is_left_active {
        (&mut app_state.state_left, app_state.children_left.len())
    } else {
        (&mut app_state.state_right, app_state.children_right.len())
    };
    move_fn(state, len);
}

fn navigate_up_panel(app_state: &mut AppState) {
    let is_left = app_state.is_left_active;
    let dir = if is_left { &app_state.dir_left } else { &app_state.dir_right };

    // Land on the directory we just came out of.
    let leaving = dir.file_name().map(std::ffi::OsStr::to_os_string);
    let Some(parent) = dir.parent().map(Path::to_path_buf) else {
        return;
    };

    app_state.open_dir(is_left, parent, leaving.as_deref());
}

pub(super) fn enter_directory_panel(app_state: &mut AppState) {
    let is_left = app_state.is_left_active;
    let (state, children, dir) = if is_left {
        (&app_state.state_left, &app_state.children_left, &app_state.dir_left)
    } else {
        (&app_state.state_right, &app_state.children_right, &app_state.dir_right)
    };

    let Some(item) = state.selected().and_then(|index| children.get(index)).cloned() else {
        return;
    };

    if item.is_parent() {
        navigate_up_panel(app_state);
        return;
    }

    if item.is_dir {
        let target = item.path_in(dir);
        app_state.open_dir(is_left, target, None);
        return;
    }

    let file_path = item.path_in(dir);
    if let Err(e) = open_with_default(&file_path) {
        app_state.display_error(format!("Cannot open file: {}", e));
    }
}

/// What F5, F6 and F8 work on in the active panel: what is selected, or the
/// entry under the cursor when nothing is. Never the parent entry.
pub(super) fn chosen_items(app_state: &AppState) -> Vec<(PathBuf, bool)> {
    let (children, selected, state, dir) = if app_state.is_left_active {
        (&app_state.children_left, &app_state.selected_left, &app_state.state_left, &app_state.dir_left)
    } else {
        (&app_state.children_right, &app_state.selected_right, &app_state.state_right, &app_state.dir_right)
    };
    let chosen: Vec<&crate::app::Item> = if selected.is_empty() {
        state.selected().and_then(|index| children.get(index)).into_iter().collect()
    } else {
        children.iter().filter(|item| selected.contains(&item.name_os)).collect()
    };
    chosen.into_iter().filter(|item| !item.is_parent()).map(|item| (item.path_in(dir), item.is_dir)).collect()
}

/// Ctrl+C copies the full paths of what is selected, or of the entry under
/// the cursor, to the clipboard, a line each; Alt+C the names alone. On the
/// parent entry, the panel's own directory. Names that are not valid UTF-8
/// go as near as they can be read: a clipboard holds text.
pub(super) fn copy_names(app_state: &mut AppState, full: bool) {
    let mut items: Vec<PathBuf> = chosen_items(app_state).into_iter().map(|(path, _)| path).collect();
    if items.is_empty() {
        items.push(app_state.panel_dir(app_state.is_left_active));
    }
    let text = items
        .iter()
        .map(|path| match path.file_name() {
            Some(name) if !full => name.to_string_lossy().into_owned(),
            _ => path.to_string_lossy().into_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    crate::utils::set_system_clipboard(&text);
    let what = if full { "path" } else { "name" };
    app_state.find_note = Some(match items.len() {
        1 => format!("Copied {what}: {text}"),
        count => format!("Copied {count} {what}s"),
    });
    // The editor pastes from it too, which works where OSC 52 does not.
    app_state.clipboard = text;
}

fn handle_f3_view(app_state: &mut AppState) {
    // Get selected item from active panel
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't open directories or parent entry
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_os);

        // Open viewer
        app_state.request_open(file_path, false);
    }
}

fn handle_f4_edit(app_state: &mut AppState) {
    // Get selected item from active panel
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't edit directories
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_os);

        // An external editor, when F11 names one; otherwise the built-in.
        if app_state.options.editor.trim().is_empty() {
            app_state.request_open(file_path, true);
        } else {
            app_state.external_edit = Some(file_path);
        }
    }
}
