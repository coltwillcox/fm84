//! Archives read as directories. A panel goes into one by a path that runs
//! through it - `/home/me/photos.zip/2024/june` - so tabs, history, bookmarks
//! and the session file all go on holding a plain path, and only what reads
//! the disk has to know that part of it lies inside a file.
//!
//! Read-only: nothing here writes into an archive.

#[cfg(test)]
pub mod fixtures;
mod index;
mod iso;
#[cfg(test)]
mod tests;
pub mod trash;

pub(crate) use index::inner_path;
pub use index::{Entry, EntryKind, Index};

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The kinds of archive that can be gone into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Zip,
    Tar,
    TarGz,
    TarZst,
    TarXz,
    TarBz2,
    SevenZ,
    /// A git commit - a branch, a tag, a stash - as its tree was: a folder
    /// at `<top of the repository>/@git:<revision>`, which is not on the disk.
    Git,
    /// The system's trash, at `~/@trash`, which is not on the disk either.
    Trash,
    /// A Debian package: an ar of tars, each tar a folder named for it.
    Deb,
    /// A CD or DVD image, ISO 9660, with the long names Rock Ridge or
    /// Joliet give it.
    Iso,
}

/// Zips by other names: programs, books, comics and office documents. Gone
/// into as a zip is, and read-only: rewritten, a signed .apk or .jar would
/// lose its signature, and an .epub the order its first entry must keep.
const ZIP_ALIKES: &[&str] = &[".jar", ".war", ".apk", ".aab", ".xpi", ".whl", ".epub", ".cbz", ".docx", ".xlsx", ".pptx", ".odt", ".ods", ".odp"];

/// Whether `path` is a zip by its name, .zip itself - one that can be
/// written into.
pub fn named_zip(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(".zip"))
}

/// What a git commit's folder is named with, the revision after it.
pub const GIT_PREFIX: &str = "@git:";

/// The folder for `revision` of the repository whose top is `top`. A slash
/// in a revision - `feature/tabs` - would make it two folders, so it is
/// written `%2F`, and a `%` as `%25`.
pub fn git_folder(top: &Path, revision: &str) -> PathBuf {
    top.join(format!("{GIT_PREFIX}{}", revision.replace('%', "%25").replace('/', "%2F")))
}

/// The repository and the revision a commit's folder stands for.
pub fn git_revision(folder: &Path) -> Option<(PathBuf, String)> {
    let name = folder.file_name()?.to_str()?;
    let revision = name.strip_prefix(GIT_PREFIX).filter(|revision| !revision.is_empty())?;
    Some((folder.parent()?.to_path_buf(), revision.replace("%2F", "/").replace("%2f", "/").replace("%25", "%")))
}

/// The kind of archive a file is, by its name. The name is only a claim:
/// what is not one fails to read, and says so.
pub fn kind_of(path: &Path) -> Option<Kind> {
    if git_revision(path).is_some() {
        return Some(Kind::Git);
    }
    if trash::is_trash_folder(path) {
        return Some(Kind::Trash);
    }
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let named = |endings: &[&str]| endings.iter().any(|ending| name.ends_with(ending));
    if named(&[".zip"]) || named(ZIP_ALIKES) {
        Some(Kind::Zip)
    } else if named(&[".deb", ".udeb"]) {
        Some(Kind::Deb)
    } else if named(&[".iso"]) {
        Some(Kind::Iso)
    } else if named(&[".7z"]) {
        Some(Kind::SevenZ)
    } else if named(&[".tar.gz", ".tgz"]) {
        Some(Kind::TarGz)
    } else if named(&[".tar.zst", ".tzst"]) {
        Some(Kind::TarZst)
    } else if named(&[".tar.xz", ".txz"]) {
        Some(Kind::TarXz)
    } else if named(&[".tar.bz2", ".tbz2", ".tbz"]) {
        Some(Kind::TarBz2)
    } else if named(&[".tar"]) {
        Some(Kind::Tar)
    } else {
        None
    }
}

/// A path that runs through an archive: the archive file, and where inside
/// it, from its root - empty for the root itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub archive: PathBuf,
    pub inner: PathBuf,
}

