//! A file too large for where it goes - past 4 GiB onto a FAT stick - split
//! into `name.001`, `name.002` and on, with `name.sha256` beside them that
//! `sha256sum -c` reads; and the pieces joined back, each checked against
//! it on the way through.

use super::*;

/// The largest file a FAT filesystem holds: a byte short of 4 GiB.
const FAT_LIMIT: u64 = u32::MAX as u64;

/// The largest file the filesystem holding `dir` takes, when it has a limit
/// short of anything a disk holds - FAT's. None for no limit, or none that
/// can be told.
pub fn max_file_size(dir: &Path) -> Option<u64> {
    #[cfg(test)]
    if let Some(limit) = test_limits::find(dir) {
        return Some(limit);
    }
    fat_filesystem(dir).then_some(FAT_LIMIT)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fat_filesystem(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    /// What statfs calls FAT, vfat and msdos both.
    const MSDOS_SUPER_MAGIC: i64 = 0x4d44;
    let Ok(c_path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return false };
    // SAFETY: a valid NUL-terminated path, and stats read only on success.
    unsafe {
        let mut stats = std::mem::zeroed::<libc::statfs>();
        libc::statfs(c_path.as_ptr(), &mut stats) == 0 && stats.f_type as i64 == MSDOS_SUPER_MAGIC
    }
}

#[cfg(any(target_vendor = "apple", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly"))]
fn fat_filesystem(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return false };
    // SAFETY: a valid NUL-terminated path, and stats read only on success;
    // the type name is NUL-terminated within its array.
    unsafe {
        let mut stats = std::mem::zeroed::<libc::statfs>();
        if libc::statfs(c_path.as_ptr(), &mut stats) != 0 {
            return false;
        }
        let name = std::ffi::CStr::from_ptr(stats.f_fstypename.as_ptr());
        matches!(name.to_bytes(), b"msdos" | b"msdosfs")
    }
}

/// Elsewhere there is no telling without more than std: a copy too big for
/// the drive fails as it is written, and is asked about as any failure is.
#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd", target_os = "dragonfly")))]
fn fat_filesystem(_dir: &Path) -> bool {
    false
}

/// How big each piece is cut, for a filesystem taking files up to `limit`:
/// whole mebibytes, as split's own sizes read, where the limit allows.
pub fn piece_size(limit: u64) -> u64 {
    const MIB: u64 = 1024 * 1024;
    if limit >= MIB { limit / MIB * MIB } else { limit.max(1) }
}

/// The `number`th piece of `whole`: `name.001`, wider past 999.
pub fn piece_path(whole: &Path, number: usize) -> PathBuf {
    let mut name = whole.as_os_str().to_os_string();
    name.push(format!(".{number:03}"));
    PathBuf::from(name)
}

/// The checksum file beside the pieces of `whole`.
pub fn sums_path(whole: &Path) -> PathBuf {
    let mut name = whole.as_os_str().to_os_string();
    name.push(".sha256");
    PathBuf::from(name)
}

/// What `first` is the first piece of: the name without its `.001`.
pub fn whole_of(first: &Path) -> Option<PathBuf> {
    let name = first.file_name()?.to_str()?;
    let stem = name.strip_suffix(".001").filter(|stem| !stem.is_empty())?;
    Some(first.with_file_name(stem))
}

/// The pieces `first` starts - itself and the `.002` on after it, as far as
/// they go unbroken - when it is the first of two or more files so named.
pub fn pieces_of(first: &Path) -> Option<Vec<PathBuf>> {
    let whole = whole_of(first)?;
    let is_file = |path: &Path| fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file());
    let pieces: Vec<PathBuf> = (1..).map(|number| piece_path(&whole, number)).take_while(|piece| is_file(piece)).collect();
    (pieces.len() >= 2).then_some(pieces)
}

/// Whether copying `source` to `dest` is the joining of pieces: the first
/// of them, to a name that is not a first piece's - copied to `name.001`,
/// it is that piece copied as it is.
pub fn joining(source: &Path, dest: &Path) -> Option<Vec<PathBuf>> {
    if whole_of(dest).is_some() {
        return None;
    }
    pieces_of(source)
}

