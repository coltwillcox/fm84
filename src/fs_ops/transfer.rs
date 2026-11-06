//! Copying and moving, an entry at a time, asking about what is in the way and
//! what fails, so one bad entry does not sink the job.

use super::*;

/// What a running transfer reports as it goes.
pub enum Step<'a> {
    /// Starting this file. Bytes reported after it belong to it.
    Starting(&'a Path),
    /// Another `amount` of the current entry is done. Bytes for a copy, and
    /// entries removed for a delete, which is what each counts its total in.
    Advanced(u64),
}

/// How a transfer ended. Cancelled leaves everything already finished where it
/// is; only the one file that was in flight is cleaned up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transfer {
    Done,
    Cancelled,
}

/// How one entry went, inside a walk. Skipped tells the directory around it
/// that something in it was left where it was: a delete then keeps the
/// directory, and a move keeps the source directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Outcome {
    Done,
    Skipped,
    Cancelled,
}

/// A walk with entries skipped still finished: what was skipped was answered
/// for, one at a time, as it came up.
impl From<Outcome> for Transfer {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Cancelled => Transfer::Cancelled,
            Outcome::Done | Outcome::Skipped => Transfer::Done,
        }
    }
}

/// What to do about an entry that could not be dealt with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reply {
    /// Try the same thing again - after freeing some space, say, or
    /// remounting a drive.
    Retry,
    /// Leave this entry where it is and go on with the rest.
    Skip,
    /// Stop the whole job, passing the error up.
    Abort,
    /// Delete it for good instead. Offered only when moving to the trash,
    /// for what the trash will not take; anywhere else it is a Skip.
    Delete,
}

/// Follows a running transfer.
pub trait Progress {
    /// Takes progress and answers whether to carry on. Returning false stops
    /// the transfer at the next chunk or file, whichever comes first.
    fn step(&mut self, step: Step<'_>) -> bool;

    /// `path` could not be copied, moved or removed, for `error`. Asked once
    /// for each failure. Left alone, the first one ends the job.
    fn failed(&mut self, _path: &Path, _error: &Error) -> Reply {
        Reply::Abort
    }

    /// `dest` is already there - a file or a link - and `source` is about to
    /// be written over it. Asked once for each; a directory onto a directory
    /// is never asked about, but merged, its files asked about one at a time.
    /// Left alone, what is there is replaced: a caller with nobody to ask has
    /// already said it may be.
    fn exists(&mut self, _source: &Path, _dest: &Path) -> Clash {
        Clash::Replace
    }

    /// Whether each file copied is read back from where it went and its
    /// checksum compared with the original's, before it takes its place.
    fn verify(&self) -> bool {
        false
    }
}

/// The answer to a name already taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clash {
    /// Write over what is there.
    Replace,
    /// Leave what is there, and the source where it was.
    Keep,
    /// Leave what is there, and write the source beside it under a name of
    /// its own: `name (2).ext`.
    KeepBoth,
    /// Stop the whole job.
    Abort,
}

/// A closure is a Progress that never answers for a failure, which is all a
/// caller wants when it has nobody to ask.
impl<F: FnMut(Step<'_>) -> bool> Progress for F {
    fn step(&mut self, step: Step<'_>) -> bool {
        self(step)
    }
}

pub type Report<'a> = &'a mut dyn Progress;

/// Do `op` for `path`, and when it fails ask what next: try it again, leave
/// this entry where it is (None), or give up with the error.
pub(super) fn attempt<T>(path: &Path, report: Report<'_>, mut op: impl FnMut(Report<'_>) -> Result<T, Error>) -> Result<Option<T>, Error> {
    loop {
        match op(&mut *report) {
            Ok(value) => return Ok(Some(value)),
            Err(error) => match report.failed(path, &error) {
                Reply::Retry => {}
                Reply::Skip | Reply::Delete => return Ok(None),
                Reply::Abort => return Err(error),
            },
        }
    }
}

