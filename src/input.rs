use crate::app::{Answer, AppState, Dialog, OverwritePrompt, PromptKind, Screen, TextInput, TransferKind};
use crate::strip::StripHit;
use crate::options::OnExisting;
use crate::fs_ops::{check_destinations, create_directory, create_file, is_plain_name, is_same_entry, path_exists, rename_path};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::display::tab_width;
use ratatui::layout::Position;
use ratatui::widgets::TableState;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn handle_input(app_state: &mut AppState) -> Result<bool> {
    if !event::poll(Duration::from_millis(100))? {
        return Ok(true);
    }
    match event::read()? {
        // Windows reports a press and a release for every key. Acting on the
        // release too would run each action twice, and toggles would cancel
        // themselves out. Repeat is kept so held keys still work.
        Event::Key(key) if key.kind != KeyEventKind::Release => return Ok(handle_key(app_state, key)),
        Event::Paste(text) => handle_paste(app_state, &text),
        // Nothing behind a popup responds to the mouse - not clicks and not
        // the wheel, which would otherwise scroll a panel out of sight of the
        // dialog asking about it.
        Event::Mouse(mouse_event) if !app_state.popup_is_open() => handle_mouse(app_state, mouse_event),
        _ => {}
    }
    Ok(true)
}

/// A key, to whatever is on top: a running job, then an error, then the
/// dialog that is open, then the screen. Each answers only its own keys, so
/// nothing a key is not meant for can act on it. False to quit.
fn handle_key(app_state: &mut AppState, key: KeyEvent) -> bool {
    // What the last search came to stays up until the next key.
    app_state.find_note = None;

    // A character carrying Ctrl or Alt is a chord, not text. Only the chords
    // bound are acted on, and the rest are dropped instead of falling through
    // as typed characters - they would otherwise land in the file being
    // edited, in a filename, or on a yes/no prompt. AltGr reports as Ctrl+Alt
    // on Windows and does produce text (@, EUR, ...), so a chord is a lone
    // Ctrl or a lone Alt.
    if let KeyCode::Char(c) = key.code
        && key.modifiers.contains(KeyModifiers::CONTROL) != key.modifiers.contains(KeyModifiers::ALT)
    {
        handle_chord(app_state, c, key.modifiers);
        return true;
    }

    if app_state.job.is_some() {
        return job_key(app_state, key);
    }
    // F10 means the same over everything else.
    if key.code == KeyCode::F(10) {
        return !request_quit(app_state);
    }
    if app_state.error.is_some() {
        // An error covers whatever is behind it - a panel, the viewer, the
        // editor with a save that failed, the options with one that did.
        // Esc or Enter puts it away and nothing else gets past it: Space or a
        // letter would otherwise select or search in a panel that cannot be
        // seen.
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            app_state.reset_error();
        }
        return true;
    }
    if app_state.dialog.is_some() {
        return dialog_key(app_state, key);
    }
    match app_state.screen {
        Screen::Panels => panel_key(app_state, key),
        Screen::Viewer(_) => viewer_key(app_state, key),
        Screen::Editor(_) => editor_key(app_state, key),
    }
    true
}

