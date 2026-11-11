//! The system's trash read as a folder: `~/@trash`, which is not on the
//! disk, as a git commit's folder is not. What went to the trash is listed
//! by the name it had, each file read from where the trash keeps it.
//!
//! Under test the trash is a stand-in of the test's own, so no test ever
//! lists, restores or empties the trash of whoever runs them.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// The trash folder's name, in the home directory.
pub const TRASH_NAME: &str = "@trash";

/// Where the trash folder is: `@trash` in the home directory.
pub fn trash_folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(TRASH_NAME))
}

/// Whether `path` is the trash folder itself.
pub fn is_trash_folder(path: &Path) -> bool {
    path.file_name() == Some(OsStr::new(TRASH_NAME)) && trash_folder().is_some_and(|folder| folder == path)
}

/// When the trash last changed - something put in, put back or emptied -
/// as the latest time of the directories that change with it, for the
/// panel showing it to be read again. The folder itself is not on the disk
/// to have a time of its own.
pub fn changed() -> Option<std::time::SystemTime> {
    system::watched().iter().filter_map(|dir| std::fs::metadata(dir).and_then(|metadata| metadata.modified()).ok()).max()
}

/// Whether what is in the trash can be put back where it came from: not on
/// macOS, which does not say where that was.
pub fn can_put_back() -> bool {
    cfg!(test) || !cfg!(target_os = "macos")
}

/// Why it cannot.
pub fn cannot_put_back() -> &'static str {
    t!("macOS does not say where things in the trash came from: copy it out with F5")
}

/// One thing in the trash: what the system knows it by, the name it had and
/// where it was, when it went, and where its bytes are now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trashed {
    pub id: OsString,
    pub name: OsString,
    pub original: PathBuf,
    /// Seconds since the epoch.
    pub deleted: i64,
    pub real: PathBuf,
}

/// A failure the system gave the trash, as the error it is: its kind -
/// denied, not found - and the system's own words for it. On Windows the
/// code is an HRESULT, a Win32 error wrapped as `0x8007xxxx`; anything else
/// there is left to the description, which the crate starts with a tag.
#[cfg(any(test, windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
fn os_error(code: i32, description: String) -> io::Error {
    let raw = if cfg!(windows) { win32_code(code) } else { Some(code).filter(|&code| code > 0) };
    match raw {
        Some(raw) => io::Error::from_raw_os_error(raw),
        None => io::Error::other(description.strip_prefix("windows error: ").map(str::to_string).unwrap_or(description)),
    }
}

/// The Win32 error inside an HRESULT that wraps one.
#[cfg(any(test, windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
fn win32_code(hresult: i32) -> Option<i32> {
    let hresult = hresult as u32;
    (hresult & 0xFFFF_0000 == 0x8007_0000).then_some((hresult & 0xFFFF) as i32)
}

#[cfg(all(not(test), any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))))]
mod system {
    use super::*;

    fn error(error: trash::Error) -> io::Error {
        match error {
            trash::Error::RestoreCollision { path, .. } => io::Error::new(io::ErrorKind::AlreadyExists, t!("{path} already exists", path = path.display())),
            // Only the freedesktop trash has this one: not on Windows.
            #[cfg(unix)]
            trash::Error::FileSystem { source, .. } => source,
            trash::Error::Os { code, description } => os_error(code, description),
            trash::Error::Unknown { description } => io::Error::other(description),
            other => io::Error::other(format!("{other:?}")),
        }
    }

    /// Where the trash keeps an item's bytes. On Linux its id is its
    /// `.trashinfo` file, `Trash/info/<name>.trashinfo`, and the item is
    /// `Trash/files/<name>`; on Windows its id is where it is.
    fn real_path(item: &trash::TrashItem) -> PathBuf {
        let id = Path::new(&item.id);
        if cfg!(windows) {
            return id.to_path_buf();
        }
        let stored = id.file_name().map(|name| name.as_encoded_bytes().strip_suffix(b".trashinfo").unwrap_or(name.as_encoded_bytes()).to_vec()).unwrap_or_default();
        // SAFETY: cut from an OsStr's own bytes at an ASCII boundary.
        let stored = unsafe { OsString::from_encoded_bytes_unchecked(stored) };
        id.parent().and_then(Path::parent).map(|trash| trash.join("files").join(stored)).unwrap_or_default()
    }

