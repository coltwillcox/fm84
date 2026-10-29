//! The dialogs that change files - rename, create, copy, move, link, delete -
//! and Shift+F3, which picks two files to compare.

use super::*;

/// F2 on the entry under the cursor, typed into in place of its name.
pub(super) fn open_rename(app_state: &mut AppState) {
    if app_state.refuse_in_archive(app_state.is_left_active) {
        return;
    }
    let children = if app_state.is_left_active { &app_state.children_left } else { &app_state.children_right };
    let state = if app_state.is_left_active { &app_state.state_left } else { &app_state.state_right };
    let Some(item) = state.selected().and_then(|index| children.get(index)) else {
        return;
    };

    // Don't rename the parent entry - ".." resolves to the parent directory itself.
    if item.is_parent() {
        return;
    }

    let mut input = TextInput::new();
    input.set(item.name_full.clone());
    app_state.dialog = Some(Dialog::Rename(input));
}

/// One dialog serves both: F7 makes a directory, Shift+F4 an empty file.
pub(super) fn toggle_create(app_state: &mut AppState, is_dir: bool) {
    // The same key closes it again; the other one switches what is being made.
    if matches!(app_state.dialog, Some(Dialog::Create { is_dir: making, .. }) if making == is_dir) {
        app_state.dialog = None;
        return;
    }
    if app_state.refuse_in_archive(app_state.is_left_active) {
        return;
    }
    app_state.dialog = Some(Dialog::Create { is_dir, input: TextInput::new() });
}

