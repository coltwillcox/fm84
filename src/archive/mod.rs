//! Archives read as directories. A panel goes into one by a path that runs
//! through it - `/home/me/photos.zip/2024/june` - so tabs, history, bookmarks
//! and the session file all go on holding a plain path, and only what reads
//! the disk has to know that part of it lies inside a file.
//!
//! Read-only: nothing here writes into an archive.

#[cfg(test)]
pub mod fixtures;
mod index;
#[cfg(test)]
mod tests;

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
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let named = |endings: &[&str]| endings.iter().any(|ending| name.ends_with(ending));
    if named(&[".zip"]) {
        Some(Kind::Zip)
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
    let mut inner = Vec::new();
    let mut here = path;
    loop {
        // A commit's folder is not on the disk, but the repository it is in is.
        if git_revision(here).is_some() && here.parent().is_some_and(Path::is_dir) {
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
/// archive's size and modified time as well as its path: one written since is
/// read again.
const CACHED: usize = 4;
type Key = (PathBuf, Option<SystemTime>, u64);
static CACHE: Mutex<Vec<(Key, Arc<Index>)>> = Mutex::new(Vec::new());

/// The index of `archive`, read on first asking.
pub fn index(archive: &Path) -> io::Result<Arc<Index>> {
    let kind = kind_of(archive).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, t!("Not an archive: {path}", path = archive.display())))?;
    // A commit never changes; a branch moves on, and is read again once it
    // names another commit.
    let (key, commit) = match git_revision(archive) {
        Some((top, revision)) => {
            let commit = crate::git::resolve(&top, &revision).map_err(io::Error::other)?;
            let hash = commit.bytes().fold(0u64, |hash, byte| hash.wrapping_mul(31).wrapping_add(u64::from(byte)));
            ((archive.to_path_buf(), None, hash), Some((top, commit)))
        }
        None => {
            let metadata = fs::metadata(archive)?;
            ((archive.to_path_buf(), metadata.modified().ok(), metadata.len()), None)
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
/// that is not its shortest, finds the same password.
fn password_key(archive: &Path) -> PathBuf {
    fs::canonicalize(archive).unwrap_or_else(|_| archive.to_path_buf())
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

/// Whether the entries at `inners`, or anything under them, need a
/// password fm84 has not been given. False for an archive it cannot read.
pub fn needs_password(archive: &Path, inners: &[PathBuf]) -> bool {
    if password(archive).is_some() {
        return false;
    }
    let Ok(index) = index(archive) else {
        return false;
    };
    inners.iter().any(|inner| index.subtree(inner).into_iter().any(|at| index.entry(at).encrypted))
}

/// Whether anything in `archive` is encrypted.
pub fn is_encrypted(archive: &Path) -> bool {
    index(archive).is_ok_and(|index| index.subtree(Path::new("")).into_iter().any(|at| index.entry(at).encrypted))
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
        _ => {
            let mut pending: std::collections::BTreeSet<usize> = positions.iter().copied().collect();
            while !pending.is_empty() {
                let mut again = std::collections::BTreeSet::new();
                let mut tar = tar::Archive::new(tar_reader(archive, kind)?);
                for (ordinal, entry) in tar.entries()?.enumerate() {
                    if !pending.contains(&ordinal) {
                        continue;
                    }
                    let next = match entry {
                        Ok(mut entry) => visit(ordinal, Ok(&mut entry)),
                        // The rest of the stream is lost with it.
                        Err(error) => {
                            visit(ordinal, Err(error));
                            return Ok(());
                        }
                    };
                    match next {
                        Next::Go => {}
                        Next::Again => {
                            again.insert(ordinal);
                        }
                        Next::Stop => return Ok(()),
                    }
                    pending.remove(&ordinal);
                    if pending.is_empty() {
                        break;
                    }
                }
                // Asked for and never met: the archive changed under us, or
                // is cut short. Nothing more to be had from it.
                if !pending.is_empty() {
                    for ordinal in std::mem::take(&mut pending) {
                        let missing = io::Error::new(io::ErrorKind::UnexpectedEof, t!("The archive ends before it"));
                        if let Next::Stop = visit(ordinal, Err(missing)) {
                            return Ok(());
                        }
                    }
                }
                pending = again;
            }
            Ok(())
        }
    }
}

/// The git side of read_entries: each file read out of the repository by
/// the object it is, in the order asked.
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
    let file = open(archive)?;
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