/// The archive `path` is in, or is. None for anything on the disk itself -
/// a directory, or a file that is not an archive - and for a path that leads
/// nowhere. It stats its way up from `path` until something is there, so it
/// belongs on a thread wherever a dead mount could be in the way.
pub fn locate(path: &Path) -> Option<Located> {
    // A separator on the end - `box.zip/`, typed as a directory is - would
    // have the disk say the archive is not a directory, and the climb go
    // past it. Put together again from its parts, the path has none.
    let path: PathBuf = path.components().collect();
    let mut inner = Vec::new();
    let mut here = path.as_path();
    loop {
        // A commit's folder is not on the disk, but the repository it is in
        // is; nor is the trash's.
        if (git_revision(here).is_some() && here.parent().is_some_and(Path::is_dir)) || trash::is_trash_folder(here) {
            return Some(Located {
                archive: here.to_path_buf(),
                inner: inner.iter().rev().collect(),
            });
        }
        match fs::metadata(here) {
            Ok(metadata) if metadata.is_file() && kind_of(here).is_some() => {
                return Some(Located {
                    archive: here.to_path_buf(),
                    inner: inner.iter().rev().collect(),
                });
            }
            // Something real, and not an archive: nothing under it can be in one.
            Ok(_) => return None,
            Err(_) => {}
        }
        inner.push(here.file_name()?.to_os_string());
        here = here.parent()?;
    }
}

/// Indexes read lately, so going in and out of a directory inside a large
/// .tar.gz does not decompress the whole of it each time. Keyed by the
/// archive's size, modified time and - on Unix - inode as well as its path:
/// one written since is read again. The time alone is too coarse for that:
/// one written over within the same tick of the clock, the same size, has
/// the same one. Written by a rename, as fm84 and most tools write one, it
/// is another inode.
const CACHED: usize = 4;
type Key = (PathBuf, Option<SystemTime>, u64, u64);

#[cfg(unix)]
fn inode(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
fn inode(_metadata: &fs::Metadata) -> u64 {
    0
}

/// Let go of what is kept of `archive`, by whatever path it was read: it
/// has just been written, and is to be read again.
pub fn forget_index(archive: &Path) {
    let real = fs::canonicalize(archive).ok();
    CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|((path, ..), _)| path != archive && (real.is_none() || fs::canonicalize(path).ok() != real));
}
static CACHE: Mutex<Vec<(Key, Arc<Index>)>> = Mutex::new(Vec::new());

/// The index of `archive`, read on first asking.
pub fn index(archive: &Path) -> io::Result<Arc<Index>> {
    let kind = kind_of(archive).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t!("Not an archive: {path}", path = archive.display())))?;
    // A commit never changes; a branch moves on, and is read again once it
    // names another commit.
    // The trash changes whenever anything goes to it: read afresh when what
    // is in it is not what was.
    if kind == Kind::Trash {
        let items = trash::list()?;
        let hash = items.iter().fold(items.len() as u64, |hash, item| {
            item.id
                .as_encoded_bytes()
                .iter()
                .fold(hash.wrapping_mul(31).wrapping_add(item.deleted as u64), |hash, &byte| hash.wrapping_mul(31).wrapping_add(u64::from(byte)))
        });
        let key = (archive.to_path_buf(), None, hash, items.len() as u64);
        if let Some((_, index)) = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(cached, _)| *cached == key) {
            return Ok(Arc::clone(index));
        }
        let index = Arc::new(Index::read_trash(&items));
        let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.retain(|(cached, _)| cached.0 != key.0);
        cache.push((key, Arc::clone(&index)));
        if cache.len() > CACHED {
            cache.remove(0);
        }
        return Ok(index);
    }
    let (key, commit) = match git_revision(archive) {
        Some((top, revision)) => {
            let commit = crate::git::resolve(&top, &revision).map_err(io::Error::other)?;
            let hash = commit.bytes().fold(0u64, |hash, byte| hash.wrapping_mul(31).wrapping_add(u64::from(byte)));
            ((archive.to_path_buf(), None, hash, 0), Some((top, commit)))
        }
        None => {
            let metadata = fs::metadata(archive)?;
            ((archive.to_path_buf(), metadata.modified().ok(), metadata.len(), inode(&metadata)), None)
        }
    };
    if let Some((_, index)) = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(cached, _)| *cached == key) {
        return Ok(Arc::clone(index));
    }
    // Read without the lock held: another archive asked for meanwhile need
    // not wait on this one.
    let index = Arc::new(match commit {
        Some((top, commit)) => Index::read_git(&top, &commit)?,
        None => Index::read(archive, kind)?,
    });
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.retain(|(cached, _)| cached.0 != key.0);
    cache.push((key, Arc::clone(&index)));
    if cache.len() > CACHED {
        cache.remove(0);
    }
    Ok(index)
}

