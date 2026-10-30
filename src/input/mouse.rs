//! The mouse: clicks, drags and the wheel, to whatever is under them.

use super::*;

/// The mouse, to what is on top. Nothing behind a popup responds to it - not
/// clicks and not the wheel, which would otherwise scroll a panel out of
/// sight of the dialog asking about it. Alt+A's fields and the rows of the
/// branch list are the things in a popup a click reaches. False to quit.
pub(super) fn mouse_to(app_state: &mut AppState, mouse_event: MouseEvent) -> bool {
    if !app_state.popup_is_open() {
        return handle_mouse(app_state, mouse_event);
    }
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) && app_state.error.is_none() && app_state.job.is_none() {
        match app_state.dialog {
            Some(Dialog::Attributes(_)) => app_state.attributes_click(mouse_event.column, mouse_event.row),
            Some(Dialog::Branches(_)) => app_state.click_branch(mouse_event.column, mouse_event.row),
            _ => {}
        }
    }
    // The wheel goes up and down F11's list, as the arrows do - not while a
    // command is being typed into it - and F1's.
    if app_state.error.is_none() && app_state.job.is_none() {
        let down = match mouse_event.kind {
            MouseEventKind::ScrollDown => Some(true),
            MouseEventKind::ScrollUp => Some(false),
            _ => None,
        };
        match (&app_state.dialog, down) {
            (Some(Dialog::Options { editing: None }), Some(down)) => app_state.options_move(down),
            (Some(Dialog::Help), Some(down)) => scroll_help(app_state, if down { 3 } else { -3 }),
            _ => {}
        }
    }
    true
}

/// False to quit, which F10 clicked on the key bar does.
pub(super) fn handle_mouse(app_state: &mut AppState, mouse_event: MouseEvent) -> bool {
    let (column, row) = (mouse_event.column, mouse_event.row);
    let (viewing, editing) = (app_state.is_viewing(), app_state.is_editing());
    // A click anywhere but on the path being edited puts Ctrl+L's field
    // away, as one off the name being renamed does, and then goes on to be
    // the click it was.
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) && matches!(app_state.dialog, Some(Dialog::Prompt(PromptKind::GoTo, _))) {
        let on_left = column < app_state.table_area_right.x;
        if !app_state.path_bar_area.contains(Position::new(column, row)) || on_left != app_state.is_left_active {
            app_state.dialog = None;
        }
    }
    // A click keeps the filter as typed so far, and goes on to be the click:
    // one on the other panel would otherwise have the typing narrow that.
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) && matches!(app_state.dialog, Some(Dialog::Prompt(PromptKind::Filter, _))) {
        app_state.dialog = None;
        app_state.filter_before.clear();
    }
    // A label on the key bar is its key. Not under the rename field or a
    // prompt, which would take the key as one of theirs.
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left)
        && app_state.dialog.is_none()
        && let Some(&(_, key)) = app_state.fkey_hits.iter().find(|(cell, _)| cell.contains(Position::new(column, row)))
    {
        return handle_key(app_state, KeyEvent::new(KeyCode::F(key), KeyModifiers::NONE));
    }
    // Over the diff, the wheel scrolls it and nothing else answers: the
    // panels it covers would otherwise take the clicks.
    if matches!(app_state.screen, Screen::Diff(_)) {
        match mouse_event.kind {
            MouseEventKind::ScrollDown => app_state.diff_scroll(3, 0),
            MouseEventKind::ScrollUp => app_state.diff_scroll(-3, 0),
            MouseEventKind::ScrollRight => app_state.diff_scroll(0, 4),
            MouseEventKind::ScrollLeft => app_state.diff_scroll(0, -4),
            _ => {}
        }
        return true;
    }
    if (viewing || editing) && scroll_bar_mouse(app_state, mouse_event.kind, column, row) {
        return true;
    }
    match mouse_event.kind {
        // A middle click closes the tab under it, as in a browser.
        MouseEventKind::Down(MouseButton::Middle) if !viewing && !editing => {
            if let Some((is_left, StripHit::Item(index))) = app_state.tab_at(column, row) {
                app_state.close_tab(is_left, index);
            }
        }
        // The left button only. The right one has nothing to offer yet, and
        // taken for the left it moved the cursor out from under a click that
        // was meant for something else.
        MouseEventKind::Down(MouseButton::Left) => {
            if editing {
                handle_editor_click(app_state, column, row, false);
            } else if viewing {
                // A picture is dragged about; anything else is selected.
                if app_state.viewer_shows_image() {
                    app_state.viewer_pan_start(column, row);
                } else {
                    handle_viewer_click(app_state, column, row, false);
                }
            } else {
                handle_mouse_click(app_state, column, row);
            }
        }
        // Dragging extends whatever the press started.
        MouseEventKind::Drag(MouseButton::Left) => {
            if editing {
                handle_editor_click(app_state, column, row, true);
            } else if viewing {
                if app_state.viewer_shows_image() {
                    app_state.viewer_pan_to(column, row);
                } else {
                    handle_viewer_click(app_state, column, row, true);
                }
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let down = mouse_event.kind == MouseEventKind::ScrollDown;
            if viewing {
                if down { app_state.viewer_scroll_down() } else { app_state.viewer_scroll_up() }
            } else if editing {
                if down { app_state.editor_scroll_down() } else { app_state.editor_scroll_up() }
            } else if let Some(is_left) = app_state.drive_strip_at(column, row) {
                // Over a drive or tab strip, the wheel moves it along.
                app_state.scroll_drive_strip(is_left, down);
            } else if let Some(is_left) = app_state.tab_strip_at(column, row) {
                app_state.scroll_tab_strip(is_left, down);
            } else if down {
                move_cursor(app_state, |state, len| {
                    state.select(state.selected().map_or(Some(0), |i| Some((i + 1).min(len.saturating_sub(1)))));
                });
            } else {
                move_cursor(app_state, |state, _len| {
                    state.select(state.selected().map_or(Some(0), |i| Some(i.saturating_sub(1))));
                });
            }
        }
        MouseEventKind::ScrollLeft => {
            if viewing {
                app_state.viewer_scroll_left();
            } else if editing {
                app_state.editor_scroll_left();
            } else {
                app_state.is_left_active = true;
            }
        }
        MouseEventKind::ScrollRight => {
            if viewing {
                app_state.viewer_scroll_right();
            } else if editing {
                app_state.editor_scroll_right();
            } else {
                app_state.is_left_active = false;
            }
        }
        _ => {}
    }
    true
}