/// Ctrl or Alt with a character.
fn handle_chord(app_state: &mut AppState, c: char, modifiers: KeyModifiers) {
    // Not behind a dialog or an error: whatever is covered could not show
    // what the key did. Nor in a panel during a job, about to be reread.
    let clear = app_state.dialog.is_none() && app_state.error.is_none();
    let in_editor = clear && app_state.is_editing();
    let in_viewer = clear && app_state.is_viewing();
    let in_panel = clear
        && app_state.job.is_none()
        && matches!(app_state.screen, Screen::Panels)
        && !app_state.panel_busy(app_state.is_left_active);

    // Alt+* inverts the directories as well as the files. Ctrl is an alias
    // for the terminals that can report it; most send Ctrl+* as a bare * or
    // not at all, while Alt arrives anywhere.
    if c == '*' && in_panel {
        app_state.invert_selection(true);
    } else if modifiers.contains(KeyModifiers::ALT) {
        // Alt+1 to Alt+8 go to that tab, and Alt+9 to the last, however
        // many there are - as browsers have it.
        if in_panel && let Some(digit) = c.to_digit(10).filter(|&digit| digit > 0) {
            let is_left = app_state.is_left_active;
            let count = if is_left { app_state.tabs_left.list.len() } else { app_state.tabs_right.list.len() };
            let index = if digit == 9 { count - 1 } else { digit as usize - 1 };
            app_state.select_tab(is_left, index);
        }
    } else {
        match c {
            'f' if in_editor || in_viewer => app_state.open_prompt(PromptKind::Find),
            'g' if in_editor || in_viewer => app_state.open_prompt(PromptKind::GoToLine),
            's' if in_editor => {
                if let Err(e) = app_state.editor_save() {
                    app_state.display_error(e);
                }
            }
            'a' if in_editor => app_state.editor_select_all(),
            // Ctrl+Z undoes; Ctrl+Shift+Z and Ctrl+Y put it back.
            'z' if in_editor && !modifiers.contains(KeyModifiers::SHIFT) => app_state.editor_undo(),
            'z' | 'Z' if in_editor => app_state.editor_redo(),
            'y' | 'Y' if in_editor => app_state.editor_redo(),
            'c' if in_editor => app_state.editor_copy(),
            'c' if app_state.is_viewing() => app_state.viewer_copy(),
            'x' if in_editor => app_state.editor_cut(),
            'v' if in_editor => app_state.editor_paste(),
            // Ctrl+R rereads both panels from disk.
            'r' if !app_state.is_modal_open() => {
                app_state.reload_panel(true, None);
                app_state.reload_panel(false, None);
            }
            't' if in_panel => app_state.new_tab(),
            'w' if in_panel => {
                let is_left = app_state.is_left_active;
                let active = if is_left { app_state.tabs_left.active } else { app_state.tabs_right.active };
                app_state.close_tab(is_left, active);
            }
            _ => {}
        }
    }
}

/// While a copy, move or delete runs. False to quit.
fn job_key(app_state: &mut AppState, key: KeyEvent) -> bool {
    let Some(job) = app_state.job.as_mut() else {
        return true;
    };
    match key.code {
        // An entry that failed, waiting on what to do about it.
        KeyCode::Char('r') | KeyCode::Char('R') if job.problem.is_some() => job.answer(Answer::Retry),
        KeyCode::Char('s') | KeyCode::Char('S') if job.problem.is_some() => job.answer(Answer::Skip),
        KeyCode::Char('a') | KeyCode::Char('A') if job.problem.is_some() => job.answer(Answer::SkipAll),
        // Offered only for what the trash would not take.
        KeyCode::Char('d') | KeyCode::Char('D') if job.problem.is_some() && job.kind == TransferKind::Trash => {
            job.answer(Answer::Delete)
        }
        KeyCode::Esc => app_state.cancel_transfer(),
        // The second F10 leaves, abandoning the job where it stands. Asked
        // for twice because it is the way out of a transfer stuck in a write
        // to a disk that has stopped answering, where a cancel is never
        // noticed - and because what is part way through writing is left
        // behind. The first press only offers it: the job runs on untouched,
        // and Esc is still the way to stop it.
        KeyCode::F(10) if app_state.quit_armed => return false,
        KeyCode::F(10) => app_state.quit_armed = true,
        // Nothing else means anything while a job runs: it would act on
        // panels about to be reread.
        _ => {}
    }
    true
}