/// The entry a path names, for whoever has only the path: the preview, the
/// detail lines, the question about a file in the way. None when it does not
/// run through an archive, or names nothing in it.
pub fn entry_at(path: &Path) -> Option<(Arc<Index>, usize)> {
    let located = locate(path)?;
    let index = index(&located.archive).ok()?;
    let at = index.find(&located.inner)?;
    Some((index, at))
}

/// Where a link points, from the bytes an archive keeps for it.
pub fn link_target(bytes: &[u8]) -> PathBuf {
    index::path_from_bytes(bytes)
}

/// What the archive's entries are read through.
fn open(archive: &Path) -> io::Result<BufReader<File>> {
    Ok(BufReader::new(File::open(archive)?))
}

pub(crate) fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(error) => error,
        zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED) => encrypted(),
        zip::result::ZipError::InvalidPassword => wrong_password(),
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// What a visit to an entry asks for next.
pub enum Next {
    /// On to the next one.
    Go,
    /// This one again from its start - after a disk was freed, say.
    Again,
    /// No more.
    Stop,
}

/// An archive that wants a password fm84 has not got - none given, or
/// the one given not right. Carried inside an io::Error, so what asks for
/// one can tell it from any other failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locked {
    pub wrong: bool,
}

impl std::fmt::Display for Locked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.wrong { t!("Wrong password") } else { t!("Encrypted: it needs its password") })
    }
}

impl std::error::Error for Locked {}

/// The Locked an error is, if it is one.
pub fn locked(error: &io::Error) -> Option<Locked> {
    error.get_ref().and_then(|inner| inner.downcast_ref::<Locked>()).copied()
}

fn encrypted() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, Locked { wrong: false })
}

fn wrong_password() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, Locked { wrong: true })
}

/// Passwords given this run, by the archive they open. Kept in memory
/// only, and forgotten when one turns out wrong.
static PASSWORDS: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// The archive itself, whichever way it is named: a link to it, or a path
/// that is not its shortest, finds the same password. One not made yet -
/// Pack gives the password before the job writes it - by the directory it
/// will be in, so it is the same key once it is there.
fn password_key(archive: &Path) -> PathBuf {
    fs::canonicalize(archive)
        .or_else(|_| match (archive.parent(), archive.file_name()) {
            (Some(dir), Some(name)) => fs::canonicalize(dir).map(|dir| dir.join(name)),
            _ => Err(io::Error::from(io::ErrorKind::NotFound)),
        })
        .unwrap_or_else(|_| archive.to_path_buf())
}

pub fn set_password(archive: &Path, password: &str) {
    let key = password_key(archive);
    let mut passwords = PASSWORDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    passwords.retain(|(path, _)| *path != key);
    passwords.push((key, password.to_string()));
}

pub fn password(archive: &Path) -> Option<String> {
    let key = password_key(archive);
    PASSWORDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(path, _)| *path == key).map(|(_, password)| password.clone())
}

pub fn forget_password(archive: &Path) {
    let key = password_key(archive);
    PASSWORDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|(path, _)| *path != key);
}

/// `error` as it is, the password it was read with forgotten when that was
/// the wrong one, so the next try asks again.
fn forgetting(archive: &Path, error: io::Error) -> io::Error {
    if locked(&error).is_some_and(|locked| locked.wrong) {
        forget_password(archive);
    }
    error
}

/// The 7z password for `archive`, or none.
pub(crate) fn sevenz_password(archive: &Path) -> sevenz_rust2::Password {
    password(archive).map_or_else(sevenz_rust2::Password::empty, |password| sevenz_rust2::Password::from(password.as_str()))
}

/// Whether `archive` is a kind that can be encrypted at all: a zip or a
/// 7z. Asked by name, so a tar - whose index means reading it all through,
/// too slow for the UI thread these are asked on - is never read for it.
fn lockable(archive: &Path) -> bool {
    matches!(kind_of(archive), Some(Kind::Zip | Kind::SevenZ))
}

/// Whether the entries at `inners`, or anything under them, need a
/// password fm84 has not been given - all of a 7z that will not even list
/// its names without one. False for an archive it cannot read otherwise.
pub fn needs_password(archive: &Path, inners: &[PathBuf]) -> bool {
    if !lockable(archive) || password(archive).is_some() {
        return false;
    }
    let index = match index(archive) {
        Ok(index) => index,
        Err(error) => return locked(&error).is_some(),
    };
    inners.iter().any(|inner| index.subtree(inner).into_iter().any(|at| index.entry(at).encrypted))
}

