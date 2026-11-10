//! Keys in the panels: moving, selecting, opening, and the F-keys that act on
//! the entry under the cursor.

use super::*;

pub(super) fn panel_key(app_state: &mut AppState, key: KeyEvent) {
    // A panel still being read shows what it did before, and a key acting on
    // that would act on the wrong directory. Esc stops waiting for it, and Tab
    // goes over to the other panel, which works as ever.
    let side = app_state.active_side;
    if app_state.panel_busy(side) {
        match key.code {
            KeyCode::Esc => app_state.cancel_listing(side),
            KeyCode::Tab => app_state.active_side = !side,
            _ => {}
        }
        return;
    }
    if app_state.panel(side).tree.is_some() {
        return tree_key(app_state, key);
    }
    let drive_chord = key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Alt+F10, Norton Commander's tree: the panel as its drive's
        // directories.
        KeyCode::F(10) if key.modifiers.contains(KeyModifiers::ALT) => app_state.toggle_tree(),
        KeyCode::Esc => {
            app_state.search_clear();
            app_state.cancel_dir_sizes();
        }
        // Alt+F1/F2 choose a drive per panel. Ctrl is an alias, because
        // window managers commonly eat Alt+F1 and Alt+F2.
        KeyCode::F(1) if drive_chord => app_state.open_drive_picker(Side::Left),
        KeyCode::F(2) if drive_chord => app_state.open_drive_picker(Side::Right),
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
        // Alt+F9 extracts an archive, as in Total Commander. Ctrl is an
        // alias, since desktops take Alt+F9 for hiding a window.
        KeyCode::F(9) if drive_chord => open_extract_all(app_state),
        KeyCode::F(9) => open_terminal(app_state),
        KeyCode::F(11) => app_state.dialog = Some(Dialog::Options { editing: None }),
        KeyCode::F(12) => app_state.show_preview = !app_state.show_preview,
        // Back and forward through where this tab has been, as a browser.
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => app_state.travel(true),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => app_state.travel(false),
        // Anywhere that has been, across the tabs, as Total Commander's.
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => app_state.open_history(),
        KeyCode::Left if control => app_state.open_in_panel(Side::Left),
        KeyCode::Right if control => app_state.open_in_panel(Side::Right),
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
        KeyCode::Tab => app_state.active_side = !app_state.active_side,
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
            let page_size = app_state.drawn.page_size as usize;
            move_cursor(app_state, |state, len| {
                state.select(state.selected().map(|selected| (selected + page_size).min(len.saturating_sub(1))));
            })
        }
        KeyCode::PageUp => {
            let page_size = app_state.drawn.page_size as usize;
            move_cursor(app_state, |state, _len| {
                state.select(state.selected().map(|selected| selected.saturating_sub(page_size)));
            })
        }
        KeyCode::Home => move_cursor(app_state, |state, _len| state.select(Some(0))),
        KeyCode::End => move_cursor(app_state, |state, len| state.select(Some(len.saturating_sub(1)))),
        // Alt+Shift+Enter sizes every directory, as in Total Commander. Alt
        // alone too, for a terminal that cannot report Shift with Enter.
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => app_state.size_all_dirs(),
        // Shift+Enter opens what is under the cursor as the system would,
        // archive or not. Ctrl+O too, for a terminal that sends Shift+Enter
        // as Enter.
        KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => open_cursor_externally(app_state),
        KeyCode::Enter => enter_directory_panel(app_state),
        _ => {}
    }
}

/// Move the active panel's cursor - not over one still being read, whose
/// rows are about to be replaced.
pub(super) fn move_cursor(app_state: &mut AppState, move_fn: impl Fn(&mut TableState, usize)) {
    if app_state.panel_busy(app_state.active_side) {
        return;
    }
    let panel = app_state.panel_mut(app_state.active_side);
    let (state, len) = (&mut panel.state, panel.children.len());
    move_fn(state, len);
}

fn navigate_up_panel(app_state: &mut AppState) {
    let side = app_state.active_side;
    let dir = &app_state.panel(side).dir;

    // Land on the directory we just came out of.
    let leaving = dir.file_name().map(std::ffi::OsStr::to_os_string);
    let Some(parent) = dir.parent().map(Path::to_path_buf) else {
        return;
    };

    app_state.open_dir(side, parent, leaving.as_deref());
}