/// A key for the dialog that is open. False to quit.
fn dialog_key(app_state: &mut AppState, key: KeyEvent) -> bool {
    let code = key.code;
    let yes = matches!(code, KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y'));
    let no = matches!(code, KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N'));

    match app_state.dialog.as_ref() {
        None => {}
        Some(Dialog::Help) => {
            if matches!(code, KeyCode::Esc | KeyCode::F(1)) {
                app_state.dialog = None;
            }
        }
        Some(Dialog::Options { editing: Some(_) }) => match code {
            KeyCode::Esc => app_state.options_cancel_edit(),
            KeyCode::Enter => app_state.options_commit_edit(),
            _ => edit_text(app_state, code),
        },
        Some(Dialog::Options { editing: None }) => match code {
            KeyCode::Esc | KeyCode::F(11) => app_state.dialog = None,
            KeyCode::Up => app_state.options_move(false),
            KeyCode::Down => app_state.options_move(true),
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => app_state.options_change(true),
            KeyCode::Left => app_state.options_change(false),
            _ => {}
        },
        Some(Dialog::Rename(_)) => match code {
            KeyCode::Esc | KeyCode::F(2) => app_state.dialog = None,
            KeyCode::Enter => handle_rename(app_state),
            _ => edit_text(app_state, code),
        },
        Some(Dialog::Create { .. }) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::F(7) => toggle_create(app_state, true),
            KeyCode::F(4) if key.modifiers.contains(KeyModifiers::SHIFT) => toggle_create(app_state, false),
            KeyCode::Enter => handle_create_confirm(app_state),
            _ => edit_text(app_state, code),
        },
        Some(Dialog::Delete { .. }) if yes => handle_delete_confirm(app_state),
        Some(Dialog::Transfer { .. }) if yes => handle_transfer_confirm(app_state),
        Some(Dialog::Overwrite(_)) if yes || no => answer_overwrite(app_state, yes),
        Some(Dialog::LargeFile(_)) if yes => app_state.confirm_large_file(),
        Some(Dialog::Delete { .. } | Dialog::Transfer { .. } | Dialog::LargeFile(_)) if no => app_state.dialog = None,
        Some(&Dialog::SaveChanges { then_quit }) => match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Save and close - unless the save fails, in which case
                // closing would throw the edits away. The editor stays, with
                // the error over it, and a quit that asked first goes ahead
                // only once the edits are safe.
                app_state.dialog = None;
                match app_state.editor_save() {
                    Ok(()) => {
                        app_state.close_editor();
                        return !then_quit;
                    }
                    Err(e) => app_state.display_error(e),
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // Discard and close, and leave if that is what F10 asked.
                app_state.dialog = None;
                app_state.close_editor();
                return !then_quit;
            }
            // Back to the editor - and stay, if F10 asked.
            KeyCode::Esc => app_state.dialog = None,
            _ => {}
        },
        Some(Dialog::DrivePicker { .. }) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.confirm_drive_picker(),
            KeyCode::F(1) | KeyCode::F(2) | KeyCode::Right | KeyCode::Down => app_state.move_drive_picker(true),
            KeyCode::Left | KeyCode::Up => app_state.move_drive_picker(false),
            _ => {}
        },
        // Find or go to line, at the foot of the viewer or editor, or a
        // pattern to select by, at the foot of the panels.
        Some(&Dialog::Prompt(kind, _)) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.confirm_prompt(),
            KeyCode::Char(c) if kind == PromptKind::GoToLine && !c.is_ascii_digit() => {}
            _ => edit_text(app_state, code),
        },
        Some(_) => {}
    }
    true
}