fn hex(hasher: sha2::Sha256) -> String {
    use sha2::Digest;
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Split `source` into pieces of `piece` bytes at `dest.001` and on, and
/// write `dest.sha256` beside them. Pieces already there under that name
/// are asked about as one file in the way is; written over, any left from a
/// split into more go too, for the join not to take them in. Each piece is
/// read back and checked when copies are verified. Done with what it
/// wrote, for the journal - nothing when it was left alone.
pub fn split_file(source: &Path, dest: &Path, piece: u64, report: Report<'_>) -> Result<(Transfer, Vec<PathBuf>), Error> {
    let metadata = fs::metadata(source)?;
    let count = metadata.len().div_ceil(piece).max(1) as usize;
    let parts = |whole: &Path| -> Vec<PathBuf> { (1..=count).map(|number| piece_path(whole, number)).chain([sums_path(whole)]).collect() };
    let mut whole = dest.to_path_buf();
    if let Some(taken) = parts(dest).into_iter().find(|part| path_exists(part)) {
        match report.exists(source, &taken) {
            Clash::Replace => {}
            Clash::KeepBoth => whole = free_name_among(dest, |candidate| parts(candidate).iter().any(|part| path_exists(part))),
            Clash::Keep => {
                let carry_on = report.step(Step::Starting(source)) && report.step(Step::Advanced(metadata.len()));
                return Ok((if carry_on { Transfer::Done } else { Transfer::Cancelled }, Vec::new()));
            }
            Clash::Abort => return Ok((Transfer::Cancelled, Vec::new())),
        }
    }
    if !report.step(Step::Starting(source)) {
        return Ok((Transfer::Cancelled, Vec::new()));
    }

    let verify = report.verify();
    let mut file = File::open(source)?;
    let mut written = Vec::new();
    let mut sums = Vec::new();
    for number in 1..=count {
        let path = piece_path(&whole, number);
        let hasher = std::cell::RefCell::new(sha2::Sha256::default());
        let mut reader = Hashing { inner: (&mut file).take(piece), hasher: &hasher };
        let finish = |dst_file: &File, _: &Path| keep_times(dst_file, &metadata);
        let check = |temp: &Path, report: Report<'_>| read_back(temp, &hasher, report);
        let outcome = write_beside(&path, &mut reader, report, &finish, verify.then_some(&check as Check<'_>));
        match outcome {
            Ok(Transfer::Done) => written.push(path.clone()),
            other => {
                // Half a split is no split: what it got as far as goes.
                for path in &written {
                    let _ = remove_file(path);
                }
                return other.map(|transfer| (transfer, Vec::new()));
            }
        }
        sums.extend(crate::checksum::sums_line(&hex(hasher.into_inner()), Path::new(path.file_name().unwrap_or_default())));
    }
    let sums_file = sums_path(&whole);
    write_whole(&sums_file, &sums, path_exists(&sums_file))?;
    written.push(sums_file);
    // Pieces of an earlier split into more, past the last of this one.
    for stale in (count + 1..).map(|number| piece_path(&whole, number)).take_while(|piece| path_exists(piece)) {
        let _ = remove_file(stale);
    }
    Ok((Transfer::Done, written))
}

/// The pieces, read one after another as one file, each checked against the
/// checksum file as its last byte goes by. The first that does not match,
/// is not in it, or is missing from the end, fails the read.
struct Joined<'a> {
    pieces: &'a [PathBuf],
    /// By the name of each piece, what it should hash to. None for pieces
    /// with no checksum file to go by.
    expected: Option<Vec<(PathBuf, String)>>,
    sums: PathBuf,
    at: usize,
    file: Option<File>,
    hasher: sha2::Sha256,
}

impl Joined<'_> {
    fn piece_done(&mut self) -> io::Result<()> {
        let piece = &self.pieces[self.at];
        let name = PathBuf::from(piece.file_name().unwrap_or_default());
        let hash = hex(std::mem::take(&mut self.hasher));
        if let Some(expected) = &self.expected {
            let shown = name.display().to_string();
            match expected.iter().find(|(listed, _)| *listed == name) {
                None => return Err(Error::other(t!("{piece} is not in {sums}", piece = shown, sums = self.sums.display()))),
                Some((_, wanted)) if *wanted != hash => return Err(Error::other(t!("{piece} does not match its checksum", piece = shown))),
                Some(_) => {}
            }
        }
        self.at += 1;
        // Listed, and not there: a piece short at the end.
        if self.at == self.pieces.len()
            && let Some(expected) = &self.expected
            && let Some((missing, _)) = expected.iter().find(|(listed, _)| !self.pieces.iter().any(|piece| piece.file_name() == Some(listed.as_os_str())))
        {
            return Err(Error::other(t!("{piece} is missing", piece = missing.display())));
        }
        Ok(())
    }
}

impl Read for Joined<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        while self.at < self.pieces.len() {
            let file = match &mut self.file {
                Some(file) => file,
                None => self.file.insert(File::open(&self.pieces[self.at])?),
            };
            let read = file.read(buffer)?;
            if read > 0 {
                sha2::Digest::update(&mut self.hasher, &buffer[..read]);
                return Ok(read);
            }
            self.file = None;
            self.piece_done()?;
        }
        Ok(0)
    }
}