/// The next entry of a listing and what kind it is, or None when reading it
/// failed and the answer was to go on past it. Retry is taken as Skip here:
/// the listing has moved on, and there is no asking it for the same entry.
pub(super) fn next_entry(dir: &Path, entry: Result<fs::DirEntry, Error>, report: Report<'_>) -> Result<Option<(fs::DirEntry, fs::FileType)>, Error> {
    match entry.and_then(|entry| entry.file_type().map(|file_type| (entry, file_type))) {
        Ok(pair) => Ok(Some(pair)),
        Err(error) => match report.failed(dir, &error) {
            Reply::Abort => Err(error),
            Reply::Retry | Reply::Skip | Reply::Delete => Ok(None),
        },
    }
}

/// Move by renaming, which only works within one filesystem. True if it did;
/// the caller copies instead when it did not. Kept separate from move_path so
/// a caller can get the instant moves out of the way before measuring the rest.
pub fn rename_in_place(source: &Path, dest: &Path) -> bool {
    rename(source, dest).is_ok()
}

/// Bytes a transfer will move, for the progress bar to count against. A
/// directory is walked; anything unreadable counts as nothing rather than
/// failing the job, since this is only ever a denominator.
pub fn measure(items: &[(PathBuf, PathBuf, bool)]) -> u64 {
    items
        .iter()
        .map(|(source, _, is_dir)| {
            // A symlink is recreated, not followed, so it carries no bytes.
            if fs::symlink_metadata(source).map(|meta| meta.file_type().is_symlink()).unwrap_or(false) {
                0
            } else if *is_dir {
                calculate_dir_size(source).unwrap_or(0)
            } else {
                fs::metadata(source).map(|meta| meta.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// A filesystem that has not the room for what a job is about to write to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortfall {
    /// The directory written into, standing for its filesystem.
    pub dir: PathBuf,
    pub needed: u64,
    pub free: u64,
}

/// What each of `items` will write, by where it goes, given `sizes` as
/// `measure` counts them. A move within one filesystem renames rather than
/// writes, so it needs no room - only a move to another one does.
pub fn writes(items: &[(PathBuf, PathBuf, bool)], sizes: &[u64], is_copy: bool) -> Vec<(PathBuf, u64)> {
    items
        .iter()
        .zip(sizes)
        .map(|((source, dest, _), &size)| {
            let renames = !is_copy && same_filesystem(source, dest);
            (dest.clone(), if renames { 0 } else { size })
        })
        .collect()
}

/// The first filesystem among `writes` - each where something goes and its
/// bytes - with less room than all that goes to it, before anything does.
/// One that will not say how much room it has is taken to have enough: a
/// copy that then fails asks about it as any failure does.
pub fn short_of_room(writes: &[(PathBuf, u64)]) -> Option<Shortfall> {
    // Each filesystem written to, by a directory on it, and what it takes.
    let mut filesystems: Vec<(PathBuf, u64)> = Vec::new();
    for (dest, bytes) in writes {
        let Some(dir) = nearest_existing_dir(dest) else { continue };
        match filesystems.iter_mut().find(|(known, _)| same_filesystem(known, &dir)) {
            Some((_, needed)) => *needed = needed.saturating_add(*bytes),
            None => filesystems.push((dir, *bytes)),
        }
    }
    filesystems.into_iter().filter(|(_, needed)| *needed > 0).find_map(|(dir, needed)| {
        let free = free_space(&dir)?;
        (needed > free).then_some(Shortfall { dir, needed, free })
    })
}

/// Whether `a` and `b` are on one filesystem: each as it is, or, when it is
/// not there yet, the directory it would go into.
fn same_filesystem(a: &Path, b: &Path) -> bool {
    let on = |path: &Path| fs::symlink_metadata(path).ok().map(|_| path.to_path_buf()).or_else(|| nearest_existing_dir(path));
    let (Some(a), Some(b)) = (on(a), on(b)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        matches!((fs::symlink_metadata(&a), fs::symlink_metadata(&b)), (Ok(a), Ok(b)) if a.dev() == b.dev())
    }
    // A drive letter, or a share, is a filesystem of its own.
    #[cfg(not(unix))]
    {
        let root = |path: &Path| path.components().next().map(|component| component.as_os_str().to_ascii_lowercase());
        root(&a) == root(&b)
    }
}

/// True when copying `source` to `dest` would descend into what it is writing:
/// a directory copied to somewhere inside itself. copy_dir_recursive creates
/// the destination before it reads the source, so it then finds it, copies it,
/// finds it again, and carries on until the path outgrows PATH_MAX - around a
/// thousand levels, re-copying every file at each one.
///
/// Paths are resolved first, so a destination reaching the source through a
/// symlink counts. A symlink source does not: it is recreated as a link rather
/// than walked, so it cannot recurse.
pub fn copies_into_itself(source: &Path, dest: &Path) -> bool {
    if fs::symlink_metadata(source).map(|meta| meta.file_type().is_symlink()).unwrap_or(false) || !source.is_dir() {
        return false;
    }

    let resolve = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let source = resolve(source);
    // The destination does not exist yet, so its parent is what can be resolved.
    let dest = match (dest.parent(), dest.file_name()) {
        (Some(parent), Some(name)) => resolve(parent).join(name),
        _ => resolve(dest),
    };
    // Component-wise, so "/x/subfoo" is not inside "/x/sub".
    dest.starts_with(&source)
}

pub fn copy_path(source: PathBuf, dest: PathBuf, is_dir: bool, report: Report<'_>) -> Result<Transfer, Error> {
    // Refused here as well as before the transfer starts, so no caller can
    // reach the runaway however it assembles its paths.
    if copies_into_itself(&source, &dest) {
        return Err(Error::new(ErrorKind::InvalidInput, t!("Cannot copy \"{path}\" into itself", path = source.display())));
    }

    // A symlink is copied as the link itself, never as its target, matching
    // cp -r. is_dir can't decide this: it comes from DirEntry::metadata(),
    // which doesn't follow links, so a link to a directory arrives false here
    // and would otherwise be handed to copy_file_content.
    let Some(metadata) = attempt(&source, report, |_| fs::symlink_metadata(&source))? else {
        return Ok(Transfer::Done);
    };
    let outcome = if metadata.file_type().is_symlink() {
        match settle_clash(&source, &dest, report) {
            Ok(target) => copy_link(&source, &target, report)?,
            Err(outcome) => outcome,
        }
    } else if is_dir {
        copy_dir_recursive(&source, &dest, report)?
    } else {
        match settle_clash(&source, &dest, report) {
            Ok(target) => copy_file(&source, &target, report)?,
            Err(outcome) => outcome,
        }
    };
    Ok(outcome.into())
}

/// Where `source` is to be written, when `dest` may already be there: at
/// `dest` itself, or beside it under a name of its own. A file or a link in
/// the way is asked about; a directory is not - a directory is merged into
/// it, and a file onto it fails, which is asked about as a failure. Err for
/// how the entry came out without being written: left where it was, or the
/// job stopped.
pub(super) fn settle_clash(source: &Path, dest: &Path, report: Report<'_>) -> Result<PathBuf, Outcome> {
    match dest.symlink_metadata() {
        Ok(existing) if !existing.is_dir() => {}
        _ => return Ok(dest.to_path_buf()),
    }
    match report.exists(source, dest) {
        Clash::Replace => Ok(dest.to_path_buf()),
        Clash::KeepBoth => Ok(free_name(dest)),
        Clash::Keep => {
            // Counted as dealt with, so the bar still reaches the end.
            let bytes = source.symlink_metadata().map_or(0, |metadata| if metadata.is_file() { metadata.len() } else { 0 });
            let carry_on = report.step(Step::Starting(source)) && report.step(Step::Advanced(bytes));
            Err(if carry_on { Outcome::Skipped } else { Outcome::Cancelled })
        }
        Clash::Abort => Err(Outcome::Cancelled),
    }
}

/// Recreate a link, asking about it if that fails.
fn copy_link(source: &Path, dest: &Path, report: Report<'_>) -> Result<Outcome, Error> {
    let copied = attempt(source, report, |_| {
        clear_for_link(dest)?;
        copy_symlink(source, dest)
    })?;
    Ok(if copied.is_some() { Outcome::Done } else { Outcome::Skipped })
}

/// Copy a file, asking about it if that fails. A retry starts the file over:
/// copy_file_content has cleared away what it wrote before failing.
fn copy_file(source: &Path, dest: &Path, report: Report<'_>) -> Result<Outcome, Error> {
    Ok(match attempt(source, report, |report| copy_file_content(source, dest, report))? {
        Some(Transfer::Done) => Outcome::Done,
        Some(Transfer::Cancelled) => Outcome::Cancelled,
        None => Outcome::Skipped,
    })
}

/// Recreate a symlink at the destination, pointing where the original pointed.
#[cfg(unix)]
fn copy_symlink(source: &Path, dest: &Path) -> Result<(), Error> {
    std::os::unix::fs::symlink(fs::read_link(source)?, dest)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, dest: &Path) -> Result<(), Error> {
    let target = fs::read_link(source)?;
    // Windows picks the call by link kind, and creating one needs Developer
    // Mode or elevation - the error surfaces to the user either way.
    if source.is_dir() { std::os::windows::fs::symlink_dir(target, dest) } else { std::os::windows::fs::symlink_file(target, dest) }
}

/// Copy a file's content, then its times and permissions - so a script or a
/// program stays executable, and a move across filesystems leaves the file
/// dated as it was. Setting them is allowed to fail: on a filesystem with no
/// Unix permissions (ext4 to exFAT, say) it would, with EPERM, and the copy
/// itself is still good.
///
/// The copy runs in COPY_CHUNK pieces so there is somewhere to report progress
/// from and somewhere to notice a cancel. Each piece is still an io::copy, over
/// a reader limited to the chunk rather than a buffer of our own, which is what
/// keeps the platform's fast path - copy_file_range on Linux.
fn copy_file_content(source: &Path, dest: &Path, report: Report<'_>) -> Result<Transfer, Error> {
    // Links are recreated before this is reached, so what arrives here is the
    // entry itself. A pipe or a device would block the worker in open() or
    // read forever, out of reach of Esc. cp -R recreates them instead of
    // reading them; short of doing that, refusing is what is safe.
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_file() {
        return Err(not_regular(source));
    }
    if !report.step(Step::Starting(source)) {
        return Ok(Transfer::Cancelled);
    }

    let mut src_file = File::open(source)?;
    let finish = |dst_file: &File, temp: &Path| {
        keep_times(dst_file, &metadata);
        let _ = fs::set_permissions(temp, metadata.permissions());
    };
    if !report.verify() {
        return write_beside(dest, &mut src_file, report, &finish, None);
    }
    // The source hashed as it is read for the copy, so it is read once; the
    // copy then read back and hashed on its own.
    let hasher = std::cell::RefCell::new(sha2::Sha256::default());
    let mut hashing = Hashing { inner: src_file, hasher: &hasher };
    write_beside(dest, &mut hashing, report, &finish, Some(&|temp, report| read_back(temp, &hasher, report)))
}

/// A reader that hashes what passes through it.
struct Hashing<'a, R> {
    inner: R,
    hasher: &'a std::cell::RefCell<sha2::Sha256>,
}

impl<R: Read> Read for Hashing<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        sha2::Digest::update(&mut *self.hasher.borrow_mut(), &buffer[..read]);
        Ok(read)
    }
}

/// Read a copy back, written whole beside where it goes, and say whether it
/// holds what `source` hashed: Done when it does, an error when it does not.
/// Flushed to the device first and its pages let go of, so what is read is
/// what the device kept, not what the system still has in memory from
/// writing it. Counted on the bar as the copy was.
fn read_back(temp: &Path, source: &std::cell::RefCell<sha2::Sha256>, report: Report<'_>) -> Result<Transfer, Error> {
    use sha2::Digest;
    let mut file = File::open(temp)?;
    file.sync_all()?;
    forget_cached(&file);
    let mut hasher = sha2::Sha256::default();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        hasher.update(&buffer[..read]);
        if !report.step(Step::Advanced(read as u64)) {
            return Ok(Transfer::Cancelled);
        }
    }
    if hasher.finalize() != source.borrow().clone().finalize() {
        return Err(Error::other(t!("The copy reads back differently from the original")));
    }
    Ok(Transfer::Done)
}

