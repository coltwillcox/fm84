//! Making entries - a file, a directory, a link, a new name - and saving the
//! editor's file whole, beside the old one, before swapping it in.

use super::*;

pub fn rename_path(original_path: PathBuf, new_path: PathBuf) -> Result<(), Error> {
    rename(original_path, new_path)?;
    Ok(())
}

/// Create an empty file, refusing if anything is already there - create_new
/// fails rather than truncating, matching how create_dir refuses.
pub fn create_file(path: PathBuf) -> Result<(), Error> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    Ok(())
}

pub fn create_directory(path: PathBuf) -> Result<(), Error> {
    create_dir(path)?;
    Ok(())
}

/// A symbolic link at `link` pointing to `target`, as `ln -s` makes one.
/// Never in place of anything already there - a dangling link included.
/// Windows makes a different kind of link for a directory, and refuses both
/// without Developer Mode or the right to make them.
pub fn create_symlink(target: &Path, link: &Path, is_dir: bool) -> Result<(), Error> {
    if path_exists(link) {
        return Err(Error::new(ErrorKind::AlreadyExists, format!("{} already exists", link.display())));
    }
    #[cfg(unix)]
    {
        let _ = is_dir;
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if is_dir { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, is_dir);
        Err(Error::new(ErrorKind::Unsupported, "Symbolic links are not supported here"))
    }
}

/// Save `content` over the file at `path` so that a failure partway - a full
/// disk, a device gone, fm84 killed - leaves the file as it was rather than
/// truncated. Written straight over, the file is emptied the moment the write
/// begins. Instead the bytes go to a temporary beside it, which is flushed to
/// disk and then renamed into place, so the file is whole either way.
///
/// A rename puts a new file where the old one was, which is not always the
/// same thing as changing it. Where the difference would show, the file is
/// written in place as before:
/// - it has other hard links, which would go on holding the old content;
/// - its owner cannot be kept, as when editing someone else's file that the
///   group may write to;
/// - its directory cannot be added to, though the file itself can be written.
pub fn save_file(path: &Path, content: &[u8]) -> Result<(), Error> {
    // Through a link to the file it names: renaming over the link would put a
    // regular file in its place and leave what it points at unedited.
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let in_place = || fs::write(&target, content);

    let Ok(metadata) = fs::metadata(&target) else {
        // Gone since it was opened, so there is nothing to keep safe.
        return in_place();
    };
    if !metadata.is_file() {
        return Err(not_regular(&target));
    }
    // Asked of the file itself, since a rename needs only the directory: a
    // read-only file would otherwise be replaced without a word. Opened
    // without truncating, so asking changes nothing.
    File::options().write(true).open(&target)?;
    if has_other_links(&metadata) {
        return in_place();
    }

    let (temp, mut file) = match temp_beside(&target) {
        Ok(pair) => pair,
        Err(e) if e.kind() == ErrorKind::PermissionDenied => return in_place(),
        Err(e) => return Err(e),
    };
    if keep_owner(&file, &metadata).is_err() {
        drop(file);
        let _ = remove_file(&temp);
        return in_place();
    }
    // After the owner, which can clear the setuid and setgid bits.
    let written = file.write_all(content).and_then(|()| file.set_permissions(metadata.permissions())).and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written.and_then(|()| rename(&temp, &target)) {
        let _ = remove_file(&temp);
        return Err(e);
    }
    Ok(())
}

#[cfg(unix)]
fn has_other_links(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn has_other_links(_metadata: &fs::Metadata) -> bool {
    false
}

/// Give the temporary the owner and group of the file it will replace. Only
/// root may give a file away, but anyone may set what is already theirs, so
/// this fails exactly when the rename would change who owns the file.
#[cfg(unix)]
fn keep_owner(file: &File, metadata: &fs::Metadata) -> Result<(), Error> {
    use std::os::unix::fs::MetadataExt;
    std::os::unix::fs::fchown(file, Some(metadata.uid()), Some(metadata.gid()))
}

#[cfg(not(unix))]
fn keep_owner(_file: &File, _metadata: &fs::Metadata) -> Result<(), Error> {
    Ok(())
}

/// A small file written whole - a checksum file - beside where it goes and
/// then put there, written over only when `replacing`.
pub fn write_whole(dest: &Path, content: &[u8], replacing: bool) -> Result<(), Error> {
    let (temp, mut file) = temp_beside(dest)?;
    if let Err(e) = file.write_all(content).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = remove_file(&temp);
        return Err(e);
    }
    drop(file);
    super::transfer::place(&temp, dest, replacing)
}

/// A file to write into beside `dest`, renamed over it once it is whole.
/// Same directory, so the rename that finishes the copy is atomic and cannot
/// half-replace anything; a temporary elsewhere would have to be copied back
/// across, which is the thing being avoided.
///
/// Named after the file, shortened: the prefix and suffix would otherwise
/// carry a name already near the filesystem's limit past it, and every copy
/// goes through one of these now. Shortened in bytes, which is what the limit
/// counts - 64 characters of emoji are 256 of them.
pub(super) fn temp_beside(dest: &Path) -> Result<(PathBuf, File), Error> {
    const NAME_BYTES: usize = 64;
    let mut name = String::new();
    for character in dest.file_name().unwrap_or_default().to_string_lossy().chars() {
        if name.len() + character.len_utf8() > NAME_BYTES {
            break;
        }
        name.push(character);
    }
    for attempt in 0..1000u32 {
        let candidate = dest.with_file_name(format!(".{name}.fm84-{}-{attempt}", std::process::id()));
        match File::options().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            // Something of ours from a run that was killed partway. Step past it.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Error::new(ErrorKind::AlreadyExists, format!("Cannot make room beside {}", dest.display())))
}

/// Clear a link out of the way of a file about to be written at `dest`, so the
/// write replaces the link instead of following it into whatever it points
/// at. Anything else stays for the write to deal with.
pub(super) fn clear_link(dest: &Path) -> Result<(), Error> {
    match dest.symlink_metadata() {
        // A link to a directory is removed as a directory on Windows.
        Ok(metadata) if metadata.file_type().is_symlink() => remove_file(dest).or_else(|_| remove_dir(dest)),
        _ => Ok(()),
    }
}

/// Make room at `dest` for a link about to be recreated there: a file or a
/// link in the way goes. A directory does not - no link replaces a tree.
pub(super) fn clear_for_link(dest: &Path) -> Result<(), Error> {
    match dest.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => Err(Error::new(ErrorKind::AlreadyExists, format!("Cannot replace directory {} with a link", dest.display()))),
        Ok(_) => clear_link(dest).and_then(|()| if path_exists(dest) { remove_file(dest) } else { Ok(()) }),
        Err(_) => Ok(()),
    }
}