/// A panel showing its tree: up and down it, each directory opened and
/// closed, Enter sending the other panel there. Nothing else acts, the
/// files behind it out of sight; F1 and Tab work as ever.
fn tree_key(app_state: &mut AppState, key: KeyEvent) {
    let page = app_state.drawn.page_size.max(2) as isize - 1;
    let typing = !app_state.search_input.is_empty();
    // What is typed is let go of by any key but the typing and Up and Down,
    // which go between the directories it matches.
    let search_key = matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Up | KeyCode::Down);
    if typing && !search_key {
        app_state.search_input.clear();
        // Esc only puts the typing away.
        if key.code == KeyCode::Esc {
            return;
        }
    }
    match key.code {
        KeyCode::F(10) if key.modifiers.contains(KeyModifiers::ALT) => app_state.toggle_tree(),
        KeyCode::Esc => app_state.toggle_tree(),
        KeyCode::Up if typing => app_state.tree_search_next(false),
        KeyCode::Down if typing => app_state.tree_search_next(true),
        KeyCode::Backspace if typing => app_state.tree_search_back(),
        // - is a name's as much as the tree's, once typing has begun.
        KeyCode::Char(c) if c.is_alphanumeric() || "._ ".contains(c) || (typing && c == '-') => app_state.tree_search_add(c),
        KeyCode::Char('*') => app_state.tree_open_all(),
        // On the directory under the cursor, as in the list: copied or moved
        // to the other panel, made in, deleted - in an archive, as it is
        // done there. Not on the top of the tree.
        KeyCode::F(5 | 6 | 8) | KeyCode::Delete if chosen_items(app_state).is_empty() => app_state.display_error(t!("Not the top of the tree").to_string()),
        KeyCode::F(5) => open_transfer(app_state, true),
        KeyCode::F(6) => open_transfer(app_state, false),
        KeyCode::F(7) => toggle_create(app_state, true),
        KeyCode::F(8) | KeyCode::Delete => open_delete(app_state, key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::Up => app_state.tree_move(-1),
        KeyCode::Down => app_state.tree_move(1),
        KeyCode::PageUp => app_state.tree_move(-page),
        KeyCode::PageDown => app_state.tree_move(page),
        KeyCode::Home => app_state.tree_move(isize::MIN),
        KeyCode::End => app_state.tree_move(isize::MAX),
        KeyCode::Right | KeyCode::Char('+') => app_state.tree_expand(),
        KeyCode::Left | KeyCode::Char('-') => app_state.tree_collapse(),
        KeyCode::Enter => app_state.tree_enter(),
        KeyCode::Tab => app_state.active_side = !app_state.active_side,
        KeyCode::F(1) => {
            app_state.help_scroll = 0;
            app_state.dialog = Some(Dialog::Help);
        }
        _ => {}
    }
}

pub(super) fn enter_directory_panel(app_state: &mut AppState) {
    let side = app_state.active_side;
    let panel = app_state.panel(side);
    let (state, children, dir) = (&panel.state, &panel.children, &panel.dir);

    let Some(item) = state.selected().and_then(|index| children.get(index)).cloned() else {
        return;
    };

    if item.is_parent() {
        navigate_up_panel(app_state);
        return;
    }

    if item.is_dir {
        let target = item.path_in(dir);
        app_state.open_dir(side, target, None);
        return;
    }

    let file_path = item.path_in(dir);
    // A file that went to the trash, put back where it came from; one in a
    // directory that went is opened from there, as from an archive.
    if app_state.panel_in_trash(side) && app_state.panel_archive(side).is_some_and(|archive| file_path.parent() == Some(archive.as_path())) {
        return app_state.restore_from_trash(vec![(file_path, false)]);
    }
    open_file(app_state, file_path);
}

/// Enter on a file, or a double-click: an archive is gone into, as a
/// directory is, and anything else is opened as the system opens it. One
/// inside an archive is opened from there, archive or not: archives within
/// archives are not gone into.
pub(super) fn open_file(app_state: &mut AppState, path: PathBuf) {
    let side = app_state.active_side;
    if app_state.panel_archive(side).is_none() && crate::archive::kind_of(&path).is_some() {
        app_state.open_dir(side, path, None);
        return;
    }
    open_externally(app_state, path, false);
}