    pub fn list() -> io::Result<Vec<Trashed>> {
        let items = trash::os_limited::list().map_err(error)?;
        Ok(items
            .into_iter()
            .map(|item| Trashed {
                real: real_path(&item),
                original: item.original_parent.clone(),
                id: item.id,
                name: item.name,
                deleted: item.time_deleted,
            })
            .collect())
    }

    fn find(id: &OsStr) -> io::Result<trash::TrashItem> {
        trash::os_limited::list()
            .map_err(error)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("No longer in the trash")))
    }

    pub fn restore(id: &OsStr) -> io::Result<()> {
        trash::os_limited::restore_all([find(id)?]).map_err(error)
    }

    /// Each trash's `info` directory - the home one's and each drive's -
    /// which gains a file for each thing put in and loses it as it goes.
    #[cfg(unix)]
    pub fn watched() -> Vec<PathBuf> {
        trash::os_limited::trash_folders().map(|folders| folders.into_iter().map(|folder| folder.join("info")).collect()).unwrap_or_default()
    }

    /// Each drive's recycle bin, a folder in it for each user, whose time
    /// moves as things go in and out.
    #[cfg(windows)]
    pub fn watched() -> Vec<PathBuf> {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\$Recycle.Bin", letter as char)))
            .filter_map(|bin| std::fs::read_dir(bin).ok())
            .flat_map(|users| users.flatten().map(|user| user.path()))
            .collect()
    }

    pub fn purge(id: &OsStr) -> io::Result<()> {
        trash::os_limited::purge_all([find(id)?]).map_err(error)
    }
}

/// The trash on macOS: `~/.Trash`, and `.Trashes/<uid>` on each other
/// drive, read as the folders they are. Nothing there says where each thing
/// came from - the Finder keeps that to itself - so it can be looked at,
/// copied out and emptied, but not put back. Reading the home one at all
/// takes Full Disk Access for the terminal fm84 runs in.
#[cfg(all(not(test), target_os = "macos"))]
mod system {
    use super::*;

    /// The home trash first, then each drive's, those there.
    fn trashes() -> Vec<PathBuf> {
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let home = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".Trash"));
        let drives = std::fs::read_dir("/Volumes").into_iter().flatten().flatten().map(|drive| drive.path().join(".Trashes").join(uid.to_string()));
        home.into_iter().chain(drives).filter(|trash| trash.is_dir()).collect()
    }

    /// What the Finder keeps in a trash for itself, not anything thrown away.
    fn is_finders(name: &OsStr) -> bool {
        name == ".DS_Store" || name == ".localized"
    }

    pub fn list() -> io::Result<Vec<Trashed>> {
        use std::os::unix::fs::MetadataExt;
        let mut found = Vec::new();
        for (at, trash) in trashes().into_iter().enumerate() {
            let entries = match std::fs::read_dir(&trash) {
                Ok(entries) => entries,
                // The home trash is the one that says why; a drive's that
                // cannot be read is passed over.
                Err(error) if at == 0 && error.kind() == io::ErrorKind::PermissionDenied => {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, t!("Reading the trash needs Full Disk Access for this terminal, in System Settings, Privacy & Security")));
                }
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if is_finders(&name) {
                    continue;
                }
                let real = entry.path();
                // Moved in, a thing's status changes: as near as there is to
                // when it went.
                let deleted = std::fs::symlink_metadata(&real).map(|metadata| metadata.ctime()).unwrap_or(0);
                found.push(Trashed {
                    id: real.clone().into_os_string(),
                    name,
                    original: PathBuf::new(),
                    deleted,
                    real,
                });
            }
        }
        Ok(found)
    }

    /// The thing in the trash that `id` - where it is - still names.
    fn find(id: &OsStr) -> io::Result<PathBuf> {
        let path = PathBuf::from(id);
        if path.symlink_metadata().is_err() {
            return Err(io::Error::new(io::ErrorKind::NotFound, t!("No longer in the trash")));
        }
        Ok(path)
    }

    pub fn restore(_id: &OsStr) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, cannot_put_back()))
    }

    pub fn purge(id: &OsStr) -> io::Result<()> {
        let path = find(id)?;
        if path.symlink_metadata()?.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
    }

    /// The trashes themselves, which change as things go in and out.
    pub fn watched() -> Vec<PathBuf> {
        trashes()
    }
}