/// The keys that edit a line being typed into a dialog: a character,
/// Backspace and Delete, and moving along it.
fn edit_text(app_state: &mut AppState, code: KeyCode) {
    let Some(input) = app_state.dialog_input() else {
        return;
    };
    match code {
        KeyCode::Char(c) => input.insert(c),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete_forward(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.move_home(),
        KeyCode::End => input.move_end(),
        _ => {}
    }
}

fn viewer_key(app_state: &mut AppState, key: KeyEvent) {
    match key.code {
        // Esc is the one way out, of the viewer and of the notice F4 raises
        // on a binary alike.
        KeyCode::Esc => app_state.close_viewer(),
        // F3 and Shift+F3 step through the matches, as in most editors - so
        // F3 here cannot also close the viewer.
        KeyCode::F(3) => app_state.find(!key.modifiers.contains(KeyModifiers::SHIFT), false),
        KeyCode::Char('x') | KeyCode::Char('X') => app_state.viewer_next_mode(),
        KeyCode::Char('f') | KeyCode::Char('F') => app_state.viewer_toggle_fill(),
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

fn editor_key(app_state: &mut AppState, key: KeyEvent) {
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
            if let Err(e) = app_state.editor_save() {
                app_state.display_error(e);
            }
        }
        KeyCode::F(3) => app_state.find(!extend, false),
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
}

fn panel_key(app_state: &mut AppState, key: KeyEvent) {
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
        KeyCode::F(1) => app_state.dialog = Some(Dialog::Help),
        KeyCode::F(2) => open_rename(app_state),
        KeyCode::F(3) => handle_f3_view(app_state),
        // Shift+F4 creates an empty file, as MC does.
        KeyCode::F(4) if key.modifiers.contains(KeyModifiers::SHIFT) => toggle_create(app_state, false),
        KeyCode::F(4) => handle_f4_edit(app_state),
        KeyCode::F(5) => open_transfer(app_state, true),
        KeyCode::F(6) => open_transfer(app_state, false),
        KeyCode::F(7) => toggle_create(app_state, true),
        // Shift deletes for good, whatever F11 says F8 does.
        KeyCode::F(8) | KeyCode::Delete => open_delete(app_state, key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::F(9) => open_terminal(app_state),
        KeyCode::F(11) => app_state.dialog = Some(Dialog::Options { editing: None }),
        KeyCode::F(12) => app_state.show_preview = !app_state.show_preview,
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
        KeyCode::Enter => enter_directory_panel(app_state),
        _ => {}
    }
}

/// Bracketed paste: the terminal hands over the system clipboard as one
/// event instead of a burst of keystrokes.
fn handle_paste(app_state: &mut AppState, text: &str) {
    // Nothing behind an error takes it.
    if app_state.error.is_some() {
        return;
    }
    if let Some(input) = app_state.dialog_input() {
        // One line: a pasted newline would end it, so none go in.
        for character in text.chars().filter(|character| !character.is_control()) {
            input.insert(character);
        }
    } else if app_state.dialog.is_none() && app_state.is_editing() {
        app_state.editor_insert_text(text);
    }
}

fn handle_mouse(app_state: &mut AppState, mouse_event: MouseEvent) {
    let (column, row) = (mouse_event.column, mouse_event.row);
    let (viewing, editing) = (app_state.is_viewing(), app_state.is_editing());
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
}

/// F10: true to leave now. Unsaved edits in the editor are asked about
/// first, whatever else is on screen - a find prompt or an error over the
/// editor included - and the answer to that question finishes the quit.
fn request_quit(app_state: &mut AppState) -> bool {
    if !app_state.editor_is_modified() {
        return true;
    }
    app_state.reset_error();
    app_state.dialog = Some(Dialog::SaveChanges { then_quit: true });
    false
}

/// F2 on the entry under the cursor, typed into in place of its name.
fn open_rename(app_state: &mut AppState) {
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let Some(item) = state.selected().and_then(|index| children.get(index)) else {
        return;
    };

    // Don't rename the parent entry - ".." resolves to the parent directory itself.
    if item.name == ".." {
        return;
    }

    let mut input = TextInput::new();
    input.set(item.name_full.clone());
    app_state.dialog = Some(Dialog::Rename(input));
}

/// One dialog serves both: F7 makes a directory, Shift+F4 an empty file.
fn toggle_create(app_state: &mut AppState, is_dir: bool) {
    // The same key closes it again; the other one switches what is being made.
    if matches!(app_state.dialog, Some(Dialog::Create { is_dir: making, .. }) if making == is_dir) {
        app_state.dialog = None;
        return;
    }
    app_state.dialog = Some(Dialog::Create { is_dir, input: TextInput::new() });
}

fn handle_rename(app_state: &mut AppState) {
    let Some(Dialog::Rename(input)) = app_state.dialog.take() else {
        return;
    };
    let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let Some(item) = state.selected().and_then(|index| children.get(index)) else {
        return;
    };

    // ".." resolves to the parent directory - never rename through it.
    if item.name == ".." {
        return;
    }

    let new_name = input.text;
    // The field opened on the name as shown, which for a name that is not
    // valid UTF-8 is not the name itself. Left as it was, it would rename
    // the file to its own lossy reading.
    // Nothing to rename to: treat it as a cancel, the way F7 treats an empty
    // name. Composing it would point at the parent directory instead.
    if new_name == item.name_full || new_name.is_empty() {
        return;
    }
    // A rename stays in its directory; moving is F6.
    if !is_plain_name(&new_name) {
        app_state.display_error(format!("Not a plain file name: {}", new_name));
        return;
    }

    let original_path = item.path_in(parent_path);
    let new_path = parent_path.join(&new_name);

    // rename() replaces the destination without a word. Equal paths mean the
    // name was left alone, which is a no-op rather than a collision - and so
    // does the same entry under another case, which on a case-insensitive
    // filesystem is how "readme" becomes "README".
    if new_path != original_path && path_exists(&new_path) && !is_same_entry(&original_path, &new_path) {
        app_state.display_error(format!("Already exists: {}", new_name));
        return;
    }

    match rename_path(original_path.clone(), new_path.clone()) {
        Ok(_) => {
            // A directory's size is kept by its path, which just changed.
            if let Some(size) = app_state.dir_sizes.remove(&original_path) {
                app_state.dir_sizes.insert(new_path, size);
            }
            app_state.reload_panel(app_state.is_left_active, Some(new_name.as_ref()));
        }
        Err(e) => app_state.display_error(e.to_string()),
    }
}

/// Move the active panel's cursor - not over one still being read, whose
/// rows are about to be replaced.
fn move_cursor(app_state: &mut AppState, move_fn: impl Fn(&mut TableState, usize)) {
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
        let target = item.path_in(dir);
        app_state.open_dir(is_left, target, None);
        return;
    }

    let file_path = item.path_in(dir);
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
    Command::new("cmd").args(["/C", "start", ""]).arg(path)
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

/// What F5, F6 and F8 work on in the active panel: what is selected, or the
/// entry under the cursor when nothing is. Never the parent entry.
fn chosen_items(app_state: &AppState) -> Vec<(PathBuf, bool)> {
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
    chosen.into_iter().filter(|item| item.name != "..").map(|item| (item.path_in(dir), item.is_dir)).collect()
}

/// F8 or Delete, to the trash unless F11 says otherwise, and Shift+F8 or
/// Shift+Delete for good. That one always asks first, whatever Confirm delete
/// is set to: there is no getting it back.
fn open_delete(app_state: &mut AppState, permanently: bool) {
    let items = chosen_items(app_state);
    if items.is_empty() {
        return;
    }
    let to_trash = app_state.options.delete_to_trash && !permanently;
    app_state.dialog = Some(Dialog::Delete { items, to_trash });
    if !app_state.options.confirm_delete && !permanently {
        handle_delete_confirm(app_state);
    }
}

fn handle_delete_confirm(app_state: &mut AppState) {
    // The removals, the panel reload and the selections are all handled by the
    // job as it finishes, the same as a copy or a move.
    if let Some(Dialog::Delete { items, to_trash }) = app_state.dialog.take() {
        if to_trash {
            app_state.start_trash(items);
        } else {
            app_state.start_delete(items);
        }
    }
}

fn handle_create_confirm(app_state: &mut AppState) {
    let Some(Dialog::Create { is_dir, input }) = app_state.dialog.take() else {
        return;
    };
    let name = input.text;
    if name.is_empty() {
        return;
    }

    // Made in the directory on show, not in one named by the path typed.
    if !is_plain_name(&name) {
        app_state.display_error(format!("Not a plain file name: {}", name));
        return;
    }

    let parent_path = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let path = parent_path.join(&name);
    let result = if is_dir { create_directory(path) } else { create_file(path) };
    match result {
        Ok(_) => app_state.reload_panel(app_state.is_left_active, Some(name.as_ref())),
        Err(e) => app_state.display_error(e.to_string()),
    }
}

fn handle_f3_view(app_state: &mut AppState) {
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
        file_path.push(&item.name_os);

        // Open viewer
        app_state.request_open(file_path, false);
    }
}

fn handle_f4_edit(app_state: &mut AppState) {
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
        file_path.push(&item.name_os);

        // An external editor, when F11 names one; otherwise the built-in.
        if app_state.options.editor.trim().is_empty() {
            app_state.request_open(file_path, true);
        } else {
            app_state.external_edit = Some(file_path);
        }
    }
}

