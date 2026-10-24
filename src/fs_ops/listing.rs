//! What a directory's listing is made of: each entry's row, the attributes and
//! owner shown for it, and the git branch and user@host shown above.

use super::*;

/// Permission bits as `ls -l` writes them. The mode comes from the metadata the
/// listing already holds, so this costs no extra syscall.
#[cfg(unix)]
fn format_attributes(metadata: &fs::Metadata, is_dir: bool, is_symlink: bool) -> String {
    use std::os::unix::fs::PermissionsExt;

    let kind = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let mut text = String::with_capacity(10);
    text.push(kind);
    text.push_str(&mode_string(metadata.permissions().mode()));
    text
}

/// The nine permission bits as `ls -l` writes them, `rwxr-xr-x`. Each
/// triplet's execute slot doubles as the setuid/setgid/sticky flag, which
/// would otherwise be invisible.
#[cfg(unix)]
pub fn mode_string(mode: u32) -> String {
    let triplet = |shift: u32, special: u32, on: char| {
        let bits = mode >> shift;
        let execute = bits & 0o1 != 0;
        [
            if bits & 0o4 != 0 { 'r' } else { '-' },
            if bits & 0o2 != 0 { 'w' } else { '-' },
            match (mode & special != 0, execute) {
                (true, true) => on,
                (true, false) => on.to_ascii_uppercase(),
                (false, true) => 'x',
                (false, false) => '-',
            },
        ]
    };

    let mut text = String::with_capacity(9);
    text.extend(triplet(6, 0o4000, 's'));
    text.extend(triplet(3, 0o2000, 's'));
    text.extend(triplet(0, 0o1000, 't'));
    text
}

#[cfg(windows)]
fn format_attributes(metadata: &fs::Metadata, _is_dir: bool, is_symlink: bool) -> String {
    use std::os::windows::fs::MetadataExt;

    const READONLY: u32 = 0x0000_0001;
    const HIDDEN: u32 = 0x0000_0002;
    const SYSTEM: u32 = 0x0000_0004;
    const DIRECTORY: u32 = 0x0000_0010;
    const ARCHIVE: u32 = 0x0000_0020;

    let flags = metadata.file_attributes();
    let flag = |bit: u32, on: char| if flags & bit != 0 { on } else { '-' };
    let kind = if is_symlink { 'l' } else { flag(DIRECTORY, 'd') };

    [kind, flag(ARCHIVE, 'a'), flag(READONLY, 'r'), flag(HIDDEN, 'h'), flag(SYSTEM, 's')].iter().collect()
}

#[cfg(not(any(unix, windows)))]
fn format_attributes(_metadata: &fs::Metadata, _is_dir: bool, _is_symlink: bool) -> String {
    String::new()
}

/// Hidden the way the platform means it: a leading dot, or on Windows the
/// hidden attribute, which is what Explorer goes by.
#[cfg(windows)]
pub fn is_hidden(_name: &str, metadata: Option<&fs::Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x0000_0002;
    metadata.is_some_and(|metadata| metadata.file_attributes() & HIDDEN != 0)
}

#[cfg(not(windows))]
pub fn is_hidden(name: &str, _metadata: Option<&fs::Metadata>) -> bool {
    name.starts_with('.')
}