/// The left button on the viewer's or the editor's scroll bars: pressed on a
/// thumb, it takes hold of it, and the thumb follows the pointer until the
/// button comes up - wherever the pointer goes meanwhile, as in any window;
/// pressed on the track either side, it is a page that way. True when the
/// event was the bars', and goes no further.
fn scroll_bar_mouse(app_state: &mut AppState, kind: MouseEventKind, column: u16, row: u16) -> bool {
    use crate::scroll_bar::BarPress;
    let scroll_to = |app_state: &mut AppState, vertical: bool, offset: usize| {
        if app_state.is_editing() {
            app_state.editor_scroll_to(vertical, offset);
        } else {
            app_state.viewer_scroll_to(vertical, offset);
        }
    };
    match kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app_state.bar_drag = None;
            let Some((bar, press)) = app_state.scroll_bars.iter().find_map(|bar| bar.press(column, row).map(|press| (*bar, press))) else {
                return false;
            };
            match press {
                BarPress::Thumb(grab) => app_state.bar_drag = Some((bar, grab)),
                BarPress::Page { forward } => {
                    let crate::scroll_bar::Extent { offset, view, .. } = bar.extent;
                    scroll_to(app_state, bar.vertical, if forward { offset + view } else { offset.saturating_sub(view) });
                }
            }
            true
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let Some((bar, grab)) = app_state.bar_drag else {
                return false;
            };
            scroll_to(app_state, bar.vertical, bar.drag_to(column, row, grab));
            true
        }
        MouseEventKind::Up(MouseButton::Left) => app_state.bar_drag.take().is_some(),
        _ => false,
    }
}

