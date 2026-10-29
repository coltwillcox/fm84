//! What an archive holds, read once into a tree that can be listed a
//! directory at a time.

use super::{Kind, open, tar_reader, zip_error};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Dir,
    File,
    /// A symbolic link. Listed and recreated as one, never followed.
    Link,
    /// A tar entry that is another name for one before it.
    HardLink,
    /// A device, a pipe: nothing to read out of the archive.
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Where it is, from the archive's root.
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub mode: Option<u32>,
    /// `user:group`, which a tar keeps and a zip does not.
    pub owner: Option<String>,
    /// Where a link points: as written for a symbolic link, from the root for
    /// a hard link. A zip keeps a link's target as its contents, so it is
    /// only known once read.
    pub link: Option<PathBuf>,
    /// Which entry of the archive it is: the index in a zip, the count from
    /// the start in a tar. None for a directory only implied by the names
    /// under it, which has no entry of its own.
    pub position: Option<usize>,
    pub encrypted: bool,
}

impl Entry {
    fn implied(path: PathBuf) -> Self {
        Entry {
            path,
            kind: EntryKind::Dir,
            size: 0,
            modified: None,
            mode: None,
            owner: None,
            link: None,
            position: None,
            encrypted: false,
        }
    }

    pub fn name(&self) -> &std::ffi::OsStr {
        self.path.file_name().unwrap_or_default()
    }
}

/// An archive's entries as a tree.
#[derive(Debug)]
pub struct Index {
    pub kind: Kind,
    entries: Vec<Entry>,
    by_path: HashMap<PathBuf, usize>,
    /// What each directory holds, by entry; the root is the empty path.
    children: HashMap<PathBuf, Vec<usize>>,
    /// Entries left out: a name that would reach outside the archive - with
    /// `..` in it, or a drive - or one that would have to be written through
    /// a link, or over a directory with a file.
    pub refused: usize,
}

impl Index {
    fn new(kind: Kind) -> Self {
        Index {
            kind,
            entries: Vec::new(),
            by_path: HashMap::new(),
            children: HashMap::new(),
            refused: 0,
        }
    }

    pub fn entry(&self, at: usize) -> &Entry {
        &self.entries[at]
    }

    /// The entry at `inner`. None for the root, which has none.
    pub fn find(&self, inner: &Path) -> Option<usize> {
        self.by_path.get(inner).copied()
    }

    /// True for the root, and for any directory in the archive.
    pub fn is_dir(&self, inner: &Path) -> bool {
        inner.as_os_str().is_empty() || self.find(inner).is_some_and(|at| self.entries[at].kind == EntryKind::Dir)
    }

    /// What the directory at `inner` holds.
    pub fn children(&self, inner: &Path) -> impl Iterator<Item = &Entry> {
        self.children.get(inner).into_iter().flatten().map(|&at| &self.entries[at])
    }

    /// The entry at `inner` and everything under it, parents before what they
    /// hold; everything in the archive for the root.
    pub fn subtree(&self, inner: &Path) -> Vec<usize> {
        let mut found: Vec<usize> = Vec::new();
        let mut stack: Vec<PathBuf> = Vec::new();
        match self.find(inner) {
            Some(at) => {
                found.push(at);
                if self.entries[at].kind == EntryKind::Dir {
                    stack.push(inner.to_path_buf());
                }
            }
            None if inner.as_os_str().is_empty() => stack.push(PathBuf::new()),
            None => return found,
        }
        while let Some(dir) = stack.pop() {
            for &at in self.children.get(&dir).into_iter().flatten() {
                found.push(at);
                if self.entries[at].kind == EntryKind::Dir {
                    stack.push(self.entries[at].path.clone());
                }
            }
        }
        found
    }

    /// The bytes in the files at and under `inner`, as the archive gives
    /// them - instantly, with no walk.
    pub fn size_of(&self, inner: &Path) -> u64 {
        self.subtree(inner).iter().map(|&at| &self.entries[at]).filter(|entry| entry.kind == EntryKind::File).map(|entry| entry.size).sum()
    }

    /// Put an entry in its place in the tree, making the directories above it
    /// that the archive only implies. A later entry of the same name replaces
    /// an earlier one, as extracting a tar would.
    fn add(&mut self, entry: Entry) {
        let mut parent = PathBuf::new();
        if let Some(above) = entry.path.parent() {
            for component in above.components() {
                let next = parent.join(component);
                match self.by_path.get(&next) {
                    Some(&at) if self.entries[at].kind == EntryKind::Dir => {}
                    // Under a file, or under a link - where writing it out
                    // would go through the link to wherever that points.
                    Some(_) => {
                        self.refused += 1;
                        return;
                    }
                    None => self.insert(Entry::implied(next.clone())),
                }
                parent = next;
            }
        }
        match self.by_path.get(&entry.path) {
            Some(&at) => {
                let existing = &mut self.entries[at];
                match (existing.kind, entry.kind) {
                    // A directory implied first and given later, or given twice.
                    (EntryKind::Dir, EntryKind::Dir) => {
                        existing.modified = entry.modified;
                        existing.mode = entry.mode;
                        existing.owner = entry.owner;
                        existing.position = entry.position;
                    }
                    // A file and a directory at one name: whichever came
                    // first stays, and what is under the directory with it.
                    (EntryKind::Dir, _) | (_, EntryKind::Dir) => self.refused += 1,
                    _ => *existing = entry,
                }
            }
            None => self.insert(entry),
        }
    }