pub fn load_directory_rows(path: &Path, options: &Options) -> Result<Vec<Item>, Error> {
    let entries: Vec<_> = read_dir(path)?.filter_map(|entry| entry.ok()).collect();

    let has_parent = path.parent().is_some();
    let mut children = Vec::with_capacity(entries.len() + usize::from(has_parent));

    // Don't add ".." on root folder.
    if has_parent {
        children.push(Item {
            name_os: OsString::from(".."),
            name_full: "..".to_string(),
            name: "..".to_string(),
            extension: String::new(),
            is_dir: true,
            is_symlink: false,
            size: String::new(),
            size_bytes: 0,
            modified_at: None,
            attributes: String::new(),
        });
    }

    // One metadata() call per entry (one stat syscall), two for a symlink
    for entry in &entries {
        let entry_path = entry.path();
        // file_type() comes from readdir's d_type - no syscall on Linux.
        let is_symlink = entry.file_type().map(|file_type| file_type.is_symlink()).unwrap_or(false);
        // DirEntry::metadata() describes the link itself, which would list a link
        // to a directory as a file: sorted among the files, sized in bytes and
        // impossible to enter. Follow it, and fall back to the link when the
        // target is missing so a broken link still lists as an ordinary entry.
        let metadata = if is_symlink { fs::metadata(&entry_path).or_else(|_| entry.metadata()).ok() } else { entry.metadata().ok() };
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let name_os = entry.file_name();
        let name_full = name_os.to_string_lossy().into_owned();
        if !options.show_hidden && is_hidden(&name_full, metadata.as_ref()) {
            continue;
        }
        // Lossy like the name above, and for the same reason: a name that is
        // not valid UTF-8 still has to be shown as something. to_str() gave
        // None for those, which left the column blank.
        let name = if is_dir { name_full.clone() } else { entry_path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
        let extension = if is_dir { String::new() } else { entry_path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default() };
        let size_bytes = if is_dir { 0 } else { metadata.as_ref().map(|m| m.len()).unwrap_or(0) };
        let size = if is_dir { "<DIR>".to_string() } else { format_size(size_bytes) };
        // Written out when drawn, in whichever format F11 names - and a
        // relative date has to be, or it would go stale between reloads.
        let modified_at = metadata.as_ref().and_then(|m| m.modified().ok());

        let attributes = metadata.as_ref().map(|metadata| format_attributes(metadata, is_dir, is_symlink)).unwrap_or_default();

        children.push(Item {
            name_os,
            name_full,
            name,
            extension,
            is_dir,
            is_symlink,
            size,
            size_bytes,
            modified_at,
            attributes,
        });
    }

    // Sort items on already-computed fields (no stat syscalls during sort)
    let sort_start = usize::from(has_parent);
    children[sort_start..].sort_by(|a, b| compare_rows(a, b, options));

    Ok(children)
}

/// Directories before files whichever way the rest runs, unless F11 mixes
/// them in, then the chosen key, with the name settling ties so equal sizes or
/// dates still list steadily.
fn compare_rows(a: &Item, b: &Item, options: &Options) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if options.dirs_first {
        match (a.is_dir, b.is_dir) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }

    let text = |a: &str, b: &str| if options.case_sensitive { a.cmp(b) } else { a.to_lowercase().cmp(&b.to_lowercase()) };
    let name = || text(&a.name_full, &b.name_full);
    let order = match options.sort_key {
        SortKey::Name => name(),
        SortKey::Extension => text(&a.extension, &b.extension).then_with(name),
        SortKey::Size => a.size_bytes.cmp(&b.size_bytes).then_with(name),
        SortKey::Modified => a.modified_at.cmp(&b.modified_at).then_with(name),
    };
    if options.sort_descending { order.reverse() } else { order }
}

/// The branch checked out in the repository `dir` is in, or the start of
/// the commit when none is. Read from the files, as a prompt does, rather
/// than by running git: up from `dir` to the first `.git`, a directory or,
/// in a worktree or submodule, a file naming one. None outside a repository.
pub fn git_branch(dir: &Path) -> Option<String> {
    for ancestor in dir.ancestors() {
        let dot_git = ancestor.join(".git");
        let Ok(metadata) = fs::metadata(&dot_git) else {
            continue;
        };
        let git_dir = if metadata.is_dir() {
            dot_git
        } else {
            let text = fs::read_to_string(&dot_git).ok()?;
            ancestor.join(text.trim().strip_prefix("gitdir:")?.trim())
        };
        let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
        let head = head.trim();
        return Some(match head.strip_prefix("ref:") {
            Some(reference) => {
                let reference = reference.trim();
                reference.strip_prefix("refs/heads/").unwrap_or(reference).to_string()
            }
            None => head.chars().take(7).collect(),
        });
    }
    None
}