/// Shift+Enter or Ctrl+O: the entry under the cursor, opened as the system
/// opens it - what Enter did before it went into archives.
pub(super) fn open_cursor_externally(app_state: &mut AppState) {
    let side = app_state.active_side;
    let panel = app_state.panel(side);
    let (state, children, dir) = (&panel.state, &panel.children, &panel.dir);
    let Some(item) = state.selected().and_then(|index| children.get(index)).filter(|item| !item.is_parent()) else {
        return;
    };
    let (path, is_dir) = (item.path_in(dir), item.is_dir);
    open_externally(app_state, path, is_dir);
}

/// Alt+O: the chooser of programs for the entry under the cursor.
pub(super) fn open_cursor_with(app_state: &mut AppState) {
    let side = app_state.active_side;
    let panel = app_state.panel(side);
    let (state, children, dir) = (&panel.state, &panel.children, &panel.dir);
    let Some(item) = state.selected().and_then(|index| children.get(index)).filter(|item| !item.is_parent()) else {
        return;
    };
    let (path, is_dir) = (item.path_in(dir), item.is_dir);
    app_state.open_with_menu(path, is_dir);
}

/// Hand `path` to the program the `open` file or the system opens it
/// with. From inside an archive it is copied out first, and a directory
/// there has nowhere on the disk to be opened from.
fn open_externally(app_state: &mut AppState, path: PathBuf, is_dir: bool) {
    if app_state.panel_archive(app_state.active_side).is_some() {
        if !is_dir {
            app_state.open_archive_entry(&path, false);
        }
        return;
    }
    app_state.open_outside(&path, is_dir);
}

/// What F5, F6 and F8 work on in the active panel: what is selected, or the
/// entry under the cursor when nothing is. Never the parent entry.
pub(super) fn chosen_items(app_state: &AppState) -> Vec<(PathBuf, bool)> {
    let panel = app_state.panel(app_state.active_side);
    // In the tree, the directory under the cursor - never the top of it,
    // the drive's root or the whole of an archive.
    if panel.tree.is_some() {
        let top = app_state.tree_top(app_state.active_side);
        return app_state.tree_cursor(app_state.active_side).filter(|path| Some(path.as_path()) != top).map(|path| vec![(path, true)]).unwrap_or_default();
    }
    let (children, selected, state, dir) = (&panel.children, &panel.selected, &panel.state, &panel.dir);
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
        items.push(app_state.panel_dir(app_state.active_side));
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
    app_state.find_note = Some(match (items.len(), full) {
        (1, true) => t!("Copied path: {text}", text),
        (1, false) => t!("Copied name: {text}", text),
        (count, true) => tn!(count, "Copied {count} path", "Copied {count} paths", count),
        (count, false) => tn!(count, "Copied {count} name", "Copied {count} names", count),
    });
    // The editor pastes from it too, which works where OSC 52 does not.
    app_state.clipboard = text;
}

fn handle_f3_view(app_state: &mut AppState) {
    // Get selected item from active panel
    let state = &app_state.panel(app_state.active_side).state;
    let children = &app_state.panel(app_state.active_side).children;
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't open directories or parent entry
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = &app_state.panel(app_state.active_side).dir;
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_os);

        // Out of an archive first, into a scratch directory.
        if app_state.panel_archive(app_state.active_side).is_some() {
            app_state.open_archive_entry(&file_path, true);
            return;
        }
        app_state.request_open(file_path, false);
    }
}

fn handle_f4_edit(app_state: &mut AppState) {
    // Get selected item from active panel
    let state = &app_state.panel(app_state.active_side).state;
    let children = &app_state.panel(app_state.active_side).children;
    let selected_item = state.selected().and_then(|index| children.get(index));

    if let Some(item) = selected_item {
        // Don't edit directories
        if item.is_dir {
            return;
        }

        // Build file path
        let parent_path = &app_state.panel(app_state.active_side).dir;
        let mut file_path = parent_path.clone();
        file_path.push(&item.name_os);

        // Nothing saved could go back into the archive. F3 shows it.
        if app_state.refuse_in_archive(app_state.active_side) {
            return;
        }
        // An external editor, when F11 names one; otherwise the built-in.
        if app_state.options.editor.trim().is_empty() {
            app_state.request_open(file_path, true);
        } else {
            app_state.external_edit = Some(file_path);
        }
    }
}