/// Join `pieces` into `dest`, each checked on the way through against the
/// checksum file beside them, when there is one. Written beside `dest` and
/// put in place whole, so a piece that fails its check leaves nothing; a
/// failure is asked about, as a copy's is, and Retry starts over.
pub fn join_pieces(pieces: &[PathBuf], dest: &Path, report: Report<'_>) -> Result<Transfer, Error> {
    let first = &pieces[0];
    let sums = whole_of(first).map(|whole| sums_path(&whole)).unwrap_or_default();
    let target = match settle_clash(first, dest, report) {
        Ok(target) => target,
        Err(outcome) => return Ok(Transfer::from(outcome)),
    };
    let metadata = fs::metadata(first)?;
    let joined = attempt(first, report, |report| {
        // What the checksum file says: SHA-256 lines only, as fm84 writes.
        let expected = match fs::read(&sums) {
            Ok(text) => Some(
                crate::checksum::parse_sums(&text)
                    .0
                    .into_iter()
                    .filter(|line| line.algorithm == crate::checksum::Algorithm::Sha256)
                    .map(|line| (line.name, line.hex))
                    .collect(),
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if !report.step(Step::Starting(first)) {
            return Ok(Transfer::Cancelled);
        }
        let mut reader = Joined {
            pieces,
            expected,
            sums: PathBuf::from(sums.file_name().unwrap_or_default()),
            at: 0,
            file: None,
            hasher: sha2::Sha256::default(),
        };
        let finish = |dst_file: &File, _: &Path| keep_times(dst_file, &metadata);
        write_beside(&target, &mut reader, report, &finish, None)
    })?;
    Ok(joined.unwrap_or(Transfer::Done))
}

/// What joining `pieces` writes: all of them, one after another.
pub fn joined_size(pieces: &[PathBuf]) -> u64 {
    pieces.iter().filter_map(|piece| fs::metadata(piece).ok()).map(|metadata| metadata.len()).sum()
}

/// Under test a directory can be given a limit, there being no FAT drive to
/// copy to. Kept by directory, so tests side by side keep their own.
#[cfg(test)]
pub mod test_limits {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static LIMITS: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

    pub fn set(dir: &Path, limit: u64) {
        LIMITS.lock().unwrap().push((dir.to_path_buf(), limit));
    }

    pub(super) fn find(dir: &Path) -> Option<u64> {
        LIMITS.lock().unwrap().iter().find(|(under, _)| dir.starts_with(under)).map(|(_, limit)| *limit)
    }
}