fn open_terminal(app_state: &mut AppState) {
    let dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let command = app_state.options.terminal.trim();
    let result = if command.is_empty() { spawn_detached_terminal(dir) } else { spawn_configured_terminal(command, dir) };
    if let Err(e) = result {
        app_state.display_error(format!("Cannot open terminal: {}", e));
    }
}

/// The terminal named in F11. Split on whitespace, with `{}` standing for the
/// directory in case the terminal wants it as an argument; it is started in
/// the directory either way.
fn spawn_configured_terminal(command: &str, dir: &std::path::Path) -> std::io::Result<()> {
    let mut parts = command.split_whitespace().map(|part| crate::utils::substitute(part, dir.as_os_str()));
    let program = parts.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Empty terminal command"))?;
    let mut process = Command::new(program);
    process.args(parts).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut process);
    process.spawn()?;
    Ok(())
}

/// Keep a started terminal alive after fm84 exits, and out of its signals.
#[cfg(unix)]
fn detach(process: &mut Command) {
    use std::os::unix::process::CommandExt;
    process.process_group(0);
}

#[cfg(windows)]
fn detach(process: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    process.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_process: &mut Command) {}

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

/// F5 or F6: copy or move what is chosen to the other panel, once confirmed.
fn open_transfer(app_state: &mut AppState, is_copy: bool) {
    let dest_dir = if app_state.is_left_active { &app_state.dir_right } else { &app_state.dir_left };
    let items: Vec<(PathBuf, PathBuf, bool)> = chosen_items(app_state)
        .into_iter()
        .map(|(source, is_dir)| {
            let dest = dest_dir.join(source.file_name().unwrap_or_default());
            (source, dest, is_dir)
        })
        .collect();
    if items.is_empty() {
        return;
    }
    app_state.dialog = Some(Dialog::Transfer { items, is_copy });
    if !app_state.options.confirm_copy_move {
        handle_transfer_confirm(app_state);
    }
}