/// Who fm84 runs as and on what machine, `colt@laserbeak`, and whether that
/// is root - worth seeing before a delete.
#[cfg(unix)]
pub fn user_at_host() -> (String, bool) {
    // SAFETY: geteuid cannot fail, and gethostname writes at most the length
    // it is given into a buffer of ours, NUL-terminated when it fits.
    let uid = unsafe { libc::geteuid() };
    let mut buffer = [0 as libc::c_char; 256];
    let host = if unsafe { libc::gethostname(buffer.as_mut_ptr(), buffer.len() - 1) } == 0 {
        // SAFETY: the last byte was never written, so the string ends by it.
        unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
    } else {
        String::new()
    };
    // A domain on the end says nothing a glance needs.
    let host = host.split('.').next().unwrap_or_default().to_string();
    (format!("{}@{}", user_name(uid), host), uid == 0)
}

#[cfg(not(unix))]
pub fn user_at_host() -> (String, bool) {
    let variable = |name: &str| std::env::var(name).unwrap_or_default();
    (format!("{}@{}", variable("USERNAME"), variable("COMPUTERNAME")), false)
}

pub fn calculate_dir_size(path: &Path) -> Result<u64, Error> {
    walk_dir_size(path, &AtomicBool::new(false), &mut |_| {}).map(Option::unwrap_or_default)
}

/// Bytes in the files under `path`, links not followed. `found` is told each
/// file's size as it is reached, so a long walk can show how far it has got,
/// and `cancel` stops it between entries, which gives None.
///
/// Only `path` itself has to be readable. Anything inside that is not - a
/// directory locked to its owner, a file gone mid-walk - counts as nothing,
/// the way du carries on past what it cannot read rather than giving up.
pub fn walk_dir_size(path: &Path, cancel: &AtomicBool, found: &mut dyn FnMut(u64)) -> Result<Option<u64>, Error> {
    let mut total = 0u64;
    for entry in read_dir(path)? {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Ok(entry) = entry else { continue };
        // file_type() uses readdir's d_type on Linux - no extra stat syscall
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() {
            match walk_dir_size(&entry.path(), cancel, found) {
                Ok(Some(size)) => total += size,
                Ok(None) => return Ok(None),
                Err(_) => {}
            }
        } else if let Ok(metadata) = entry.metadata() {
            total += metadata.len();
            found(metadata.len());
        }
    }
    Ok(Some(total))
}

/// The parts of an entry the columns have no room for: its exact size, the
/// second on its timestamp, who owns it, and where it points if it is a link.
pub struct Description {
    pub size_bytes: Option<u64>,
    pub modified: Option<std::time::SystemTime>,
    pub owner: String,
    pub attributes: String,
    /// Where a symlink points. Listed but never shown until now, and it is the
    /// one thing about a link worth knowing.
    pub link: Option<String>,
}

/// Look one entry up. Costs a stat, a readlink for a link, and the owner's
/// name - which is why it is done when the cursor moves rather than on the way
/// to drawing a frame, and on a thread of its own, where a dead mount holds
/// up nothing.
pub fn describe(path: &Path, is_dir: bool) -> Option<Description> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let is_symlink = metadata.file_type().is_symlink();
    // A link's own size is the length of the path it holds, which tells nobody
    // anything; follow it for the figure. A broken one has no figure at all,
    // and saying 31 bytes would be worse than saying nothing.
    let followed = if is_symlink { fs::metadata(path).ok() } else { None };
    let size_bytes = if is_symlink { followed.as_ref().map(|target| target.len()) } else { (!is_dir).then_some(metadata.len()) };

    Some(Description {
        size_bytes,
        modified: metadata.modified().ok(),
        owner: owner_of(&metadata),
        // The link's own bits, as ls -l writes them, not the target's.
        attributes: format_attributes(&metadata, is_dir, is_symlink),
        link: is_symlink.then(|| fs::read_link(path).ok()).flatten().map(|target| target.display().to_string()),
    })
}