/// Whether the password kept for a zip opens it, tried on one of its
/// encrypted entries - which checks it from the entry's header, before a
/// byte is read. Wrong, it is let go of and said so. A 7z needs nothing of
/// the kind: what is read of it with the wrong one fails on its own.
pub fn check_password(archive: &Path) -> io::Result<()> {
    let Some(password) = password(archive).filter(|_| kind_of(archive) == Some(Kind::Zip)) else {
        return Ok(());
    };
    let index = index(archive)?;
    let Some(position) = index.subtree(Path::new("")).into_iter().map(|at| index.entry(at)).find(|entry| entry.encrypted).and_then(|entry| entry.position) else {
        return Ok(());
    };
    let mut zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
    match zip.by_index_decrypt(position, password.as_bytes()) {
        Ok(_) => Ok(()),
        Err(error) => Err(forgetting(archive, zip_error(error))),
    }
}

/// Whether anything in `archive` is encrypted.
pub fn is_encrypted(archive: &Path) -> bool {
    lockable(archive) && index(archive).is_ok_and(|index| index.subtree(Path::new("")).into_iter().any(|at| index.entry(at).encrypted))
}

pub(crate) fn sevenz_error(error: sevenz_rust2::Error) -> io::Error {
    match error {
        sevenz_rust2::Error::Io(error, _) | sevenz_rust2::Error::FileOpen(error, _) => error,
        // Built without AES, an encrypted 7z reads as one packed by a method
        // fm84 does not have - which is so, and says nothing useful.
        sevenz_rust2::Error::PasswordRequired => encrypted(),
        sevenz_rust2::Error::MaybeBadPassword(_) => wrong_password(),
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// Hand each entry at `positions` its contents, or the reason they cannot be
/// read. In the order given for a zip, which reads any entry at once; in the
/// archive's own order for a tar or a 7z, which have to be read through from
/// the start to reach one - a 7z a block at a time. Again, for those, goes
/// round once more for what was asked to be read again.
pub fn read_entries(archive: &Path, kind: Kind, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
            let password = password(archive);
            for &position in positions {
                loop {
                    let opened = match &password {
                        Some(password) => zip.by_index_decrypt(position, password.as_bytes()),
                        None => zip.by_index(position),
                    };
                    let next = match opened {
                        Ok(mut file) => visit(position, Ok(&mut file)),
                        Err(error) => visit(position, Err(forgetting(archive, zip_error(error)))),
                    };
                    match next {
                        Next::Go => break,
                        Next::Again => {}
                        Next::Stop => return Ok(()),
                    }
                }
            }
            Ok(())
        }
        Kind::SevenZ => read_7z_entries(archive, positions, visit),
        Kind::Git => read_git_entries(archive, positions, visit),
        Kind::Trash => read_trash_entries(archive, positions, visit),
        Kind::Deb | Kind::Iso => read_piece_entries(archive, positions, visit),
        _ => walk_tar(&|| tar_reader(archive, kind), positions.iter().map(|&position| (position, position)).collect(), visit).map(drop),
    }
}

