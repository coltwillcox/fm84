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
    TarZst,
    TarXz,
    TarBz2,
    SevenZ,
}

/// The kind of archive a file is, by its name. The name is only a claim:
/// what is not one fails to read, and says so.
pub fn kind_of(path: &Path) -> Option<Kind> {
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

pub(crate) fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(error) => error,
        zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED) => encrypted(),
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

fn encrypted() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "Encrypted: fm84 cannot read it without the password")
}

pub(crate) fn sevenz_error(error: sevenz_rust2::Error) -> io::Error {
    match error {
        sevenz_rust2::Error::Io(error, _) | sevenz_rust2::Error::FileOpen(error, _) => error,
        // Built without AES, an encrypted 7z reads as one packed by a method
        // fm84 does not have - which is so, and says nothing useful.
        sevenz_rust2::Error::PasswordRequired | sevenz_rust2::Error::MaybeBadPassword(_) => encrypted(),
        sevenz_rust2::Error::UnsupportedCompressionMethod(method) if method.starts_with("AES") => encrypted(),
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
        Kind::SevenZ => read_7z_entries(archive, positions, visit),
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

/// The 7z side of read_entries. The archive hands out its entries in its
/// own order, each by reference into its list of them, which is how each is
/// known again. All of them share one stream per block: one not read to its
/// end - not wanted, cut short by a failure - is read past, or the next would
/// start in the middle of it.
fn read_7z_entries(archive: &Path, positions: &[usize], visit: &mut dyn FnMut(usize, io::Result<&mut dyn Read>) -> Next) -> io::Result<()> {
    let mut pending: std::collections::BTreeSet<usize> = positions.iter().copied().collect();
    while !pending.is_empty() {
        let mut again = std::collections::BTreeSet::new();
        let mut reader = sevenz_rust2::ArchiveReader::new(open(archive)?, sevenz_rust2::Password::empty()).map_err(sevenz_error)?;
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
            let error = sevenz_error(error);
            for position in std::mem::take(&mut pending) {
                if let Next::Stop = visit(position, Err(io::Error::new(error.kind(), error.to_string()))) {
                    return Ok(());
                }
            }
        }
        for position in std::mem::take(&mut pending) {
            let missing = io::Error::new(io::ErrorKind::UnexpectedEof, "The archive ends before it");
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
fn tar_reader(archive: &Path, kind: Kind) -> io::Result<Box<dyn Read>> {
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