/// Elsewhere the trash can be put into but not looked through.
#[cfg(all(not(test), not(any(windows, target_os = "macos", all(unix, not(target_os = "ios"), not(target_os = "android"))))))]
mod system {
    use super::*;

    fn unsupported() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, t!("The trash cannot be looked into on this system"))
    }

    pub fn list() -> io::Result<Vec<Trashed>> {
        Err(unsupported())
    }

    pub fn restore(_id: &OsStr) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn purge(_id: &OsStr) -> io::Result<()> {
        Err(unsupported())
    }

    pub fn watched() -> Vec<PathBuf> {
        Vec::new()
    }
}

pub use system::{list, purge, restore};

/// The stand-in under test: items whose bytes are in a directory of the
/// test's own, restored by a rename and purged by a delete.
#[cfg(test)]
pub mod system {
    use super::*;
    use std::sync::Mutex;

    static ITEMS: Mutex<Vec<Trashed>> = Mutex::new(Vec::new());

    /// `path` put in the stand-in trash, its bytes moved into `keep`.
    pub fn throw(path: &Path, keep: &Path, deleted: i64) {
        *KEPT.lock().unwrap() = Some(keep.to_path_buf());
        let mut items = ITEMS.lock().unwrap();
        let real = keep.join(format!("{}", items.len()));
        std::fs::rename(path, &real).unwrap();
        items.push(Trashed {
            id: real.clone().into_os_string(),
            name: path.file_name().unwrap().to_os_string(),
            original: path.parent().unwrap().to_path_buf(),
            deleted,
            real,
        });
    }

    pub fn list() -> io::Result<Vec<Trashed>> {
        Ok(ITEMS.lock().unwrap().clone())
    }

    fn take(id: &OsStr) -> io::Result<Trashed> {
        let mut items = ITEMS.lock().unwrap();
        let at = items.iter().position(|item| item.id == id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("No longer in the trash")))?;
        Ok(items.remove(at))
    }

    pub fn restore(id: &OsStr) -> io::Result<()> {
        let item = take(id)?;
        let to = item.original.join(&item.name);
        if to.symlink_metadata().is_ok() {
            ITEMS.lock().unwrap().push(item);
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, t!("{path} already exists", path = to.display())));
        }
        std::fs::rename(&item.real, to)
    }

    /// The directory the stand-in keeps its bytes in, which changes as
    /// they come and go.
    static KEPT: Mutex<Option<PathBuf>> = Mutex::new(None);

    pub fn watched() -> Vec<PathBuf> {
        KEPT.lock().unwrap().iter().cloned().collect()
    }

    pub fn purge(id: &OsStr) -> io::Result<()> {
        let item = take(id)?;
        if item.real.is_dir() { std::fs::remove_dir_all(&item.real) } else { std::fs::remove_file(&item.real) }
    }
}

#[cfg(test)]
mod os_error_tests {
    use super::*;

    /// A Win32 error comes out of its HRESULT; anything else is no Win32
    /// error, and is left to the description, its tag taken off.
    #[test]
    fn a_systems_failure_reads_as_the_error_it_is() {
        // E_ACCESSDENIED, ERROR_FILE_NOT_FOUND wrapped.
        assert_eq!(win32_code(0x8007_0005_u32 as i32), Some(5));
        assert_eq!(win32_code(0x8007_0002_u32 as i32), Some(2));
        // E_FAIL and S_OK wrap none.
        assert_eq!(win32_code(0x8000_4005_u32 as i32), None);
        assert_eq!(win32_code(0), None);
        if !cfg!(windows) {
            assert_eq!(os_error(2, String::new()).kind(), io::ErrorKind::NotFound);
            let told = os_error(0, "windows error: it broke".to_string());
            assert_eq!(told.to_string(), "it broke");
        }
    }
}