/// Go through a tar from its start - `open` gives it, from the start again
/// for what is asked to be read again - handing `visit` each entry wanted:
/// `wanted` maps where one is in the tar to the position it is known by.
/// True when `visit` said to stop.
fn walk_tar(open: &dyn Fn() -> io::Result<Box<dyn Read>>, mut wanted: std::collections::BTreeMap<usize, usize>, visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<bool> {
    while !wanted.is_empty() {
        let mut again = std::collections::BTreeMap::new();
        let mut tar = tar::Archive::new(open()?);
        for (ordinal, entry) in tar.entries()?.enumerate() {
            let Some(&position) = wanted.get(&ordinal) else {
                continue;
            };
            let next = match entry {
                Ok(mut entry) => visit(position, Ok(&mut entry)),
                // The rest of the stream is lost with it.
                Err(error) => {
                    visit(position, Err(error));
                    return Ok(true);
                }
            };
            match next {
                Next::Go => {}
                Next::Again => {
                    again.insert(ordinal, position);
                }
                Next::Stop => return Ok(true),
            }
            wanted.remove(&ordinal);
            if wanted.is_empty() {
                break;
            }
        }
        // Asked for and never met: the archive changed under us, or is cut
        // short. Nothing more to be had from it.
        for (_, position) in std::mem::take(&mut wanted) {
            let missing = io::Error::new(io::ErrorKind::UnexpectedEof, t!("The archive ends before it"));
            if let Next::Stop = visit(position, Err(missing)) {
                return Ok(true);
            }
        }
        wanted = again;
    }
    Ok(false)
}

/// The side of read_entries for a .deb and an .iso, whose files are pieces
/// of the archive file where the index found them: read from there, or out
/// of the tar in the .deb they are in - each tar gone through once for all
/// that is wanted of it.
fn read_piece_entries(archive: &Path, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    use index::Piece;
    let index = index(archive)?;
    // Each tar by where it is, and what is wanted of it.
    type Wanted = std::collections::BTreeMap<usize, usize>;
    let mut tars: Vec<((u64, u64, Kind), Wanted)> = Vec::new();
    for &position in positions {
        match index.piece(position) {
            Some(Piece::Spans(spans)) => loop {
                let next = match spans_reader(archive, spans) {
                    Ok(mut reader) => visit(position, Ok(&mut reader)),
                    Err(error) => visit(position, Err(error)),
                };
                match next {
                    Next::Go => break,
                    Next::Again => {}
                    Next::Stop => return Ok(()),
                }
            },
            Some(&Piece::InTar { at, length, kind, ordinal }) => match tars.iter_mut().find(|(tar, _)| *tar == (at, length, kind)) {
                Some((_, wanted)) => {
                    wanted.insert(ordinal, position);
                }
                None => tars.push(((at, length, kind), [(ordinal, position)].into())),
            },
            None => {
                if let Next::Stop = visit(position, Err(io::Error::new(io::ErrorKind::NotFound, t!("The archive ends before it")))) {
                    return Ok(());
                }
            }
        }
    }
    for ((at, length, kind), wanted) in tars {
        if walk_tar(&|| decompressed(piece_reader(archive, at, length)?, kind), wanted, visit)? {
            return Ok(());
        }
    }
    Ok(())
}

/// The `length` bytes of `archive` from `at` on.
pub(crate) fn piece_reader(archive: &Path, at: u64, length: u64) -> io::Result<BufReader<io::Take<File>>> {
    use io::Seek;
    let mut file = File::open(archive)?;
    file.seek(io::SeekFrom::Start(at))?;
    Ok(BufReader::new(file.take(length)))
}

/// One piece after another of `archive`, as one file: a file on a disc
/// image can be kept in more than one.
fn spans_reader(archive: &Path, spans: &[(u64, u64)]) -> io::Result<Box<dyn Read>> {
    let mut reader: Box<dyn Read> = Box::new(io::empty());
    for &(at, length) in spans {
        reader = Box::new(reader.chain(piece_reader(archive, at, length)?));
    }
    Ok(reader)
}

/// The git side of read_entries: each file read out of the repository by
/// the object it is, in the order asked.
/// What is in the trash, each file read from where the trash keeps it.
fn read_trash_entries(archive: &Path, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    let index = index(archive)?;
    for &position in positions {
        loop {
            let opened = index.real(position).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, t!("No longer in the trash"))).and_then(File::open);
            let next = match opened {
                Ok(mut file) => visit(position, Ok(&mut file)),
                Err(error) => visit(position, Err(error)),
            };
            match next {
                Next::Go => break,
                Next::Again => {}
                Next::Stop => return Ok(()),
            }
        }
    }
    Ok(())
}

fn read_git_entries(archive: &Path, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    let index = index(archive)?;
    let (top, _) = git_revision(archive).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t!("Not an archive: {path}", path = archive.display())))?;
    for &position in positions {
        loop {
            let next = match index.object(position).map(|object| crate::git::blob(&top, object)) {
                Some(Ok(mut child)) => {
                    let mut stdout = child.stdout.take().expect("piped");
                    let next = visit(position, Ok(&mut stdout));
                    // Not read to its end, git would wait to write the rest.
                    drop(stdout);
                    let _ = child.wait();
                    next
                }
                Some(Err(error)) => visit(position, Err(error)),
                None => visit(position, Err(io::Error::new(io::ErrorKind::NotFound, t!("The archive ends before it")))),
            };
            match next {
                Next::Go => break,
                Next::Again => {}
                Next::Stop => return Ok(()),
            }
        }
    }
    Ok(())
}

