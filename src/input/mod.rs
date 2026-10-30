//! Keys, pastes and the mouse, each to whatever is on top: a running job, an
//! error, the dialog that is open, then the screen.

use crate::app::{AfterSave, Answer, AppState, Clashes, Dialog, FindFiles, PromptKind, READ_ONLY, Saved, Screen, TextInput, TransferKind};
use crate::fs_ops::{check_destinations, create_directory, create_file, create_hard_link, create_symlink, is_plain_name, is_same_entry, path_exists, rename_path, transfer_targets, typed_path};
use crate::options::{OnExisting, SortKey};
use crate::strip::StripHit;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use ratatui::widgets::TableState;
use std::io::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod file_ops;
mod launch;
mod mouse;
mod panel;
mod screens;
#[cfg(test)]
mod tests;

pub(crate) use self::launch::open_with_default;
use self::{file_ops::*, launch::*, mouse::*, panel::*, screens::*};

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
        Event::Mouse(mouse_event) => return Ok(mouse_to(app_state, mouse_event)),
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
        app_state.stay_put = key.modifiers.contains(KeyModifiers::SHIFT);
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
        Screen::Diff(_) => diff_key(app_state, key),
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
        // Alt+A changes the attributes of what is chosen, as in Total
        // Commander.
        if in_panel && c == 'a' {
            let items = chosen_items(app_state);
            if !items.is_empty() && !app_state.refuse_in_archive(app_state.is_left_active) {
                app_state.open_attributes(items);
            }
        }
        // Alt+C copies the names of what Ctrl+C would copy the paths of.
        if in_panel && c == 'c' {
            copy_names(app_state, false);
        }
        // Alt+H works out checksums, or checks a checksum file.
        if in_panel && c == 'h' {
            open_checksums(app_state);
        }
        // Alt+P packs what is chosen into an archive.
        if in_panel && c == 'p' {
            open_pack(app_state);
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
            // Ctrl+S, Total Commander's quick filter.
            's' if in_panel => app_state.open_filter_prompt(),
            // Ctrl+O opens what is under the cursor as the system would, as
            // Shift+Enter does where the terminal can send it.
            'o' if in_panel => open_cursor_externally(app_state),
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
        KeyCode::Char('k') | KeyCode::Char('K') if clash => job.answer(Answer::KeepBoth),
        KeyCode::Char('b') | KeyCode::Char('B') if clash => job.answer(Answer::KeepBothAll),
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
        KeyCode::F(10) if app_state.quit_armed => {
            app_state.stay_put = key.modifiers.contains(KeyModifiers::SHIFT);
            return false;
        }
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
            let page = app_state.help_room.max(1) as isize;
            match code {
                KeyCode::Esc | KeyCode::F(1) => app_state.dialog = None,
                KeyCode::Down => scroll_help(app_state, 1),
                KeyCode::Up => scroll_help(app_state, -1),
                KeyCode::PageDown => scroll_help(app_state, page),
                KeyCode::PageUp => scroll_help(app_state, -page),
                KeyCode::Home => scroll_help(app_state, isize::MIN),
                KeyCode::End => scroll_help(app_state, isize::MAX),
                _ => {}
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
        Some(Dialog::Symlink { .. }) => match code {
            KeyCode::Enter => handle_symlink_confirm(app_state),
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(Dialog::Symlink { hard, .. }) = &mut app_state.dialog {
                    *hard = !*hard;
                }
            }
            _ => edit_text(app_state, code),
        },
        Some(Dialog::Checksums(_)) => checksums_key(app_state, code),
        Some(Dialog::Pack { .. }) => match code {
            KeyCode::Enter => handle_pack_confirm(app_state),
            KeyCode::Tab => cycle_pack_format(app_state),
            KeyCode::Esc => app_state.dialog = None,
            _ => edit_text(app_state, code),
        },
        Some(Dialog::Extract { .. }) => match code {
            KeyCode::Enter => handle_extract_confirm(app_state),
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
        Some(Dialog::Attributes(_)) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.apply_attributes(),
            KeyCode::Tab | KeyCode::Down => app_state.attributes_switch_field(true),
            KeyCode::BackTab | KeyCode::Up => app_state.attributes_switch_field(false),
            _ => edit_text(app_state, code),
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
        // Typing narrows the list; the arrows go up and down it.
        Some(Dialog::History(_)) => {
            let page = 10;
            match code {
                KeyCode::Esc => app_state.dialog = None,
                KeyCode::Enter => app_state.go_to_history(),
                KeyCode::Up => app_state.move_history_cursor(-1),
                KeyCode::Down => app_state.move_history_cursor(1),
                KeyCode::PageUp => app_state.move_history_cursor(-page),
                KeyCode::PageDown => app_state.move_history_cursor(page),
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete => {
                    edit_text(app_state, code);
                    app_state.history_typed();
                }
                _ => edit_text(app_state, code),
            }
        }
        Some(Dialog::Branches(_)) => match code {
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.switch_branch(),
            KeyCode::Up => app_state.move_branch_cursor(-1),
            KeyCode::Down => app_state.move_branch_cursor(1),
            KeyCode::PageUp => app_state.move_branch_cursor(-10),
            KeyCode::PageDown => app_state.move_branch_cursor(10),
            KeyCode::Home => app_state.move_branch_cursor(isize::MIN),
            KeyCode::End => app_state.move_branch_cursor(isize::MAX),
            _ => {}
        },
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
            KeyCode::Esc if kind == PromptKind::Filter => app_state.cancel_filter_prompt(),
            KeyCode::Esc => app_state.dialog = None,
            KeyCode::Enter => app_state.confirm_prompt(),
            KeyCode::Char(c) if kind == PromptKind::GoToLine && !c.is_ascii_digit() => {}
            KeyCode::Tab if kind == PromptKind::GoTo => app_state.complete_go_to(),
            _ => {
                edit_text(app_state, code);
                app_state.filter_typed();
            }
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
        app_state.filter_typed();
        app_state.history_typed();
    } else if app_state.dialog.is_none() && app_state.is_editing() {
        app_state.editor_insert_text(text);
    }
}

/// Scroll F1's list by `lines`, no further than its last line coming into
/// view at the bottom.
fn scroll_help(app_state: &mut AppState, lines: isize) {
    let last = crate::ui::help_lines().len().saturating_sub(app_state.help_room);
    app_state.help_scroll = app_state.help_scroll.saturating_add_signed(lines).min(last);
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