pub(super) fn handle_rename(app_state: &mut AppState) {
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
    if item.is_parent() {
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

/// F8 or Delete, to the trash unless F11 says otherwise, and Shift+F8 or
/// Shift+Delete for good. That one always asks first, whatever Confirm delete
/// is set to: there is no getting it back.
pub(super) fn open_delete(app_state: &mut AppState, permanently: bool) {
    let items = chosen_items(app_state);
    if items.is_empty() || app_state.refuse_in_archive(app_state.is_left_active) {
        return;
    }
    let to_trash = app_state.options.delete_to_trash && !permanently;
    app_state.dialog = Some(Dialog::Delete { items, to_trash });
    if !app_state.options.confirm_delete && !permanently {
        handle_delete_confirm(app_state);
    }
}

pub(super) fn handle_delete_confirm(app_state: &mut AppState) {
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

pub(super) fn handle_create_confirm(app_state: &mut AppState) {
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

/// F5 or F6: copy or move what is chosen, to where the dialog is told. For a
/// single file it opens on that file's path in the other panel, ready to be
/// given another name; for anything else - several entries, or a directory -
/// on the other panel's directory. Enter alone does what it always did. With
/// Confirm copy and move off there is no dialog, and it goes straight there.
pub(super) fn open_transfer(app_state: &mut AppState, is_copy: bool) {
    let sources = chosen_items(app_state);
    if sources.is_empty() {
        return;
    }
    // Out of an archive is a copy; a move would take entries out of it.
    if !is_copy && app_state.refuse_in_archive(app_state.is_left_active) {
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
pub(super) fn handle_transfer_confirm(app_state: &mut AppState) {
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
    // Into an archive: the other panel inside one, or a path typed through
    // one. Asked of the disk, as transfer_targets asks it.
    if crate::archive::locate(destination).is_some() {
        app_state.display_error(READ_ONLY.to_string());
        return;
    }
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
    // Into an archive, whichever way the destination was arrived at.
    if items.iter().any(|(_, dest, _)| dest.parent().is_some_and(|dir| crate::archive::locate(dir).is_some())) {
        return app_state.display_error(READ_ONLY.to_string());
    }
    if let Some(archive) = app_state.panel_archive(app_state.is_left_active).cloned() {
        return submit_extract(app_state, archive, items);
    }
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

/// F5 from a panel inside an archive: copy the entries out. What no answer
/// could make safe is refused before anything is written, as for a copy - a
/// file and a directory in each other's place - going by what the archive
/// says each entry is, since none of them is on the disk to be asked.
fn submit_extract(app_state: &mut AppState, archive: PathBuf, items: Vec<(PathBuf, PathBuf, bool)>) {
    let mut taken = Vec::new();
    let mut extracting = Vec::new();
    for (source, dest, is_dir) in items {
        if let Ok(metadata) = dest.symlink_metadata() {
            match (is_dir, metadata.is_dir()) {
                (false, true) => return app_state.display_error(format!("Cannot replace directory {} with a file", dest.display())),
                (true, false) => return app_state.display_error(format!("Cannot replace file {} with a directory", dest.display())),
                _ => taken.push(dest.clone()),
            }
        }
        let Ok(inner) = source.strip_prefix(&archive) else {
            continue;
        };
        extracting.push((inner.to_path_buf(), dest));
    }
    match app_state.options.on_existing {
        OnExisting::Refuse if !taken.is_empty() => app_state.display_error(format!("Destination already exists: {}", taken[0].display())),
        OnExisting::Overwrite => app_state.start_extract(archive, extracting, Clashes::ReplaceAll, None),
        OnExisting::Ask | OnExisting::Refuse => app_state.start_extract(archive, extracting, Clashes::Ask, None),
    }
}

/// Shift+F3: which two files to compare. Two files selected in the panel you
/// are in, in the order it lists them; otherwise the file under the cursor,
/// against the one of the same name in the other panel, or failing that the
/// one under the cursor there. Each side of the comparison is the panel its
/// file came from.
pub(super) fn open_compare(app_state: &mut AppState) {
    let is_left = app_state.is_left_active;
    if app_state.panel_archive(true).is_some() || app_state.panel_archive(false).is_some() {
        app_state.display_error("Files inside an archive cannot be compared yet".to_string());
        return;
    }
    let file_at_cursor = |app_state: &AppState, is_left: bool| {
        let (children, state, dir) = if is_left {
            (&app_state.children_left, &app_state.state_left, &app_state.dir_left)
        } else {
            (&app_state.children_right, &app_state.state_right, &app_state.dir_right)
        };
        state.selected().and_then(|index| children.get(index)).filter(|item| !item.is_dir).map(|item| (item.name_os.clone(), item.path_in(dir)))
    };

    let (children, selected, dir) = if is_left {
        (&app_state.children_left, &app_state.selected_left, &app_state.dir_left)
    } else {
        (&app_state.children_right, &app_state.selected_right, &app_state.dir_right)
    };
    let selected_files: Vec<PathBuf> = children.iter().filter(|item| !item.is_dir && selected.contains(&item.name_os)).map(|item| item.path_in(dir)).collect();
    if let [first, second] = &selected_files[..] {
        app_state.compare_files(first.clone(), second.clone());
        return;
    }

    let Some((name, here)) = file_at_cursor(app_state, is_left) else {
        app_state.display_error("Put the cursor on a file to compare, or select two".to_string());
        return;
    };
    let (other_children, other_dir) = if is_left { (&app_state.children_right, &app_state.dir_right) } else { (&app_state.children_left, &app_state.dir_left) };
    let namesake = other_children.iter().find(|item| !item.is_dir && item.name_os == name).map(|item| item.path_in(other_dir));
    let Some(there) = namesake.or_else(|| file_at_cursor(app_state, !is_left).map(|(_, path)| path)) else {
        app_state.display_error("Nothing to compare with: the other panel has no file of that name, nor one under its cursor".to_string());
        return;
    };
    if is_left {
        app_state.compare_files(here, there);
    } else {
        app_state.compare_files(there, here);
    }
}

/// Ctrl+Shift+F5: a symbolic link to what is chosen. For a single entry,
/// file or directory, it opens on that name in the other panel, ready to be
/// given another; for several, on the other panel's directory, a link in it
/// for each under its own name. Always asked, whatever F11 says about
/// confirming copies: there is no destination to take for granted.
pub(super) fn open_symlink(app_state: &mut AppState) {
    let sources = chosen_items(app_state);
    // A link to an entry of an archive would point at nothing on the disk.
    if sources.is_empty() || app_state.refuse_in_archive(app_state.is_left_active) {
        return;
    }
    let other = app_state.panel_dir(!app_state.is_left_active);
    let (default, into_dir) = match &sources[..] {
        [(source, _)] => (other.join(source.file_name().unwrap_or_default()), false),
        _ => (other, true),
    };
    let mut opened_with = default.to_string_lossy().into_owned();
    if into_dir && !opened_with.ends_with(std::path::is_separator) {
        opened_with.push(std::path::MAIN_SEPARATOR);
    }
    let mut input = TextInput::new();
    input.set(opened_with.clone());
    app_state.dialog = Some(Dialog::Symlink { sources, input, opened_with, default });
}

/// Enter on the symlink dialog. Read as F5 reads where to copy to, each link
/// pointing at its source by the absolute path. An entry that cannot be
/// linked is passed over and named, and the rest are still made.
pub(super) fn handle_symlink_confirm(app_state: &mut AppState) {
    let Some(Dialog::Symlink { sources, input, opened_with, default }) = app_state.dialog.take() else {
        return;
    };
    let links = if input.text == opened_with {
        match &sources[..] {
            [(source, is_dir)] => Ok(vec![(source.clone(), default, *is_dir)]),
            _ => transfer_targets(&sources, &default, true),
        }
    } else if input.text.trim().is_empty() {
        return;
    } else {
        let base = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
        transfer_targets(&sources, &typed_path(&input.text, base), input.text.ends_with(std::path::is_separator))
    };
    // Made inside an archive: the other panel in one, or a path typed
    // through one.
    if let Ok(links) = &links
        && links.iter().any(|(_, link, _)| link.parent().is_some_and(|dir| crate::archive::locate(dir).is_some()))
    {
        app_state.display_error(READ_ONLY.to_string());
        return;
    }
    let links = match links {
        Ok(links) => links,
        Err(message) => {
            app_state.display_error(message);
            return;
        }
    };

    let mut failed = Vec::new();
    for (target, link, is_dir) in &links {
        if let Err(e) = create_symlink(target, link, *is_dir) {
            failed.push(format!("{}: {}", link.display(), e));
        }
    }

    // A single link is landed on, in whichever panel shows where it went.
    let landed = match &links[..] {
        [(_, link, _)] if failed.is_empty() => link.parent().zip(link.file_name()),
        _ => None,
    };
    for is_left in [true, false] {
        let name = landed.filter(|(parent, _)| app_state.panel_dir(is_left) == *parent).map(|(_, name)| name);
        app_state.reload_panel(is_left, name);
    }
    match failed.len() {
        0 => {}
        1 if links.len() == 1 => app_state.display_error(format!("Cannot link {}", failed[0])),
        count => app_state.display_error(format!("Could not link {count} of {}. {}", links.len(), failed[0])),
    }
}