fn handle_transfer_confirm(app_state: &mut AppState) {
    if let Some(Dialog::Transfer { items, is_copy }) = app_state.dialog.take() {
        submit_transfer(app_state, items, is_copy);
    }
}

/// Start a confirmed copy or move, once what it would write over is settled.
/// Every destination is checked before anything is written: bailing out
/// partway would leave some items done and the rest not. Names already taken
/// go by F11's "When destination exists" - ask, overwrite, or refuse.
fn submit_transfer(app_state: &mut AppState, items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool) {
    let taken = match check_destinations(&items) {
        Ok(taken) => taken,
        Err(message) => {
            app_state.display_error(message);
            return;
        }
    };
    if taken.is_empty() {
        // The work itself, the panel reload and the selections are all
        // handled by the job as it finishes.
        app_state.start_transfer(items, is_copy, false);
        return;
    }
    match app_state.options.on_existing {
        OnExisting::Overwrite => app_state.start_transfer(items, is_copy, true),
        OnExisting::Refuse => app_state.display_error(format!("Destination already exists: {}", taken[0].display())),
        OnExisting::Ask => app_state.dialog = Some(Dialog::Overwrite(OverwritePrompt { items, is_copy, taken })),
    }
}

/// The answer to the overwrite question: yes starts the transfer, writing
/// over what is there; no drops it.
fn answer_overwrite(app_state: &mut AppState, overwrite: bool) {
    if let Some(Dialog::Overwrite(prompt)) = app_state.dialog.take()
        && overwrite
    {
        app_state.start_transfer(prompt.items, prompt.is_copy, true);
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
                    let file_path = children[actual_index].path_in(dir);
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

    if let Screen::Viewer(state) = &mut app_state.screen {
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

    if let Screen::Editor(state) = &mut app_state.screen {
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
            current_visual += if character == '\t' { tab_width() } else { 1 };
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

#[cfg(test)]
mod tests {
    use super::{handle_key, request_quit};
    use crate::app::{AppState, Dialog, Screen};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn press(app_state: &mut AppState, code: KeyCode) -> bool {
        handle_key(app_state, KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// Keys go to whatever is on top - an error, then a dialog, then the
    /// screen - and to nothing underneath it.
    #[test]
    fn a_key_reaches_only_what_is_on_top() {
        let dir = std::env::temp_dir().join(format!("fm84-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "text").unwrap();
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), Some("a.txt".as_ref()));

        // Space behind an error selects nothing, and Enter puts the error away.
        app_state.display_error("in the way".to_string());
        assert!(press(&mut app_state, KeyCode::Char(' ')));
        assert!(app_state.selected_left.is_empty() && app_state.error.is_some());
        press(&mut app_state, KeyCode::Enter);
        assert!(app_state.error.is_none());

        // F1 opens help over the panels; typing there searches nothing.
        press(&mut app_state, KeyCode::F(1));
        assert!(matches!(app_state.dialog, Some(Dialog::Help)));
        press(&mut app_state, KeyCode::Char('a'));
        assert!(app_state.search_input.is_empty());
        press(&mut app_state, KeyCode::F(1));
        assert!(app_state.dialog.is_none());

        // F2 types into the rename field, and Esc leaves the name alone.
        press(&mut app_state, KeyCode::F(2));
        press(&mut app_state, KeyCode::Char('x'));
        assert_eq!(app_state.dialog_input().unwrap().text, "a.txtx");
        press(&mut app_state, KeyCode::Esc);
        assert!(app_state.dialog.is_none() && dir.join("a.txt").exists());

        // In the editor, Esc over edits asks; Esc again goes back to them.
        press(&mut app_state, KeyCode::F(4));
        assert!(matches!(app_state.screen, Screen::Editor(_)));
        press(&mut app_state, KeyCode::Char('!'));
        press(&mut app_state, KeyCode::Esc);
        assert!(matches!(app_state.dialog, Some(Dialog::SaveChanges { then_quit: false })));
        press(&mut app_state, KeyCode::Esc);
        assert!(app_state.dialog.is_none() && app_state.is_editing());
        // F10 asks too, and Discard then leaves.
        assert!(press(&mut app_state, KeyCode::F(10)));
        assert!(!press(&mut app_state, KeyCode::Char('n')));
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "text");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// F10 leaves at once, unless the editor holds edits nobody has saved:
    /// then the save prompt comes up first, over anything else in the way.
    #[test]
    fn quitting_asks_about_unsaved_edits_first() {
        let path = std::env::temp_dir().join(format!("fm84-quit-{}.txt", std::process::id()));
        std::fs::write(&path, "text").unwrap();
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        assert!(request_quit(&mut app_state));

        app_state.open_editor(path.clone()).unwrap();
        assert!(request_quit(&mut app_state), "nothing to lose yet");

        app_state.editor_insert_char('x');
        app_state.open_prompt(crate::app::PromptKind::Find);
        app_state.display_error("in the way".to_string());
        assert!(!request_quit(&mut app_state));
        // The save prompt takes the find prompt's place, and the error goes.
        assert!(matches!(app_state.dialog, Some(Dialog::SaveChanges { then_quit: true })));
        assert!(app_state.error.is_none());
        std::fs::remove_file(path).unwrap();
    }
}