/// The 7z side of read_entries. The archive hands out its entries in its
/// own order, each by reference into its list of them, which is how each is
/// known again. All of them share one stream per block: one not read to its
/// end - not wanted, cut short by a failure - is read past, or the next would
/// start in the middle of it.
fn read_7z_entries(archive: &Path, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    let mut pending: std::collections::BTreeSet<usize> = positions.iter().copied().collect();
    while !pending.is_empty() {
        let mut again = std::collections::BTreeSet::new();
        let mut reader = sevenz_rust2::ArchiveReader::new(open(archive)?, sevenz_password(archive)).map_err(|error| forgetting(archive, sevenz_error(error)))?;
        let (base, count) = (reader.archive().files.as_ptr() as usize, reader.archive().files.len());
        let mut stopped = false;
        let walked = reader.for_each_entries(|entry, contents| {
            let position = (entry as *const sevenz_rust2::ArchiveEntry as usize).wrapping_sub(base) / std::mem::size_of::<sevenz_rust2::ArchiveEntry>();
            if position < count && pending.remove(&position) {
                match visit(position, Ok(&mut *contents)) {
                    Next::Go => {}
                    Next::Again => {
                        again.insert(position);
                    }
                    Next::Stop => {
                        stopped = true;
                        return Ok(false);
                    }
                }
            }
            io::copy(contents, &mut io::sink())?;
            Ok(!pending.is_empty())
        });
        if stopped {
            return Ok(());
        }
        // The rest could not be reached: a block that would not decode, a
        // method fm84 does not have, a password it was not given.
        if let Err(error) = walked {
            let error = forgetting(archive, sevenz_error(error));
            for position in std::mem::take(&mut pending) {
                if let Next::Stop = visit(position, Err(io::Error::new(error.kind(), error.to_string()))) {
                    return Ok(());
                }
            }
        }
        for position in std::mem::take(&mut pending) {
            let missing = io::Error::new(io::ErrorKind::UnexpectedEof, t!("The archive ends before it"));
            if let Next::Stop = visit(position, Err(missing)) {
                return Ok(());
            }
        }
        pending = again;
    }
    Ok(())
}

/// A tar read through whatever it was compressed with. Every one of them
/// reads on past the end of its first stream: files joined end to end, as
/// pigz, pzstd and pbzip2 write them, are one archive.
pub(crate) fn tar_reader(archive: &Path, kind: Kind) -> io::Result<Box<dyn Read>> {
    decompressed(open(archive)?, kind)
}

/// A tar's bytes, from `file`, uncompressed as `kind` says.
pub(crate) fn decompressed<R: io::BufRead + 'static>(file: R, kind: Kind) -> io::Result<Box<dyn Read>> {
    Ok(match kind {
        Kind::TarGz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        Kind::TarZst => Box::new(ZstdFrames::new(file)?),
        Kind::TarXz => Box::new(lzma_rust2::XzReader::new(file, true)),
        Kind::TarBz2 => Box::new(bzip2::read::MultiBzDecoder::new(file)),
        _ => Box::new(file),
    })
}

/// zstd, a frame after another until the input ends. ruzstd's own reader
/// stops at the end of the first.
struct ZstdFrames<R: io::BufRead> {
    frame: Option<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>,
}

impl<R: io::BufRead> ZstdFrames<R> {
    fn new(source: R) -> io::Result<Self> {
        Ok(ZstdFrames {
            frame: Some(ruzstd::decoding::StreamingDecoder::new(source).map_err(io::Error::other)?),
        })
    }
}

impl<R: io::BufRead> Read for ZstdFrames<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let Some(frame) = &mut self.frame else {
                return Ok(0);
            };
            let read = frame.read(buf)?;
            if read > 0 || buf.is_empty() {
                return Ok(read);
            }
            let (mut source, decoder) = self.frame.take().expect("matched above").into_parts();
            if source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            self.frame = Some(ruzstd::decoding::StreamingDecoder::new_with_decoder(source, decoder).map_err(io::Error::other)?);
        }
    }
}

/// A directory where entries opened from an archive are put to be viewed or
/// handed to another program, emptied when fm84 quits. Each gets one of its
/// own, so two entries of the same name never meet.
pub fn scratch_dir() -> io::Result<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = scratch_root().join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join(format!("fm84-{}", std::process::id()))
}

/// Remove what scratch_dir made. A program still holding a file in it keeps
/// what it opened on Unix; on Windows what it holds is left behind.
pub fn clean_scratch() {
    let _ = fs::remove_dir_all(scratch_root());
}