/// A click on the panels. Popups are turned away before the event gets this
/// far, and the viewer and editor are picked off by the caller; a second list
/// of them here is what let a transfer's popup be clicked straight through.
fn handle_mouse_click(app_state: &mut AppState, column: u16, row: u16) {
    // A click anywhere cancels a rename.
    if matches!(app_state.dialog, Some(Dialog::Rename(_))) {
        app_state.dialog = None;
    }

    // A drive icon sends that panel to the mount, the same as picking one with
    // Alt+F1 or Alt+F2. Checked before the panels, since the strip sits above
    // them and a click there is never a click on a file.
    // The arrows at its ends move it along when not every drive fits.
    if let Some((is_left, hit)) = app_state.drive_at(column, row) {
        match hit {
            StripHit::Item(index) => {
                if let Some(path) = app_state.mounts.get(index).map(|mount| mount.path.clone()) {
                    app_state.is_left_active = is_left;
                    app_state.open_dir(is_left, path, None);
                }
            }
            StripHit::Back => app_state.scroll_drive_strip(is_left, false),
            StripHit::Forward => app_state.scroll_drive_strip(is_left, true),
        }
        return;
    }

    // A tab shows it in that panel, which becomes the active one.
    if let Some((is_left, hit)) = app_state.tab_at(column, row) {
        match hit {
            StripHit::Item(index) => {
                app_state.is_left_active = is_left;
                app_state.select_tab(is_left, index);
            }
            StripHit::Back => app_state.scroll_tab_strip(is_left, false),
            StripHit::Forward => app_state.scroll_tab_strip(is_left, true),
        }
        return;
    }

    // A panel's git branch, after its path, lists the repository's branches
    // to change to; the panel becomes the active one. Checked before the
    // path, which the branch is drawn along with.
    if let Some(is_left) = [true, false].into_iter().find(|&is_left| app_state.branch_hits[usize::from(!is_left)].is_some_and(|cell| cell.contains(Position::new(column, row)))) {
        if app_state.job.is_none() && !app_state.panel_busy(is_left) {
            app_state.is_left_active = is_left;
            app_state.open_branches(is_left);
        }
        return;
    }

    // A panel's path, in the row above it, is edited where it is as Ctrl+L
    // edits it; the panel becomes the active one. The split between the two
    // is where the right panel starts.
    if app_state.path_bar_area.contains(Position::new(column, row)) {
        let is_left = column < app_state.table_area_right.x;
        let editing = matches!(app_state.dialog, Some(Dialog::Prompt(PromptKind::GoTo, _)));
        if app_state.job.is_none() && !app_state.panel_busy(is_left) && !(editing && is_left == app_state.is_left_active) {
            app_state.is_left_active = is_left;
            app_state.open_go_to();
        }
        return;
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

    // Hit-test the panels the last frame actually drew, rather than deriving
    // their position from the layout constants a second time.
    let position = Position::new(column, row);
    let clicked_left = app_state.table_area_left.contains(position);
    if !clicked_left && !app_state.table_area_right.contains(position) {
        return;
    }
    let area = if clicked_left { app_state.table_area_left } else { app_state.table_area_right };
    // A click on a panel still being read makes it the active one, and does
    // nothing to rows about to be replaced.
    if app_state.panel_busy(clicked_left) {
        app_state.is_left_active = clicked_left;
        return;
    }

    // The table draws its header on the first row of its area, and a click
    // on a heading sorts by it.
    if row <= area.y {
        if let Some(&(_, key)) = app_state.sort_headers.iter().find(|(cell, _)| cell.contains(position)) {
            app_state.sort_panels_by(key);
        }
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
        let state = if clicked_left { &mut app_state.state_left } else { &mut app_state.state_right };
        state.select(Some(actual_index));

        // Double-click on directory: enter it
        if is_double_click {
            let children = if clicked_left { &app_state.children_left } else { &app_state.children_right };
            if actual_index < children.len() {
                if children[actual_index].is_dir {
                    enter_directory_panel(app_state);
                } else {
                    let dir = if clicked_left { &app_state.dir_left } else { &app_state.dir_right };
                    let file_path = children[actual_index].path_in(dir);
                    open_file(app_state, file_path);
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

    if let Screen::Viewer(state) = &mut app_state.screen {
        // The binary notice has nothing to select.
        if state.from_edit {
            return;
        }

        // Through the rows, so a click on a wrapped line lands in it.
        let position = state.position_at(state.scroll_offset + (row - area.y) as usize, (column - area.x) as usize);

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

    if let Screen::Editor(state) = &mut app_state.screen {
        let visual_row = (row - area.y) as usize;
        let target_line = (state.scroll_offset + visual_row).min(state.lines.len().saturating_sub(1));

        // Map the visual column back to a character index, past the horizontal
        // scroll, any expanded tabs and the second column of a wide character.
        let visual_col = (column - area.x) as usize + state.horizontal_offset;
        let line = &state.lines[target_line];
        let mut char_col = 0;
        let mut current_visual = 0;
        for character in line.chars() {
            if current_visual >= visual_col {
                break;
            }
            current_visual += crate::utils::char_display_width(character);
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
