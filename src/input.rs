use crate::app::{AfterSave, Answer, AppState, Clashes, Dialog, FindFiles, PromptKind, Saved, Screen, TextInput, TransferKind};
use crate::display::tab_width;
use crate::fs_ops::{check_destinations, create_directory, create_file, is_plain_name, is_same_entry, path_exists, rename_path, transfer_targets, typed_path};
use crate::options::{OnExisting, SortKey};
use crate::strip::StripHit;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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
        Event::Mouse(mouse_event) if !app_state.popup_is_open() => return Ok(handle_mouse(app_state, mouse_event)),
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
    let in_panel = clear && app_state.job.is_none() && matches!(app_state.screen, Screen::Panels) && !app_state.panel_busy(app_state.is_left_active);

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
        // Alt+C copies the names of what Ctrl+C would copy the paths of.
        if in_panel && c == 'c' {
            copy_names(app_state, false);
        }
        // Alt+I sends the other panel to this one's directory.
        if in_panel && c == 'i' {
            app_state.same_dir_in_other_panel();
        }
    } else {
        match c {
            'f' if in_editor || in_viewer => app_state.open_prompt(PromptKind::Find),
            'g' if in_editor || in_viewer => app_state.open_prompt(PromptKind::GoToLine),
            // Replace, in the editor; elsewhere Ctrl+R rereads the panels.
            'r' if in_editor => app_state.open_replace(),
            's' if in_editor => {
                save_then(app_state, AfterSave::Stay);
            }
            'a' if in_editor => app_state.editor_select_all(),
            // Ctrl+Z undoes; Ctrl+Shift+Z and Ctrl+Y put it back.
            'z' if in_editor && !modifiers.contains(KeyModifiers::SHIFT) => app_state.editor_undo(),
            'z' | 'Z' if in_editor => app_state.editor_redo(),
            'y' | 'Y' if in_editor => app_state.editor_redo(),
            'c' if in_editor => app_state.editor_copy(),
            'c' if app_state.is_viewing() => app_state.viewer_copy(),
            'c' if in_panel => copy_names(app_state, true),
            'x' if in_editor => app_state.editor_cut(),
            'v' if in_editor => app_state.editor_paste(),
            // Ctrl+R rereads both panels from disk.
            'r' if !app_state.is_modal_open() => {
                app_state.reload_panel(true, None);
                app_state.reload_panel(false, None);
            }
            // Ctrl+L, as a browser's address bar.
            'l' if in_panel => app_state.open_go_to(),
            // Ctrl+D, Total Commander's directory hotlist; again closes it.
            'd' if in_panel => app_state.open_bookmarks(),
            'd' if app_state.error.is_none() && matches!(app_state.dialog, Some(Dialog::Bookmarks { .. })) => app_state.dialog = None,
            // Ctrl+U swaps the panels, as in Midnight Commander.
            'u' if in_panel => app_state.swap_panels(),
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
    let clash = job.clash.is_some();
    match key.code {
        // A file in the way, waiting on whether to write over it. Enter is
        // not one of the answers: a key pressed out of habit should not be
        // the one that writes over a file.
        KeyCode::Char('y') | KeyCode::Char('Y') if clash => job.answer(Answer::Overwrite),
        KeyCode::Char('n') | KeyCode::Char('N') if clash => job.answer(Answer::Skip),
        KeyCode::Char('a') | KeyCode::Char('A') if clash => job.answer(Answer::OverwriteAll),
        KeyCode::Char('s') | KeyCode::Char('S') if clash => job.answer(Answer::SkipAll),
        KeyCode::Char('o') | KeyCode::Char('O') if clash => job.answer(Answer::OverwriteOlder),
        // An entry that failed, waiting on what to do about it.
        KeyCode::Char('r') | KeyCode::Char('R') if job.problem.is_some() => job.answer(Answer::Retry),
        KeyCode::Char('s') | KeyCode::Char('S') if job.problem.is_some() => job.answer(Answer::Skip),
        KeyCode::Char('a') | KeyCode::Char('A') if job.problem.is_some() => job.answer(Answer::SkipAll),
        // Offered only for what the trash would not take.
        KeyCode::Char('d') | KeyCode::Char('D') if job.problem.is_some() && job.kind == TransferKind::Trash => job.answer(Answer::Delete),
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
    let finding = match &app_state.dialog {
        Some(Dialog::FindFiles(find)) => Some(matches!(**find, FindFiles::Asking { .. })),
        _ => None,
    };
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
        // Y and N type into the destination; only Enter and Esc answer it.
        Some(Dialog::Transfer { .. }) => match code {
            KeyCode::Enter => handle_transfer_confirm(app_state),
            KeyCode::Esc => app_state.dialog = None,
            _ => edit_text(app_state, code),
        },
        Some(Dialog::LargeFile(_)) if yes => app_state.confirm_large_file(),
        Some(Dialog::Delete { .. } | Dialog::LargeFile(_)) if no => app_state.dialog = None,
        Some(&Dialog::SaveChanges { then_quit }) => match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                // Save and close - unless the save fails, in which case
                // closing would throw the edits away. The editor stays, with
                // the error over it, and a quit that asked first goes ahead
                // only once the edits are safe.
                app_state.dialog = None;
                return save_then(app_state, if then_quit { AfterSave::Quit } else { AfterSave::Close });
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
        // A match asked about, while replacing. Enter is not an answer: a key
        // pressed out of habit should not be the one that changes the text.
        Some(Dialog::Replacing { replaced, .. }) => {
            let replaced = *replaced;
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app_state.replace_step(true);
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    app_state.replace_step(false);
                }
                KeyCode::Char('a') | KeyCode::Char('A') => app_state.replace_all(),
                KeyCode::Esc => app_state.finish_replace(replaced),
                _ => {}
            }
        }
        // Changed on disk since the editor read it: write over it anyway,
        // read it again, or go back to the edits.
        Some(&Dialog::DiskChanged { then, now, .. }) => match code {
            KeyCode::Char('o') | KeyCode::Char('O') => {
                app_state.dialog = None;
                match app_state.editor_save_over() {
                    Ok(()) => return after_save(app_state, then),
                    Err(e) => app_state.display_error(e),
                }
            }
            // Only while there is a file to read.
            KeyCode::Char('r') | KeyCode::Char('R') if now.is_some() => {
                app_state.dialog = None;
                if let Err(e) = app_state.editor_reload() {
                    app_state.display_error(e);
                }
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => app_state.dialog = None,
            _ => {}
        },
        Some(Dialog::Bookmarks { .. }) => {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match code {
                KeyCode::Esc => app_state.dialog = None,
                KeyCode::Enter => app_state.go_to_bookmark(),
                KeyCode::Up if shift => app_state.shift_bookmark(true),
                KeyCode::Down if shift => app_state.shift_bookmark(false),
                KeyCode::Up => app_state.move_bookmark_cursor(-1),
                KeyCode::Down => app_state.move_bookmark_cursor(1),
                KeyCode::Home => app_state.move_bookmark_cursor(isize::MIN),
                KeyCode::End => app_state.move_bookmark_cursor(isize::MAX),
                KeyCode::Insert | KeyCode::Char('a') | KeyCode::Char('A') => app_state.add_bookmark(),
                KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('D') => app_state.remove_bookmark(),
                // 1 to 9 go straight to that one.
                KeyCode::Char(c @ '1'..='9') => {
                    let wanted = c as usize - '1' as usize;
                    if let Some(Dialog::Bookmarks { list, index }) = &mut app_state.dialog
                        && wanted < list.len()
                    {
                        *index = wanted;
                        app_state.go_to_bookmark();
                    }
                }
                _ => {}
            }
        }
        Some(Dialog::DrivePicker { .. }) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.confirm_drive_picker(),
            KeyCode::F(1) | KeyCode::F(2) | KeyCode::Right | KeyCode::Down => app_state.move_drive_picker(true),
            KeyCode::Left | KeyCode::Up => app_state.move_drive_picker(false),
            _ => {}
        },
        // Find files: first what to look for, then what was found. Esc closes
        // either, which stops a search still running.
        Some(Dialog::FindFiles(_)) if finding == Some(true) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.start_find_files(),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => app_state.find_files_switch_field(),
            _ => edit_text(app_state, code),
        },
        Some(Dialog::FindFiles(_)) => {
            let page = app_state.find_files_page.max(1) as isize;
            match code {
                KeyCode::Esc => app_state.dialog = None,
                KeyCode::Enter => app_state.find_files_go(),
                KeyCode::Up => app_state.find_files_move(-1),
                KeyCode::Down => app_state.find_files_move(1),
                KeyCode::PageUp => app_state.find_files_move(-page),
                KeyCode::PageDown => app_state.find_files_move(page),
                KeyCode::Home => app_state.find_files_move(isize::MIN),
                KeyCode::End => app_state.find_files_move(isize::MAX),
                _ => {}
            }
        }
        // Find or go to line, at the foot of the viewer or editor, or a
        // pattern to select by, at the foot of the panels.
        Some(&Dialog::Prompt(kind, _)) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.confirm_prompt(),
            KeyCode::Char(c) if kind == PromptKind::GoToLine && !c.is_ascii_digit() => {}
            KeyCode::Tab if kind == PromptKind::GoTo => app_state.complete_go_to(),
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
        // Ctrl+F3 to Ctrl+F6 sort by name, extension, date and size, as in
        // Total Commander; the same one again turns the order round.
        KeyCode::F(3) if control => app_state.sort_panels_by(SortKey::Name),
        KeyCode::F(4) if control => app_state.sort_panels_by(SortKey::Extension),
        KeyCode::F(5) if control => app_state.sort_panels_by(SortKey::Modified),
        KeyCode::F(6) if control => app_state.sort_panels_by(SortKey::Size),
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