/// Let go of what the system holds of `file` in memory, so the next read
/// goes to the device. Where there is no saying so, the read may come from
/// memory; the flush before it has still made the device take the bytes.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd"))]
fn forget_cached(file: &File) {
    use std::os::fd::AsRawFd;
    // SAFETY: a valid descriptor, and advice the kernel is free to ignore.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

#[cfg(target_vendor = "apple")]
fn forget_cached(file: &File) {
    use std::os::fd::AsRawFd;
    // SAFETY: a valid descriptor; reads through it then skip the cache.
    unsafe {
        libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd", target_vendor = "apple")))]
fn forget_cached(_file: &File) {}

/// A look at a file written whole, before it takes its place: Done to let
/// it, Cancelled or an error to throw it away.
pub(super) type Check<'a> = &'a dyn Fn(&Path, Report<'_>) -> Result<Transfer, Error>;

/// Write what `reader` holds to `dest`, reporting each chunk as it goes, and
/// `finish` the file - its times, its mode - before it takes its place.
/// Generic, not a `dyn Read`, so a file copied from a file keeps the
/// platform's fast path.
///
/// The bytes go to a temporary beside the destination, and a rename puts
/// them in place once they are all there. Only an overwrite, or a directory
/// merged into another, finds something already at the destination; that
/// file is somebody's, and writing straight over it would destroy it the
/// moment the copy began. Where nothing is there, a file written under its
/// own name would still be half a file whenever the copy stopped short of
/// the end - fm84 quit partway, or killed - and would pass for a whole one.
/// A temporary that is left behind is plainly not one.
///
/// `check`, when there is one, looks at the temporary once it is finished
/// and before it takes its place: an error from it leaves nothing behind,
/// as a failed write does.
pub(super) fn write_beside<R: Read + ?Sized>(dest: &Path, reader: &mut R, report: Report<'_>, finish: &dyn Fn(&File, &Path), check: Option<Check<'_>>) -> Result<Transfer, Error> {
    let replacing = dest.symlink_metadata().is_ok();
    let (temp, mut dst_file) = temp_beside(dest)?;
    // What a failure or a cancel partway leaves: nothing, of ours.
    let discard = |dst_file: File| {
        drop(dst_file);
        let _ = remove_file(&temp);
    };
    loop {
        let copied = match io::copy(&mut (&mut *reader).take(COPY_CHUNK), &mut dst_file) {
            Ok(copied) => copied,
            Err(e) => {
                // A read or write that failed partway - a full disk, a device
                // gone. What was at the destination, if anything, is untouched.
                discard(dst_file);
                return Err(e);
            }
        };
        if copied == 0 {
            finish(&dst_file, &temp);
            drop(dst_file);
            if let Some(check) = check {
                match check(&temp, &mut *report) {
                    Ok(Transfer::Done) => {}
                    outcome => {
                        let _ = remove_file(&temp);
                        return outcome;
                    }
                }
            }
            place(&temp, dest, replacing)?;
            return Ok(Transfer::Done);
        }
        if !report.step(Step::Advanced(copied)) {
            // Half a file that will never be finished.
            discard(dst_file);
            return Ok(Transfer::Cancelled);
        }
    }
}

/// Put a temporary, written whole, where it belongs: the one moment the
/// destination changes, and it changes all at once - anything reading it sees
/// the old bytes or the new ones. `replacing` says something was there when
/// the writing began and may be written over. The temporary goes either way.
pub(super) fn place(temp: &Path, dest: &Path, replacing: bool) -> Result<(), Error> {
    let placed = if replacing {
        // A rename takes a link's place without following it, and in one
        // step. Cleared first, the link was lost whenever the rename then
        // failed; that is kept for where it refuses - Windows, over a link
        // to a directory.
        rename(temp, dest).or_else(|_| clear_link(dest).and_then(|()| rename(temp, dest)))
    } else if path_exists(dest) {
        // Taken while the copy ran, and nobody said it could be replaced. A
        // rename would replace it without a word.
        Err(Error::new(ErrorKind::AlreadyExists, t!("Destination already exists: {path}", path = dest.display())))
    } else {
        rename(temp, dest)
    };
    if placed.is_err() {
        let _ = remove_file(temp);
    }
    placed
}

/// Give a copy the access and modification times of what it was copied
/// from, as cp -p and file managers do. Allowed to fail, like the permissions:
/// some filesystems keep no such times, or will not take them from us.
pub(super) fn keep_times(dest: &File, metadata: &fs::Metadata) {
    let mut times = fs::FileTimes::new();
    if let Ok(modified) = metadata.modified() {
        times = times.set_modified(modified);
    }
    if let Ok(accessed) = metadata.accessed() {
        times = times.set_accessed(accessed);
    }
    let _ = dest.set_times(times);
}

/// keep_times for a directory, once everything is inside it - each file
/// copied in moves its modification time on. Opening a directory to set them
/// fails on Windows, which only costs the times.
pub(super) fn keep_dir_times(dest: &Path, metadata: &fs::Metadata) {
    if let Ok(dir) = File::open(dest) {
        keep_times(&dir, metadata);
    }
}

fn copy_dir_recursive(source: &Path, dest: &Path, report: Report<'_>) -> Result<Outcome, Error> {
    if attempt(dest, report, |_| fs::create_dir_all(dest))?.is_none() {
        return Ok(Outcome::Skipped);
    }
    let Some(entries) = attempt(source, report, |_| read_dir(source))? else {
        return Ok(Outcome::Skipped);
    };

    let mut skipped = false;
    for entry in entries {
        let Some((entry, file_type)) = next_entry(source, entry, report)? else {
            skipped = true;
            continue;
        };
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        // file_type() describes the entry itself; is_dir() would follow the
        // link, copying the target's contents - and a link pointing at an
        // ancestor recurses until the path outgrows PATH_MAX. cp -r recreates
        // the link, so do the same.
        let outcome = if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path, &mut *report)?
        } else {
            match settle_clash(&entry_path, &dest_path, &mut *report) {
                Err(outcome) => outcome,
                Ok(target) if file_type.is_symlink() => copy_link(&entry_path, &target, &mut *report)?,
                Ok(target) => copy_file(&entry_path, &target, &mut *report)?,
            }
        };
        match outcome {
            Outcome::Cancelled => return Ok(Outcome::Cancelled),
            Outcome::Skipped => skipped = true,
            Outcome::Done => {}
        }
    }

    // Last, once everything is inside: a read-only directory given its mode
    // first would refuse the files being copied into it. Allowed to fail, as
    // for files.
    if let Ok(metadata) = fs::metadata(source) {
        keep_dir_times(dest, &metadata);
        let _ = fs::set_permissions(dest, metadata.permissions());
    }
    Ok(if skipped { Outcome::Skipped } else { Outcome::Done })
}

/// Move `source` to `dest`. Where something is already at `dest`, it goes
/// by way of move_across: a file in the way is asked about, and a directory
/// moved onto a directory is merged into it - a rename cannot replace a
/// directory that holds anything, so that goes entry by entry.
pub fn move_path(source: PathBuf, dest: PathBuf, report: Report<'_>) -> Result<Transfer, Error> {
    if path_exists(&dest) {
        return Ok(move_across(&source, &dest, report)?.into());
    }

    // Try rename first (fast, same filesystem)
    let renamed = attempt(&source, report, |_| match rename(&source, &dest) {
        Ok(()) => Ok(true),
        Err(e) if is_cross_device(&e) => Ok(false),
        Err(e) => Err(e),
    })?;
    match renamed {
        Some(true) | None => Ok(Transfer::Done),
        Some(false) => Ok(move_across(&source, &dest, report)?.into()),
    }
}

/// Move what one rename could not: across filesystems, or into a directory
/// already there. Entry by entry, each source removed only once its copy is
/// whole - so a skip, a cancel or an error partway leaves every entry either
/// moved or where it was, and never gone from both. A directory with anything
/// skipped inside stays behind, holding what was not moved.
fn move_across(source: &Path, dest: &Path, report: Report<'_>) -> Result<Outcome, Error> {
    let Some(metadata) = attempt(source, report, |_| fs::symlink_metadata(source))? else {
        return Ok(Outcome::Skipped);
    };
    // Another name for the very same file - a hard link to it. It is there
    // already, so what is left of the move is to take away the name it had,
    // as mv does. A rename between the two would report success and do
    // nothing, leaving the source where it was and its directory refusing to
    // go as not empty. Nothing would be written over, so nothing is asked.
    if !metadata.is_dir() && is_same_entry(source, dest) {
        let bytes = if metadata.is_file() { metadata.len() } else { 0 };
        if !report.step(Step::Starting(source)) || !report.step(Step::Advanced(bytes)) {
            return Ok(Outcome::Cancelled);
        }
        let removed = attempt(source, report, |_| remove_file(source))?;
        return Ok(if removed.is_some() { Outcome::Done } else { Outcome::Skipped });
    }

    // Asked first, since the rename below would replace it without a word -
    // and asked where to: over it, or beside it.
    let dest = if metadata.is_dir() {
        dest.to_path_buf()
    } else {
        match settle_clash(source, dest, report) {
            Ok(target) => target,
            Err(outcome) => return Ok(outcome),
        }
    };
    let dest = dest.as_path();

    // Within one filesystem - merging into a directory already there - a
    // rename does it at once: a file replaces the one in the way, and a
    // directory goes whole when nothing is in its way. Across filesystems it
    // fails at once, and costs nothing to try.
    if rename(source, dest).is_ok() {
        if !metadata.is_dir() {
            let bytes = if metadata.file_type().is_symlink() { 0 } else { metadata.len() };
            if !report.step(Step::Starting(source)) || !report.step(Step::Advanced(bytes)) {
                return Ok(Outcome::Cancelled);
            }
        }
        return Ok(Outcome::Done);
    }

    if !metadata.is_dir() {
        let copied = if metadata.file_type().is_symlink() { copy_link(source, dest, report)? } else { copy_file(source, dest, report)? };
        if copied != Outcome::Done {
            return Ok(copied);
        }
        let removed = attempt(source, report, |_| remove_file(source))?;
        return Ok(if removed.is_some() { Outcome::Done } else { Outcome::Skipped });
    }

    if attempt(dest, report, |_| fs::create_dir_all(dest))?.is_none() {
        return Ok(Outcome::Skipped);
    }
    let Some(entries) = attempt(source, report, |_| read_dir(source))? else {
        return Ok(Outcome::Skipped);
    };
    let mut skipped = false;
    for entry in entries {
        let Some((entry, _)) = next_entry(source, entry, report)? else {
            skipped = true;
            continue;
        };
        match move_across(&entry.path(), &dest.join(entry.file_name()), &mut *report)? {
            Outcome::Cancelled => return Ok(Outcome::Cancelled),
            Outcome::Skipped => skipped = true,
            Outcome::Done => {}
        }
    }
    if skipped {
        return Ok(Outcome::Skipped);
    }

    keep_dir_times(dest, &metadata);
    let _ = fs::set_permissions(dest, metadata.permissions());
    let removed = attempt(source, report, |_| remove_dir(source))?;
    Ok(if removed.is_some() { Outcome::Done } else { Outcome::Skipped })
}

/// A directory itself, not a link to one.
fn is_real_dir(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|metadata| metadata.is_dir())
}

/// Where each source goes when copied or moved to `destination`, read the
/// way cp and mv read it. Into a directory that is there, or one named with a
/// separator on the end (`names_dir`), each goes under its own name. Anything
/// else is the new path of the one source being copied - several cannot all
/// take one name - and the directory it would go in has to be there.
pub fn transfer_targets(sources: &[(PathBuf, bool)], destination: &Path, names_dir: bool) -> Result<Vec<(PathBuf, PathBuf, bool)>, String> {
    if destination.is_dir() {
        return Ok(sources.iter().map(|(source, is_dir)| (source.clone(), destination.join(source.file_name().unwrap_or_default()), *is_dir)).collect());
    }
    if names_dir {
        return Err(t!("No such directory: {path}", path = destination.display()));
    }
    let [(source, is_dir)] = sources else {
        return Err(t!("{count} items need a directory to go into, and {path} is not one", count = sources.len(), path = destination.display()));
    };
    match destination.parent() {
        Some(parent) if parent.is_dir() => Ok(vec![(source.clone(), destination.to_path_buf(), *is_dir)]),
        Some(parent) => Err(t!("No such directory: {path}", path = parent.display())),
        None => Err(t!("Cannot copy to {path}", path = destination.display())),
    }
}

/// What a copy or move would write over, checked for every item before any
/// is started - bailing out partway would leave some done and the rest not.
/// Returns the destinations already taken, for the caller to ask about or
/// refuse. What no answer could make safe is an error instead: an item onto
/// itself, which overwriting would empty before reading; a directory into
/// itself, which never ends; and a file and a directory in each other's
/// place, where replacing would throw away a whole tree.
pub fn check_destinations(items: &[(PathBuf, PathBuf, bool)]) -> Result<Vec<PathBuf>, String> {
    let mut taken = Vec::new();
    for (source, dest, _) in items {
        if !path_exists(dest) {
            continue;
        }
        if is_same_entry(source, dest) {
            return Err(t!("\"{path}\" is already there", path = source.display()));
        }
        match (is_real_dir(source), is_real_dir(dest)) {
            (false, true) => return Err(t!("Cannot replace directory {path} with a file", path = dest.display())),
            (true, false) => return Err(t!("Cannot replace file {path} with a directory", path = dest.display())),
            _ => {}
        }
        taken.push(dest.clone());
    }
    // A destination inside the source would be copied into itself, filling the
    // disk.
    if let Some((source, _, _)) = items.iter().find(|(source, dest, _)| copies_into_itself(source, dest)) {
        return Err(t!("Cannot copy \"{path}\" into itself", path = source.display()));
    }
    Ok(taken)
}

/// True for the error a rename gives across filesystems: EXDEV on Unix,
/// ERROR_NOT_SAME_DEVICE on Windows. By kind rather than number - the two are
/// 18 and 17, and 17 on Unix is EEXIST, which is no reason to copy anything.
pub(super) fn is_cross_device(error: &Error) -> bool {
    error.kind() == ErrorKind::CrossesDevices
}

#[cfg(test)]
mod tests;
