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
}

/// The kind of archive a file is, by its name. The name is only a claim:
/// what is not one fails to read, and says so.
pub fn kind_of(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        Some(Kind::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(Kind::TarGz)
    } else if name.ends_with(".tar") {
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
    let kind = kind_of(archive).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("Not an archive: {}", archive.display())))?;
    let metadata = fs::metadata(archive)?;
    let key = (archive.to_path_buf(), metadata.modified().ok(), metadata.len());
    if let Some((_, index)) = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|(cached, _)| *cached == key) {
        return Ok(Arc::clone(index));
    }
    // Read without the lock held: another archive asked for meanwhile need
    // not wait on this one.
    let index = Arc::new(Index::read(archive, kind)?);
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

fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(error) => error,
        zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED) => io::Error::new(io::ErrorKind::Unsupported, "Encrypted: fm84 cannot read it without the password"),
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

/// Hand each entry at `positions` its contents, or the reason they cannot be
/// read. In the order given for a zip, which reads any entry at once; in the
/// archive's own order for a tar, which has to be read through from the
/// start to reach one. Again, for a tar, goes round once more for what was
/// asked to be read again.
pub fn read_entries(archive: &Path, kind: Kind, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(open(archive)?).map_err(zip_error)?;
            for &position in positions {
                loop {
                    let next = match zip.by_index(position) {
                        Ok(mut file) => visit(position, Ok(&mut file)),
                        Err(error) => visit(position, Err(zip_error(error))),
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
        Kind::Tar | Kind::TarGz => {
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
                        let missing = io::Error::new(io::ErrorKind::UnexpectedEof, "The archive ends before it");
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

fn tar_reader(archive: &Path, kind: Kind) -> io::Result<Box<dyn Read>> {
    let file = open(archive)?;
    Ok(match kind {
        Kind::TarGz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        _ => Box::new(file),
    })
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