/// False to quit, which F10 clicked on the key bar does.
fn handle_mouse(app_state: &mut AppState, mouse_event: MouseEvent) -> bool {
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
    // A label on the key bar is its key. Not under the rename field or a
    // prompt, which would take the key as one of theirs.
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left)
        && app_state.dialog.is_none()
        && let Some(&(_, key)) = app_state.fkey_hits.iter().find(|(cell, _)| cell.contains(Position::new(column, row)))
    {
        return handle_key(app_state, KeyEvent::new(KeyCode::F(key), KeyModifiers::NONE));
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

/// Save the file in the editor, then go on to `then`. When something else
/// has written the file since it was read, nothing is saved yet: the question
/// goes up instead, and its answer carries on to `then`. A save that fails
/// leaves the editor open, edits and all, with the error over it. False to
/// quit.
fn save_then(app_state: &mut AppState, then: AfterSave) -> bool {
    match app_state.editor_save() {
        Ok(Saved::Done) => after_save(app_state, then),
        Ok(Saved::ChangedOnDisk { opened, now }) => {
            app_state.dialog = Some(Dialog::DiskChanged { then, opened, now });
            true
        }
        Err(e) => {
            app_state.display_error(e);
            true
        }
    }
}

/// What a save goes on to once the file is written. False to quit.
fn after_save(app_state: &mut AppState, then: AfterSave) -> bool {
    match then {
        AfterSave::Stay => true,
        AfterSave::Close => {
            app_state.close_editor();
            true
        }
        AfterSave::Quit => {
            app_state.close_editor();
            false
        }
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
    Command::new("open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("cmd").args(["/C", "start", ""]).arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("xdg-open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
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

/// Ctrl+C copies the full paths of what is selected, or of the entry under
/// the cursor, to the clipboard, a line each; Alt+C the names alone. On the
/// parent entry, the panel's own directory. Names that are not valid UTF-8
/// go as near as they can be read: a clipboard holds text.
fn copy_names(app_state: &mut AppState, full: bool) {
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
    Command::new("open").arg("-a").arg("Terminal").arg(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    Command::new("cmd")
        .args(["/C", "start", "cmd"])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    if let Ok(term) = std::env::var("TERMINAL") {
        Command::new(&term).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
        return Ok(());
    }
    let emulators = ["xdg-terminal-emulator", "alacritty", "kitty", "foot", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"];
    for emu in &emulators {
        if Command::new(emu).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().is_ok() {
            return Ok(());
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No terminal emulator found. Set $TERMINAL."))
}

/// F5 or F6: copy or move what is chosen, to where the dialog is told. For a
/// single file it opens on that file's path in the other panel, ready to be
/// given another name; for anything else - several entries, or a directory -
/// on the other panel's directory. Enter alone does what it always did. With
/// Confirm copy and move off there is no dialog, and it goes straight there.
fn open_transfer(app_state: &mut AppState, is_copy: bool) {
    let sources = chosen_items(app_state);
    if sources.is_empty() {
        return;
    }
    // Where the other panel is going, should it be on its way somewhere.
    let other = app_state.panel_dir(!app_state.is_left_active);
    if !app_state.options.confirm_copy_move {
        go_transfer(app_state, &sources, &other, true, is_copy);
        return;
    }
    let (default, into_dir) = match &sources[..] {
        [(file, false)] => (other.join(file.file_name().unwrap_or_default()), false),
        _ => (other, true),
    };
    // A directory gets a separator on the end, which says it is one and
    // leaves the cursor where a name would be typed.
    let mut opened_with = default.to_string_lossy().into_owned();
    if into_dir && !opened_with.ends_with(std::path::is_separator) {
        opened_with.push(std::path::MAIN_SEPARATOR);
    }
    let mut input = TextInput::new();
    input.set(opened_with.clone());
    app_state.dialog = Some(Dialog::Transfer { sources, is_copy, input, opened_with, default });
}

/// Enter on the copy or move dialog.
fn handle_transfer_confirm(app_state: &mut AppState) {
    let Some(Dialog::Transfer { sources, is_copy, input, opened_with, default }) = app_state.dialog.take() else {
        return;
    };
    // As it opened: the file to exactly that path - so one in the way is
    // asked about, or refused if it is a directory, as F5 always did - and
    // anything else into that directory.
    if input.text == opened_with {
        match &sources[..] {
            [(file, false)] if !opened_with.ends_with(std::path::is_separator) => submit_transfer(app_state, vec![(file.clone(), default, false)], is_copy),
            _ => go_transfer(app_state, &sources, &default, true, is_copy),
        }
        return;
    }
    // Nothing to copy to: taken as a cancel, as an empty name is by F2 and F7.
    if input.text.trim().is_empty() {
        return;
    }
    let base = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let destination = typed_path(&input.text, base);
    go_transfer(app_state, &sources, &destination, input.text.ends_with(std::path::is_separator), is_copy);
}

/// Copy or move `sources` to `destination`, read as transfer_targets reads it.
fn go_transfer(app_state: &mut AppState, sources: &[(PathBuf, bool)], destination: &Path, names_dir: bool, is_copy: bool) {
    match transfer_targets(sources, destination, names_dir) {
        Ok(items) => submit_transfer(app_state, items, is_copy),
        Err(message) => app_state.display_error(message),
    }
}

/// Start a confirmed copy or move. Every destination is checked before
/// anything is written, for what no answer could make safe - an item onto
/// itself, a directory into itself, a file and a directory in each other's
/// place - since bailing out partway would leave some items done and the rest
/// not. Files already there go by F11's "When destination exists": asked
/// about one at a time as the job reaches them, written over, or - for Refuse
/// - the whole job refused before it starts.
fn submit_transfer(app_state: &mut AppState, items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool) {
    let taken = match check_destinations(&items) {
        Ok(taken) => taken,
        Err(message) => {
            app_state.display_error(message);
            return;
        }
    };
    // The work itself, the panel reload and the selections are all handled
    // by the job as it finishes.
    match app_state.options.on_existing {
        OnExisting::Refuse if !taken.is_empty() => app_state.display_error(format!("Destination already exists: {}", taken[0].display())),
        OnExisting::Overwrite => app_state.start_transfer(items, is_copy, Clashes::ReplaceAll),
        OnExisting::Ask | OnExisting::Refuse => app_state.start_transfer(items, is_copy, Clashes::Ask),
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
    use crate::app::{AppState, Dialog, FindFiles, PromptKind, Screen};
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

    /// Moving about the Options dialog and typing into a text row, put
    /// back with Esc. Nothing here changes a value, so nothing is saved over
    /// the real config.
    #[test]
    fn the_options_dialog_moves_and_edits_from_the_keys() {
        use crate::options::{OPTION_ROWS, OptionRow};
        let mut app_state = AppState::new();
        press(&mut app_state, KeyCode::F(11));
        assert!(matches!(app_state.dialog, Some(Dialog::Options { editing: None })));

        press(&mut app_state, KeyCode::Up);
        assert_eq!(app_state.options_cursor, 0);
        press(&mut app_state, KeyCode::Down);
        press(&mut app_state, KeyCode::Down);
        assert_eq!(app_state.options_cursor, 2);
        for _ in 0..OPTION_ROWS.len() {
            press(&mut app_state, KeyCode::Down);
        }
        assert_eq!(app_state.options_cursor, OPTION_ROWS.len() - 1);

        // A text row opens for typing, with what it holds; Esc puts it back.
        app_state.options_cursor = OPTION_ROWS.iter().position(|&row| row == OptionRow::Terminal).unwrap();
        let before = app_state.options.text(OptionRow::Terminal).to_string();
        press(&mut app_state, KeyCode::Enter);
        let Some(Dialog::Options { editing: Some(input) }) = &app_state.dialog else { panic!("not editing") };
        assert_eq!(input.text, before);
        press(&mut app_state, KeyCode::Char('x'));
        // Up and Down belong to the text while it is being typed into.
        let row = app_state.options_cursor;
        press(&mut app_state, KeyCode::Down);
        assert_eq!(app_state.options_cursor, row);
        press(&mut app_state, KeyCode::Esc);
        assert!(matches!(app_state.dialog, Some(Dialog::Options { editing: None })));
        assert_eq!(app_state.options.text(OptionRow::Terminal), before);

        press(&mut app_state, KeyCode::F(11));
        assert!(app_state.dialog.is_none());
        press(&mut app_state, KeyCode::F(11));
        press(&mut app_state, KeyCode::Esc);
        assert!(app_state.dialog.is_none());
    }

    /// Ctrl+D's list: A keeps the panel's directory once, Shift+arrows move
    /// one, D forgets it, and Enter or a number goes there.
    #[test]
    fn bookmarks_are_kept_moved_forgotten_and_gone_to() {
        let dir = std::env::temp_dir().join(format!("fm84-bookmarks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("one")).unwrap();
        std::fs::create_dir_all(dir.join("two")).unwrap();
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.join("one"), None);
        // Opened on an empty list rather than the bookmarks of whoever runs this.
        app_state.dialog = Some(Dialog::Bookmarks { list: Vec::new(), index: 0 });
        let list = |app_state: &AppState| match &app_state.dialog {
            Some(Dialog::Bookmarks { list, index }) => (list.clone(), *index),
            _ => panic!("not open"),
        };

        press(&mut app_state, KeyCode::Char('a'));
        press(&mut app_state, KeyCode::Char('a'));
        assert_eq!(list(&app_state), (vec![dir.join("one")], 0));
        app_state.dialog = None;
        app_state.open_dir(true, dir.join("two"), None);
        app_state.dialog = Some(Dialog::Bookmarks { list: vec![dir.join("one")], index: 0 });
        press(&mut app_state, KeyCode::Insert);
        assert_eq!(list(&app_state), (vec![dir.join("one"), dir.join("two")], 1));
        handle_key(&mut app_state, KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(list(&app_state), (vec![dir.join("two"), dir.join("one")], 0));

        // 2 goes to the second, and the dialog closes.
        press(&mut app_state, KeyCode::Char('2'));
        assert!(app_state.dialog.is_none());
        assert_eq!(app_state.dir_left, dir.join("one"));

        // Gone since: said so, and the panel stays put.
        app_state.dialog = Some(Dialog::Bookmarks {
            list: vec![dir.join("gone"), dir.join("two")],
            index: 0,
        });
        press(&mut app_state, KeyCode::Enter);
        assert!(app_state.error.is_some());
        assert_eq!(app_state.dir_left, dir.join("one"));
        press(&mut app_state, KeyCode::Esc);

        app_state.dialog = Some(Dialog::Bookmarks {
            list: vec![dir.join("gone"), dir.join("one")],
            index: 1,
        });
        // Drawn numbered, the panel's own directory marked.
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        crate::ui::render_ui(&mut terminal, &mut app_state);
        let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
        assert!(screen.contains("1   ") && screen.contains("2 • "), "{screen}");
        press(&mut app_state, KeyCode::Delete);
        assert_eq!(list(&app_state), (vec![dir.join("gone")], 0));
        handle_key(&mut app_state, KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(app_state.dialog.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

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
            super::handle_mouse(
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
            super::handle_mouse(
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

    /// Ctrl+C copies full paths and Alt+C names: of the selection, of the
    /// entry under the cursor, or on the parent entry of the panel itself.
    #[test]
    fn copy_paths_and_names_from_a_panel() {
        let dir = std::env::temp_dir().join(format!("fm84-copy-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "").unwrap();
        std::fs::write(dir.join("b.txt"), "").unwrap();
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), Some("b.txt".as_ref()));
        let chord = |app_state: &mut AppState, modifiers| handle_key(app_state, KeyEvent::new(KeyCode::Char('c'), modifiers));

        chord(&mut app_state, KeyModifiers::CONTROL);
        assert_eq!(app_state.clipboard, dir.join("b.txt").to_string_lossy());
        assert!(app_state.find_note.as_deref().unwrap().starts_with("Copied path: "));
        chord(&mut app_state, KeyModifiers::ALT);
        assert_eq!(app_state.clipboard, "b.txt");

        app_state.selected_left.insert("a.txt".into());
        app_state.selected_left.insert("b.txt".into());
        chord(&mut app_state, KeyModifiers::ALT);
        assert_eq!(app_state.clipboard, "a.txt\nb.txt");
        assert_eq!(app_state.find_note.as_deref(), Some("Copied 2 names"));
        // The note goes with the next key.
        press(&mut app_state, KeyCode::Down);
        assert!(app_state.find_note.is_none());

        app_state.selected_left.clear();
        app_state.state_left.select(Some(0));
        chord(&mut app_state, KeyModifiers::CONTROL);
        assert_eq!(app_state.clipboard, dir.to_string_lossy());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Ctrl+U swaps the panels, tabs, cursors and selections with them, and
    /// the focus goes along; Alt+I sends the other panel here.
    #[test]
    fn swap_the_panels_and_send_the_other_one_here() {
        let dir = std::env::temp_dir().join(format!("fm84-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in ["one", "two", "three"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        std::fs::write(dir.join("one").join("a.txt"), "").unwrap();
        std::fs::write(dir.join("one").join("b.txt"), "").unwrap();
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.join("one"), Some("b.txt".as_ref()));
        app_state.selected_left.insert("a.txt".into());
        app_state.open_dir(false, dir.join("two"), None);
        app_state.is_left_active = false;
        app_state.new_tab();
        app_state.open_dir(false, dir.join("three"), None);
        app_state.is_left_active = true;
        let chord = |app_state: &mut AppState, c, modifiers| handle_key(app_state, KeyEvent::new(KeyCode::Char(c), modifiers));
        let cursor = |app_state: &AppState, is_left: bool| {
            let (children, state) = if is_left { (&app_state.children_left, &app_state.state_left) } else { (&app_state.children_right, &app_state.state_right) };
            children[state.selected().unwrap()].name_full.clone()
        };

        chord(&mut app_state, 'u', KeyModifiers::CONTROL);
        assert_eq!((app_state.dir_left.clone(), app_state.dir_right.clone()), (dir.join("three"), dir.join("one")));
        assert_eq!((app_state.tabs_left.list.len(), app_state.tabs_left.active, app_state.tabs_right.list.len()), (2, 1, 1));
        assert!(!app_state.is_left_active);
        assert_eq!(cursor(&app_state, false), "b.txt");
        assert!(app_state.selected_right.contains(std::ffi::OsStr::new("a.txt")) && app_state.selected_left.is_empty());
        // The other tab went across too, as it was.
        app_state.select_tab(true, 0);
        assert_eq!(app_state.dir_left, dir.join("two"));

        chord(&mut app_state, 'i', KeyModifiers::ALT);
        assert_eq!(app_state.dir_left, dir.join("one"));
        assert_eq!(cursor(&app_state, true), "b.txt");
        assert!(!app_state.is_left_active);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Ctrl+F3 to Ctrl+F6 and a click on a heading sort both panels.
    #[test]
    fn sort_from_the_keys_and_the_headings() {
        use crate::options::{Options, SortKey};
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let dir = std::env::temp_dir().join(format!("fm84-sort-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a long one").unwrap();
        std::fs::write(dir.join("b.rs"), "short").unwrap();
        std::fs::write(dir.join("c.md"), "mid size").unwrap();
        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), Some("b.rs".as_ref()));
        let names = |app_state: &AppState| app_state.children_left.iter().skip(1).map(|item| item.name_full.clone()).collect::<Vec<_>>();
        let control = |app_state: &mut AppState, n| handle_key(app_state, KeyEvent::new(KeyCode::F(n), KeyModifiers::CONTROL));

        // Ctrl+F3 sorts by name, and again turns it round, the cursor
        // staying on the entry it was on.
        control(&mut app_state, 3);
        assert_eq!(names(&app_state), ["a.txt", "b.rs", "c.md"]);
        control(&mut app_state, 3);
        assert!(app_state.options.sort_descending);
        assert_eq!(names(&app_state), ["c.md", "b.rs", "a.txt"]);
        control(&mut app_state, 4);
        assert_eq!((app_state.options.sort_key, app_state.options.sort_descending), (SortKey::Extension, false));
        assert_eq!(names(&app_state), ["c.md", "b.rs", "a.txt"]);
        let cursor = app_state.state_left.selected().unwrap();
        assert_eq!(app_state.children_left[cursor].name_full, "b.rs");

        // A click on the Size heading sorts by size, arrow and all.
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        crate::ui::render_ui(&mut terminal, &mut app_state);
        let (cell, _) = *app_state.sort_headers.iter().find(|(_, key)| *key == SortKey::Size).unwrap();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: cell.x,
            row: cell.y,
            modifiers: KeyModifiers::NONE,
        };
        super::handle_mouse(&mut app_state, click);
        assert_eq!(app_state.options.sort_key, SortKey::Size);
        assert_eq!(names(&app_state), ["b.rs", "c.md", "a.txt"]);
        crate::ui::render_ui(&mut terminal, &mut app_state);
        let heading: String = (cell.x..cell.x + cell.width).map(|x| terminal.backend().buffer()[(x, cell.y)].symbol().to_string()).collect();
        assert_eq!(heading.trim_end(), "Size ↑");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Ctrl+L goes where it is told - a directory, or a file to land on -
    /// and Alt+Left and Alt+Right step back and forth through where the tab
    /// has been, landing on the entry the cursor was on.
    #[test]
    fn go_to_a_path_and_back_and_forward() {
        let dir = std::env::temp_dir().join(format!("fm84-go-to-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
        std::fs::write(dir.join("a.txt"), "text").unwrap();
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), Some("sub".as_ref()));
        let alt = |app_state: &mut AppState, code: KeyCode| handle_key(app_state, KeyEvent::new(code, KeyModifiers::ALT));
        let ctrl_l = |app_state: &mut AppState| handle_key(app_state, KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
        let cursor = |app_state: &AppState| {
            let index = app_state.state_left.selected().unwrap();
            app_state.children_left[index].name.clone()
        };
        let type_in = |app_state: &mut AppState, text: &str| {
            let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &mut app_state.dialog else { panic!("no prompt") };
            input.set(text.to_string());
        };

        // It opens on where the panel is, ready to have a name typed on.
        ctrl_l(&mut app_state);
        let Some(Dialog::Prompt(PromptKind::GoTo, input)) = &app_state.dialog else { panic!("no prompt") };
        assert_eq!(input.text, format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR));
        // Tab fills out the name, and Enter goes there.
        press(&mut app_state, KeyCode::Char('s'));
        press(&mut app_state, KeyCode::Tab);
        press(&mut app_state, KeyCode::Char('d'));
        press(&mut app_state, KeyCode::Tab);
        press(&mut app_state, KeyCode::Enter);
        assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

        // A path from the panel, `..` and all, to a file: landed on, in the
        // directory it is in.
        ctrl_l(&mut app_state);
        type_in(&mut app_state, "../../a.txt");
        press(&mut app_state, KeyCode::Enter);
        assert_eq!((app_state.dir_left.clone(), cursor(&app_state)), (dir.clone(), "a".to_string()));

        // Somewhere that is not there is said to be, and the panel stays.
        ctrl_l(&mut app_state);
        type_in(&mut app_state, "nowhere");
        press(&mut app_state, KeyCode::Enter);
        assert!(app_state.error.take().unwrap().contains("nowhere"));
        assert_eq!(app_state.dir_left, dir);

        // Back the way it came, onto what the cursor was on, and forward.
        alt(&mut app_state, KeyCode::Left);
        assert_eq!(app_state.dir_left, dir.join("sub/deeper"));
        alt(&mut app_state, KeyCode::Left);
        assert_eq!((app_state.dir_left.clone(), cursor(&app_state)), (dir.clone(), "sub".to_string()));
        alt(&mut app_state, KeyCode::Right);
        assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

        // A step somewhere new drops what was ahead.
        press(&mut app_state, KeyCode::Backspace);
        assert_eq!(app_state.dir_left, dir.join("sub"));
        alt(&mut app_state, KeyCode::Right);
        assert_eq!(app_state.dir_left, dir.join("sub"));
        alt(&mut app_state, KeyCode::Left);
        assert_eq!(app_state.dir_left, dir.join("sub/deeper"));

        // A directory gone since is passed over, rather than stuck on: from
        // sub, back to deeper climbs out of it to sub again, and on to dir.
        std::fs::remove_dir(dir.join("sub/deeper")).unwrap();
        alt(&mut app_state, KeyCode::Right);
        assert_eq!(app_state.dir_left, dir.join("sub"));
        alt(&mut app_state, KeyCode::Left);
        assert_eq!(app_state.dir_left, dir, "the gone one was passed over");
        alt(&mut app_state, KeyCode::Right);
        assert_eq!(app_state.dir_left, dir.join("sub"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_typed_path_starts_from_the_panel_or_from_home() {
        let base = std::path::Path::new("/panel/dir");
        let home = std::env::home_dir().unwrap_or_default();
        assert_eq!(super::typed_path("/abs/x", base), std::path::Path::new("/abs/x"));
        assert_eq!(super::typed_path("x", base), base.join("x"));
        assert_eq!(super::typed_path("sub/x", base), base.join("sub/x"));
        assert_eq!(super::typed_path("~", base), home);
        assert_eq!(super::typed_path("~/x", base), home.join("x"));
        // Only a `~` that stands for home: one starting a name is the name.
        assert_eq!(super::typed_path("~x", base), base.join("~x"));
    }

    /// F5 and F6 go where the field says. A single file opens on its path in
    /// the other panel, anything else on the other panel's directory, and
    /// either goes there when it is left as it opened. A name edited in
    /// renames it on the way, and a bare one puts it beside the original. Y
    /// and N are typed, not taken for an answer.
    #[test]
    fn copy_and_move_go_where_the_destination_says() {
        let dir = std::env::temp_dir().join(format!("fm84-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (left, right) = (dir.join("left"), dir.join("right"));
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join("a.txt"), "a").unwrap();
        std::fs::write(left.join("z.txt"), "z").unwrap();
        std::fs::create_dir(left.join("sub")).unwrap();

        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(false, right.clone(), None);
        let on_a = |app_state: &mut AppState| {
            app_state.open_dir(true, left.clone(), Some("a.txt".as_ref()));
        };
        let finish = |app_state: &mut AppState| {
            let started = std::time::Instant::now();
            while app_state.job.is_some() {
                assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
                std::thread::sleep(std::time::Duration::from_millis(5));
                app_state.poll_transfer();
            }
        };
        let type_in = |app_state: &mut AppState, text: &str| {
            app_state.dialog_input().unwrap().set(text.to_string());
            press(app_state, KeyCode::Enter);
        };

        // A single file opens on its path in the other panel, and Enter alone
        // copies it there.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(5));
        assert_eq!(std::path::PathBuf::from(&app_state.dialog_input().unwrap().text), right.join("a.txt"));
        press(&mut app_state, KeyCode::Enter);
        finish(&mut app_state);
        assert_eq!(std::fs::read_to_string(right.join("a.txt")).unwrap(), "a");

        // Again, onto the copy now there: the job stops to ask about it, and
        // Esc leaves it as it was.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(5));
        press(&mut app_state, KeyCode::Enter);
        let started = std::time::Instant::now();
        while !app_state.job.as_ref().is_some_and(|job| job.clash.is_some()) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "never asked");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
        press(&mut app_state, KeyCode::Esc);
        finish(&mut app_state);
        assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Copy cancelled")), "{:?}", app_state.error);
        assert_eq!(std::fs::read_to_string(right.join("a.txt")).unwrap(), "a");
        app_state.reset_error();

        // The name edited, one key at a time.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(5));
        for _ in "a.txt".chars() {
            press(&mut app_state, KeyCode::Backspace);
        }
        for c in "b.txt".chars() {
            press(&mut app_state, KeyCode::Char(c));
        }
        press(&mut app_state, KeyCode::Enter);
        finish(&mut app_state);
        assert!(right.join("b.txt").exists());

        // N is typed into the field, and Esc leaves it.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(5));
        press(&mut app_state, KeyCode::Char('n'));
        assert!(app_state.dialog_input().unwrap().text.ends_with('n'));
        press(&mut app_state, KeyCode::Esc);
        assert!(app_state.dialog.is_none() && app_state.job.is_none());

        // A bare name is beside the original.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(5));
        type_in(&mut app_state, "dup.txt");
        finish(&mut app_state);
        assert_eq!(std::fs::read_to_string(left.join("dup.txt")).unwrap(), "a");

        // F6 to a new name in the same place is a rename.
        on_a(&mut app_state);
        press(&mut app_state, KeyCode::F(6));
        type_in(&mut app_state, "moved.txt");
        finish(&mut app_state);
        assert!(!left.join("a.txt").exists() && left.join("moved.txt").exists());

        // A directory in the way of a file is refused, not copied into, when
        // the field is left as it opened.
        std::fs::create_dir(right.join("z.txt")).unwrap();
        app_state.open_dir(true, left.clone(), Some("z.txt".as_ref()));
        press(&mut app_state, KeyCode::F(5));
        press(&mut app_state, KeyCode::Enter);
        assert!(app_state.error.as_deref().is_some_and(|error| error.contains("Cannot replace directory")), "{:?}", app_state.error);
        assert!(!right.join("z.txt").join("z.txt").exists());
        app_state.reset_error();

        // A directory, and several entries, open on the other panel's
        // directory alone.
        let opened_on_directory = |app_state: &mut AppState| {
            let text = app_state.dialog_input().unwrap().text.clone();
            std::path::Path::new(&text) == right && text.ends_with(std::path::MAIN_SEPARATOR)
        };
        app_state.open_dir(true, left.clone(), Some("sub".as_ref()));
        press(&mut app_state, KeyCode::F(5));
        assert!(opened_on_directory(&mut app_state));
        press(&mut app_state, KeyCode::Esc);
        app_state.open_dir(true, left.clone(), None);
        app_state.selected_left.extend(["dup.txt".into(), "z.txt".into()]);
        press(&mut app_state, KeyCode::F(5));
        assert!(opened_on_directory(&mut app_state));

        // And several cannot share one name.
        type_in(&mut app_state, "one.txt");
        assert!(app_state.error.as_deref().is_some_and(|error| error.contains("2 items need a directory")), "{:?}", app_state.error);
        assert!(!left.join("one.txt").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Alt+F7 asks what to look for, offering the last search; Enter looks,
    /// and Enter on a result takes the panel to it, cursor and all.
    #[test]
    fn find_files_finds_and_goes_there() {
        let dir = std::env::temp_dir().join(format!("fm84-find-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a").join("deep")).unwrap();
        std::fs::write(dir.join("a").join("deep").join("notes.md"), "the needle is here").unwrap();
        std::fs::write(dir.join("a").join("other.md"), "nothing").unwrap();
        std::fs::write(dir.join("top.txt"), "needle").unwrap();

        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.clone(), None);
        let alt_f7 = KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT);
        handle_key(&mut app_state, alt_f7);
        assert_eq!(app_state.dialog_input().unwrap().text, "*", "everything, the first time");

        // *.md, then Tab to the text.
        app_state.dialog_input().unwrap().set("*.md".to_string());
        press(&mut app_state, KeyCode::Tab);
        for c in "needle".chars() {
            press(&mut app_state, KeyCode::Char(c));
        }
        press(&mut app_state, KeyCode::Enter);
        let started = std::time::Instant::now();
        let found = loop {
            app_state.poll_find_files();
            if let Some(Dialog::FindFiles(find)) = &app_state.dialog
                && let FindFiles::Found(search) = find.as_ref()
                && search.done
            {
                break search.results.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>();
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the search never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(found, [dir.join("a").join("deep").join("notes.md")]);

        // Enter goes there: its directory, with the cursor on it.
        press(&mut app_state, KeyCode::Enter);
        assert!(app_state.dialog.is_none());
        assert_eq!(app_state.dir_left, dir.join("a").join("deep"));
        let on = app_state.state_left.selected().and_then(|index| app_state.children_left.get(index)).map(|item| item.name_full.clone());
        assert_eq!(on.as_deref(), Some("notes.md"));

        // The next time, the last search is offered again.
        handle_key(&mut app_state, KeyEvent::new(KeyCode::F(7), KeyModifiers::CONTROL));
        assert_eq!(app_state.dialog_input().unwrap().text, "*.md");
        press(&mut app_state, KeyCode::Esc);
        assert!(app_state.dialog.is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

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

    /// The other panel on its way to a directory slow to read: F5 offers the
    /// one it is going to, not the one it is leaving.
    #[test]
    fn a_copy_goes_where_the_other_panel_is_going() {
        let dir = std::env::temp_dir().join(format!("fm84-copy-pending-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["from", "leaving", "going"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("from").join("a.txt"), "").unwrap();
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        app_state.open_dir(true, dir.join("from"), Some("a.txt".as_ref()));
        app_state.open_dir(false, dir.join("leaving"), None);
        app_state.listing_right = Some(crate::app::PendingListing::going_to(dir.join("going")));

        press(&mut app_state, KeyCode::F(5));
        assert_eq!(std::path::PathBuf::from(&app_state.dialog_input().unwrap().text), dir.join("going").join("a.txt"));
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