    fn insert(&mut self, entry: Entry) {
        let at = self.entries.len();
        let parent = entry.path.parent().map(Path::to_path_buf).unwrap_or_default();
        self.children.entry(parent).or_default().push(at);
        self.by_path.insert(entry.path.clone(), at);
        self.entries.push(entry);
    }

    pub(super) fn read(archive: &Path, kind: Kind) -> io::Result<Self> {
        match kind {
            Kind::Zip => Self::read_zip(archive),
            Kind::Tar | Kind::TarGz => Self::read_tar(archive, kind),
        }
    }

    fn read_zip(archive: &Path) -> io::Result<Self> {
        let mut index = Index::new(Kind::Zip);
        let mut zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
        for position in 0..zip.len() {
            // Raw: nothing is decompressed or decrypted to list it.
            let file = zip.by_index_raw(position).map_err(zip_error)?;
            let Some(path) = file.name().ok().and_then(|name| inner_path(name.as_bytes())) else {
                index.refused += 1;
                continue;
            };
            if path.as_os_str().is_empty() {
                continue;
            }
            let kind = if file.is_dir() {
                EntryKind::Dir
            } else if file.is_symlink() {
                EntryKind::Link
            } else {
                EntryKind::File
            };
            index.add(Entry {
                path,
                kind,
                size: if kind == EntryKind::File { file.size() } else { 0 },
                modified: file.last_modified().and_then(zip_time),
                mode: file.unix_mode(),
                owner: None,
                link: None,
                position: Some(position),
                encrypted: file.encrypted(),
            });
        }
        Ok(index)
    }

    fn read_tar(archive: &Path, kind: Kind) -> io::Result<Self> {
        let mut index = Index::new(kind);
        let mut tar = tar::Archive::new(tar_reader(archive, kind)?);
        let mut read_any = false;
        for (position, entry) in tar.entries()?.enumerate() {
            // Cut short, or damaged past some point: what came before it is
            // still worth showing. Nothing at all is an archive that is not.
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if !read_any => return Err(error),
                Err(_) => break,
            };
            read_any = true;
            let header = entry.header();
            let Some(path) = inner_path(&entry.path_bytes()) else {
                index.refused += 1;
                continue;
            };
            if path.as_os_str().is_empty() {
                continue;
            }
            use tar::EntryType;
            let kind = match header.entry_type() {
                EntryType::Directory => EntryKind::Dir,
                EntryType::Symlink => EntryKind::Link,
                EntryType::Link => EntryKind::HardLink,
                EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => EntryKind::File,
                _ => EntryKind::Other,
            };
            let link = entry.link_name_bytes().map(|bytes| match kind {
                // A hard link names another entry, and is only followed to one.
                EntryKind::HardLink => inner_path(&bytes).unwrap_or_default(),
                _ => path_from_bytes(&bytes),
            });
            let owner = match (header.username(), header.groupname()) {
                (Ok(Some(user)), Ok(Some(group))) if !user.is_empty() => Some(format!("{user}:{group}")),
                _ => None,
            };
            index.add(Entry {
                path,
                kind,
                size: if kind == EntryKind::File { entry.size() } else { 0 },
                modified: header.mtime().ok().map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
                mode: header.mode().ok(),
                owner,
                link,
                position: Some(position),
                encrypted: false,
            });
        }
        Ok(index)
    }
}

/// A zip keeps local time, to the even second, with no zone.
fn zip_time(time: zip::DateTime) -> Option<SystemTime> {
    use chrono::TimeZone;
    let date = chrono::NaiveDate::from_ymd_opt(time.year().into(), time.month().into(), time.day().into())?;
    let at = date.and_hms_opt(time.hour().into(), time.minute().into(), time.second().into())?;
    chrono::Local.from_local_datetime(&at).earliest().map(SystemTime::from)
}

/// An entry's name as a path from the archive's root, or None for one that
/// must not be written anywhere: `..`, or a part that is not a plain name on
/// this system - a drive, a separator of its own. A leading `/` is dropped,
/// as tar does. Empty for the root itself.
pub(super) fn inner_path(raw: &[u8]) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in raw.split(|&byte| byte == b'/') {
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." {
            return None;
        }
        let name = os_from_bytes(part)?;
        let mut components = Path::new(&name).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(only)), None) if only == name => path.push(only),
            _ => return None,
        }
    }
    Some(path)
}

/// A link's target, kept as written.
pub(super) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    os_from_bytes(bytes).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()))
}

#[cfg(unix)]
fn os_from_bytes(bytes: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt;
    Some(std::ffi::OsStr::from_bytes(bytes).to_os_string())
}

/// Elsewhere a name is text, and one that is not UTF-8 cannot be made.
#[cfg(not(unix))]
fn os_from_bytes(bytes: &[u8]) -> Option<OsString> {
    std::str::from_utf8(bytes).ok().map(OsString::from)
}
