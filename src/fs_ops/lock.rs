//! Alt+L, the floppy lock: what is chosen encrypted with a password in the
//! age format - safe to share, or to carry on a stick - or a locked file
//! opened again. A job as a pack is, with its bar and Esc, each file
//! written beside where it goes and put there whole once it is.
//!
//! age holds one stream, so a directory goes in as a tar of itself and comes
//! out as that tar, for Enter to go into as any archive. The files are
//! those of `age -p` and rage: any of them opens what another made.

use super::pack::{Counted, PackPlan, Shared, append_tar};
use super::transfer::place;
use super::*;
use age::secrecy::SecretString;
use std::cell::{Cell, RefCell};

/// What a locked file's name ends in.
pub const LOCKED_ENDING: &str = ".age";

/// What a locked directory's name ends in: the tar it went in as, then age.
const LOCKED_DIR_ENDING: &str = ".tar.age";

/// The scrypt work factor locks are made with under test, as log2 of its
/// cost: small, where age's own takes a second.
const TEST_WORK_FACTOR: u8 = 4;

/// Whether `path` is named as a locked file - the ending, in any case. The
/// name only: what is in it is not looked at until it is opened.
pub fn is_locked(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().to_ascii_lowercase().ends_with(LOCKED_ENDING) && name.len() > LOCKED_ENDING.len())
}

/// The name `source` is locked under: a file's with `.age` on the end, a
/// directory's with `.tar.age`.
pub fn locked_name(source: &Path, is_dir: bool) -> OsString {
    let mut name = source.file_name().unwrap_or_default().to_os_string();
    name.push(if is_dir { LOCKED_DIR_ENDING } else { LOCKED_ENDING });
    name
}

/// The name a locked file opens to: its own without `.age`. None for one
/// with nothing before the ending.
pub fn unlocked_name(source: &Path) -> Option<OsString> {
    let name = source.file_name()?.to_string_lossy().into_owned();
    let front = &name[..name.len().checked_sub(LOCKED_ENDING.len())?];
    (is_locked(source) && !front.is_empty()).then(|| OsString::from(front))
}

/// What is to be locked: a file, as long as it is, or a directory walked
/// for the tar it goes in as.
pub enum LockPlan {
    File { path: PathBuf, len: u64 },
    Dir(PackPlan),
}

impl LockPlan {
    /// The bytes read to lock it, for the bar.
    pub fn bytes(&self) -> u64 {
        match self {
            LockPlan::File { len, .. } => *len,
            LockPlan::Dir(plan) => plan.bytes,
        }
    }
}

/// Look at what is to be locked into `dest`: a file's length, or a
/// directory's tree. None when the walk was stopped.
pub fn plan_lock(source: &Path, is_dir: bool, dest: &Path, report: Report<'_>) -> Result<Option<LockPlan>, Error> {
    if is_dir {
        return Ok(plan_pack(&[(source.to_path_buf(), true)], dest, PackFormat::Tar, None, report)?.map(LockPlan::Dir));
    }
    let len = fs::metadata(source)?.len();
    Ok(Some(LockPlan::File { path: source.to_path_buf(), len }))
}

/// Lock what `plan` says into `dest` with `password`. Written beside `dest`
/// and put there once whole; what was there is written over only when
/// `replacing`.
pub fn lock(plan: &LockPlan, dest: &Path, password: &str, replacing: bool, report: Report<'_>) -> Result<Transfer, Error> {
    let (temp, file) = temp_beside(dest)?;
    let cancelled = Cell::new(false);
    let report = RefCell::new(report);
    let written = write_locked(file, plan, password, &report, &cancelled);
    finish_written(written, &temp, dest, replacing, &cancelled)
}

fn write_locked(file: File, plan: &LockPlan, password: &str, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    let mut recipient = age::scrypt::Recipient::new(SecretString::from(password.to_owned()));
    // age's own, worked out to take about a second, is what keeps a short
    // password from being guessed - and would take a second for each lock
    // in the tests.
    if cfg!(test) {
        recipient.set_work_factor(TEST_WORK_FACTOR);
    }
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient)).map_err(Error::other)?;
    let mut out = encryptor.wrap_output(io::BufWriter::new(file))?;
    match plan {
        LockPlan::File { path, len } => {
            if !report.borrow_mut().step(Step::Starting(path)) {
                cancelled.set(true);
                return Err(Error::other("Cancelled"));
            }
            let source = File::open(path)?;
            io::copy(&mut Counted::new(source, *len, report, cancelled), &mut out)?;
        }
        LockPlan::Dir(plan) => {
            let mut tar = tar::Builder::new(out);
            tar.follow_symlinks(false);
            append_tar(&mut tar, &plan.members, report, cancelled)?;
            out = tar.into_inner()?;
        }
    }
    out.finish()?.into_inner().map_err(io::IntoInnerError::into_error)
}

/// Open the locked file `source`, `len` bytes long, into `dest` with
/// `password`. A wrong password, or a file locked to a key rather than a
/// password, is found before anything is written.
pub fn unlock(source: &Path, len: u64, dest: &Path, password: &str, replacing: bool, report: Report<'_>) -> Result<Transfer, Error> {
    if !report.step(Step::Starting(source)) {
        return Ok(Transfer::Cancelled);
    }
    let cancelled = Cell::new(false);
    let report = RefCell::new(report);
    let counted = Counted::new(File::open(source)?, len, &report, &cancelled);
    let decryptor = age::Decryptor::new_buffered(io::BufReader::new(counted)).map_err(|error| unlock_error(error, &cancelled))?;
    if !decryptor.is_scrypt() {
        return Err(Error::new(ErrorKind::InvalidData, t!("Locked to a key, not a password")));
    }
    let identity = age::scrypt::Identity::new(SecretString::from(password.to_owned()));
    let mut plain = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity)).map_err(|error| unlock_error(error, &cancelled))?;
    let (temp, file) = temp_beside(dest)?;
    let mut out = io::BufWriter::new(file);
    let written = io::copy(&mut plain, &mut out).and_then(|_| out.into_inner().map_err(io::IntoInnerError::into_error));
    finish_written(written, &temp, dest, replacing, &cancelled)
}

/// What went wrong opening a locked file, said plainly.
fn unlock_error(error: age::DecryptError, cancelled: &Cell<bool>) -> Error {
    use age::DecryptError;
    match error {
        DecryptError::Io(error) => error,
        _ if cancelled.get() => Error::other("Cancelled"),
        DecryptError::DecryptionFailed | DecryptError::KeyDecryptionFailed | DecryptError::NoMatchingKeys | DecryptError::InvalidMac => Error::new(ErrorKind::PermissionDenied, t!("Wrong password")),
        DecryptError::InvalidHeader | DecryptError::UnknownFormat => Error::new(ErrorKind::InvalidData, t!("Not a file locked with age")),
        other => Error::new(ErrorKind::InvalidData, other.to_string()),
    }
}

/// The file written to `temp`, synced and put at `dest`; or, written only
/// in part, taken away again - a stop said as one, anything else as the
/// error it was.
fn finish_written(written: Result<File, Error>, temp: &Path, dest: &Path, replacing: bool, cancelled: &Cell<bool>) -> Result<Transfer, Error> {
    match written.and_then(|file| file.sync_all()) {
        Ok(()) => {
            place(temp, dest, replacing)?;
            crate::archive::forget_index(dest);
            Ok(Transfer::Done)
        }
        Err(error) => {
            let _ = remove_file(temp);
            if cancelled.get() { Ok(Transfer::Cancelled) } else { Err(error) }
        }
    }
}

#[cfg(test)]
mod tests;
