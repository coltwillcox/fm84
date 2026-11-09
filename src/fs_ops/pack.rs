//! Making an archive of what is chosen - Alt+P - as a copy is made: a job
//! with its bar and Esc, asking about what cannot be read, written beside
//! where it goes and put there whole once it is.

use super::transfer::place;
use super::*;
use std::cell::{Cell, RefCell};

/// What Pack can make: everything fm84 reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackFormat {
    Zip,
    SevenZ,
    Tar,
    TarGz,
    TarXz,
    TarBz2,
    TarZst,
}

impl PackFormat {
    /// In the order Tab goes through them in the dialog.
    pub const ALL: [PackFormat; 7] = [PackFormat::Zip, PackFormat::SevenZ, PackFormat::TarGz, PackFormat::TarXz, PackFormat::TarBz2, PackFormat::TarZst, PackFormat::Tar];

    /// The format a name asks for, by its ending.
    pub fn of(path: &Path) -> Option<Self> {
        use crate::archive::Kind;
        Some(match crate::archive::kind_of(path)? {
            Kind::Zip => PackFormat::Zip,
            Kind::SevenZ => PackFormat::SevenZ,
            Kind::Tar => PackFormat::Tar,
            Kind::TarGz => PackFormat::TarGz,
            Kind::TarXz => PackFormat::TarXz,
            Kind::TarBz2 => PackFormat::TarBz2,
            Kind::TarZst => PackFormat::TarZst,
            Kind::Git => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            PackFormat::Zip => ".zip",
            PackFormat::SevenZ => ".7z",
            PackFormat::Tar => ".tar",
            PackFormat::TarGz => ".tar.gz",
            PackFormat::TarXz => ".tar.xz",
            PackFormat::TarBz2 => ".tar.bz2",
            PackFormat::TarZst => ".tar.zst",
        }
    }

    /// Zip and 7z keep names as text; a tar keeps them as the bytes they are.
    pub(super) fn needs_utf8(self) -> bool {
        matches!(self, PackFormat::Zip | PackFormat::SevenZ)
    }

    /// The archive kind it is read back as.
    pub(super) fn kind(self) -> crate::archive::Kind {
        use crate::archive::Kind;
        match self {
            PackFormat::Zip => Kind::Zip,
            PackFormat::SevenZ => Kind::SevenZ,
            PackFormat::Tar => Kind::Tar,
            PackFormat::TarGz => Kind::TarGz,
            PackFormat::TarXz => Kind::TarXz,
            PackFormat::TarBz2 => Kind::TarBz2,
            PackFormat::TarZst => Kind::TarZst,
        }
    }

    /// Whether the level means anything to it: a plain tar only lays files
    /// end to end, and the pure-Rust zstd encoder has the one level.
    pub fn has_levels(self) -> bool {
        !matches!(self, PackFormat::Tar | PackFormat::TarZst)
    }
}

/// How hard the archive is compressed: each format's own levels 1, 6 and 9,
/// 6 being what each of them does when not told.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PackLevel {
    Fast,
    #[default]
    Normal,
    Best,
}

impl PackLevel {
    pub const ALL: [PackLevel; 3] = [PackLevel::Fast, PackLevel::Normal, PackLevel::Best];

    pub(super) fn preset(self) -> u32 {
        match self {
            PackLevel::Fast => 1,
            PackLevel::Normal => 6,
            PackLevel::Best => 9,
        }
    }

    /// The one after, or before, going round.
    pub fn step(self, forward: bool) -> Self {
        let at = Self::ALL.iter().position(|level| *level == self).unwrap_or(1);
        let len = Self::ALL.len();
        Self::ALL[if forward { (at + 1) % len } else { (at + len - 1) % len }]
    }
}

/// One thing going into the archive: where it is, the name it goes in under,
/// and what it is.
pub struct Member {
    pub(super) path: PathBuf,
    pub(super) name: PathBuf,
    pub(super) kind: MemberKind,
    pub(super) metadata: fs::Metadata,
    /// Asked about, as it was being written or before, and not written.
    pub(super) left_out: Cell<bool>,
    /// A directory the archive already has: only what is in it goes in.
    pub(super) in_place: Cell<bool>,
}

impl Member {
    /// Whether it is written into the archive.
    fn goes_in(&self) -> bool {
        !self.left_out.get() && !self.in_place.get()
    }
}

pub(super) enum MemberKind {
    Dir,
    File,
    Link(PathBuf),
}

/// Everything that will go in, found before anything is written.
pub struct PackPlan {
    pub(super) members: Vec<Member>,
    /// The bytes of the files, for the bar.
    pub bytes: u64,
}

/// Walk what is to be packed, links not followed. Each source goes in under
/// its own name, and a directory with everything in it. Done before the
/// archive is begun, so an archive made inside a directory being packed is
/// not found and packed into itself; one already at `dest` is left out too.
/// None when cancelled.
pub fn plan_pack(sources: &[(PathBuf, bool)], dest: &Path, format: PackFormat, leave_out: Option<&crate::glob::Glob>, report: Report<'_>) -> Result<Option<PackPlan>, Error> {
    let named: Vec<(PathBuf, PathBuf)> = sources.iter().filter_map(|(source, _)| Some((source.clone(), PathBuf::from(source.file_name()?)))).collect();
    plan_named(&named, dest, format, leave_out, report)
}

/// As plan_pack, each source going in under the name given with it.
/// `leave_out` is matched against each name as it goes in - `docs/a.txt` -
/// and what it matches is not walked, nor is anything under it.
pub(super) fn plan_named(sources: &[(PathBuf, PathBuf)], dest: &Path, format: PackFormat, leave_out: Option<&crate::glob::Glob>, report: Report<'_>) -> Result<Option<PackPlan>, Error> {
    let mut members = Vec::new();
    let mut walking = Walking { dest, format, leave_out, members: &mut members };
    for (source, name) in sources {
        if !walk(source, name.clone(), &mut walking, report)? {
            return Ok(None);
        }
    }
    let bytes = members.iter().filter(|member| matches!(member.kind, MemberKind::File)).map(|member| member.metadata.len()).sum();
    Ok(Some(PackPlan { members, bytes }))
}

/// What every step of a walk goes by, and what it finds.
struct Walking<'w> {
    dest: &'w Path,
    format: PackFormat,
    leave_out: Option<&'w crate::glob::Glob>,
    members: &'w mut Vec<Member>,
}

/// False when cancelled.
fn walk(path: &Path, name: PathBuf, walking: &mut Walking<'_>, report: Report<'_>) -> Result<bool, Error> {
    let (dest, format) = (walking.dest, walking.format);
    // Asked between entries, so Esc stops a walk through a large tree.
    if !report.step(Step::Advanced(0)) {
        return Ok(false);
    }
    if path == dest {
        return Ok(true);
    }
    if let Some(glob) = walking.leave_out {
        // A link is never a directory here: it goes in as a link.
        let is_dir = path.symlink_metadata().is_ok_and(|metadata| metadata.is_dir());
        if glob.matches(&name.to_string_lossy(), is_dir) {
            return Ok(true);
        }
    }
    let members = &mut *walking.members;
    if format.needs_utf8() && name.to_str().is_none() {
        let refused = Error::new(ErrorKind::InvalidData, t!("{name} is not a name a {format} can hold: it is not UTF-8", name = name.display(), format = format.extension()));
        attempt(path, report, |_| -> Result<(), Error> { Err(Error::new(refused.kind(), refused.to_string())) })?;
        return Ok(true);
    }
    let Some(metadata) = attempt(path, report, |_| fs::symlink_metadata(path))? else {
        return Ok(true);
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        if let Some(target) = attempt(path, report, |_| fs::read_link(path))? {
            members.push(Member {
                path: path.to_path_buf(),
                name,
                kind: MemberKind::Link(target),
                metadata,
                left_out: Cell::new(false),
                in_place: Cell::new(false),
            });
        }
    } else if file_type.is_dir() {
        members.push(Member {
            path: path.to_path_buf(),
            name: name.clone(),
            kind: MemberKind::Dir,
            metadata,
            left_out: Cell::new(false),
            in_place: Cell::new(false),
        });
        let Some(entries) = attempt(path, report, |_| read_dir(path))? else {
            return Ok(true);
        };
        for entry in entries {
            let Some((entry, _)) = next_entry(path, entry, report)? else {
                continue;
            };
            if !walk(&entry.path(), name.join(entry.file_name()), walking, report)? {
                return Ok(false);
            }
        }
    } else if file_type.is_file() {
        // A 7z declares every file before it reads one, so one that cannot
        // be read is asked about now, while it can still be left out.
        if format == PackFormat::SevenZ && attempt(path, report, |_| File::open(path))?.is_none() {
            return Ok(true);
        }
        members.push(Member {
            path: path.to_path_buf(),
            name,
            kind: MemberKind::File,
            metadata,
            left_out: Cell::new(false),
            in_place: Cell::new(false),
        });
    } else {
        // A pipe or a device: nothing a reader of the archive could use.
        attempt(path, report, |_| -> Result<(), Error> { Err(not_regular(path)) })?;
    }
    Ok(true)
}

/// Write the archive `plan` describes to `dest`, written over only when
/// `replacing` - the caller has been told it may - and with `password`, for
/// a zip or a 7z, encrypted: AES-256, and a 7z's names with it.
pub fn pack(plan: &PackPlan, dest: &Path, format: PackFormat, level: PackLevel, password: Option<&str>, replacing: bool, report: Report<'_>) -> Result<Transfer, Error> {
    let (temp, file) = temp_beside(dest)?;
    let cancelled = Cell::new(false);
    let report = RefCell::new(report);
    let written = match format {
        PackFormat::Zip => write_zip(file, &plan.members, level, password, &report, &cancelled),
        PackFormat::SevenZ => write_7z(file, &plan.members, level, password, &report, &cancelled),
        _ => Compressor::new(file, format, level).and_then(|compressor| write_tar(compressor, &plan.members, &report, &cancelled)).and_then(Compressor::finish),
    };
    match written.and_then(|file| file.sync_all()) {
        Ok(()) => {
            place(&temp, dest, replacing)?;
            Ok(Transfer::Done)
        }
        Err(error) => {
            let _ = remove_file(&temp);
            if cancelled.get() { Ok(Transfer::Cancelled) } else { Err(error) }
        }
    }
}

/// Move into the archive: what went into it taken off the disk, now that the
/// archive holding it is there. Only what is as it was walked - a file
/// changed since stays, as does anything that turned up in a directory
/// meanwhile and so each directory still holding something. Deleted, not
/// put in the trash: the archive is where it went.
pub fn remove_packed(plan: &PackPlan, report: Report<'_>) -> Result<Transfer, Error> {
    // Backwards: what is in a directory before the directory.
    for member in plan.members.iter().rev() {
        if member.left_out.get() {
            continue;
        }
        if !report.step(Step::Starting(&member.path)) {
            return Ok(Transfer::Cancelled);
        }
        match member.kind {
            // Still holding something - left out, or new - it stays.
            MemberKind::Dir => {
                let _ = fs::remove_dir(&member.path);
            }
            MemberKind::File | MemberKind::Link(_) => {
                let unchanged = fs::symlink_metadata(&member.path).is_ok_and(|now| now.file_type() == member.metadata.file_type() && now.len() == member.metadata.len() && now.modified().ok() == member.metadata.modified().ok());
                if unchanged {
                    attempt(&member.path, report, |_| remove_file(&member.path))?;
                }
            }
        }
    }
    Ok(Transfer::Done)
}

pub(super) type Shared<'r, 'p> = RefCell<&'r mut (dyn Progress + 'p)>;

/// A file opened to go in, announced; None when it was asked about and left
/// out. Cancelled when the report says to stop.
fn open_member(member: &Member, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<Option<File>, Error> {
    let mut report = report.borrow_mut();
    let Some(file) = attempt(&member.path, &mut **report, |_| File::open(&member.path))? else {
        member.left_out.set(true);
        return Ok(None);
    };
    if !report.step(Step::Starting(&member.path)) {
        cancelled.set(true);
        return Err(Error::new(ErrorKind::Interrupted, "Cancelled"));
    }
    Ok(Some(file))
}

/// A file's contents, counted onto the bar as they are read, ending in an
/// error when the bar says to stop. Exactly as long as the file was when it
/// was walked: an archive says how long each file is before its bytes, and a
/// file that changed meanwhile would leave it saying something else.
pub(super) struct Counted<'s, 'r, 'p, R> {
    inner: R,
    left: u64,
    report: &'s Shared<'r, 'p>,
    cancelled: &'s Cell<bool>,
}

impl<'s, 'r, 'p, R: Read> Counted<'s, 'r, 'p, R> {
    pub(super) fn new(inner: R, len: u64, report: &'s Shared<'r, 'p>, cancelled: &'s Cell<bool>) -> Self {
        Counted { inner, left: len, report, cancelled }
    }
}

impl<R: Read> Read for Counted<'_, '_, '_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buf[..room])?;
        if read == 0 && self.left > 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, t!("It got shorter while it was being packed")));
        }
        self.left -= read as u64;
        if !self.report.borrow_mut().step(Step::Advanced(read as u64)) {
            // Not Interrupted: a copy loop would take that as a reason to
            // read again.
            self.cancelled.set(true);
            return Err(Error::other("Cancelled"));
        }
        Ok(read)
    }
}

/// The tar formats: a tar written through whatever compresses it.
pub(super) enum Compressor {
    Plain(io::BufWriter<File>),
    Gz(flate2::write::GzEncoder<io::BufWriter<File>>),
    Xz(Box<lzma_rust2::XzWriter<io::BufWriter<File>>>),
    Bz2(bzip2::write::BzEncoder<io::BufWriter<File>>),
    Zst(ZstdWriter),
}

impl Compressor {
    pub(super) fn new(file: File, format: PackFormat, level: PackLevel) -> Result<Self, Error> {
        let file = io::BufWriter::new(file);
        let preset = level.preset();
        Ok(match format {
            PackFormat::TarGz => Compressor::Gz(flate2::write::GzEncoder::new(file, flate2::Compression::new(preset))),
            PackFormat::TarXz => Compressor::Xz(Box::new(lzma_rust2::XzWriter::new(file, lzma_rust2::XzOptions::with_preset(preset))?)),
            PackFormat::TarBz2 => Compressor::Bz2(bzip2::write::BzEncoder::new(file, bzip2::Compression::new(preset))),
            PackFormat::TarZst => Compressor::Zst(ZstdWriter::new(file)),
            _ => Compressor::Plain(file),
        })
    }

    /// The end of the stream written, and the file it went to.
    pub(super) fn finish(self) -> Result<File, Error> {
        let buffered = match self {
            Compressor::Plain(file) => file,
            Compressor::Gz(gz) => gz.finish()?,
            Compressor::Xz(xz) => xz.finish()?,
            Compressor::Bz2(bz) => bz.finish()?,
            Compressor::Zst(zst) => zst.finish()?,
        };
        buffered.into_inner().map_err(io::IntoInnerError::into_error)
    }
}

impl Write for Compressor {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Compressor::Plain(file) => file.write(buf),
            Compressor::Gz(gz) => gz.write(buf),
            Compressor::Xz(xz) => xz.write(buf),
            Compressor::Bz2(bz) => bz.write(buf),
            Compressor::Zst(zst) => zst.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Compressor::Plain(file) => file.flush(),
            Compressor::Gz(gz) => gz.flush(),
            Compressor::Xz(xz) => xz.flush(),
            Compressor::Bz2(bz) => bz.flush(),
            Compressor::Zst(zst) => zst.flush(),
        }
    }
}

/// zstd through ruzstd, the one pure-Rust encoder, which pulls what it
/// compresses from a reader and panics when that reader or its writer
/// fails. So it runs on a thread of its own, fed through a channel, writing
/// through a drain that never fails: the first failure is kept, the rest
/// thrown away and the feed ended, and it is told once the encoder is done.
pub(super) struct ZstdWriter {
    pending: Vec<u8>,
    sender: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    encoder: Option<std::thread::JoinHandle<Result<io::BufWriter<File>, Error>>>,
    failed: std::sync::Arc<AtomicBool>,
}

const ZSTD_PIECE: usize = 128 * 1024;

impl ZstdWriter {
    fn new(file: io::BufWriter<File>) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Vec<u8>>(4);
        let failed = std::sync::Arc::new(AtomicBool::new(false));
        let drain_failed = std::sync::Arc::clone(&failed);
        let encoder = std::thread::spawn(move || {
            let mut source = ZstdSource {
                receiver,
                piece: Vec::new(),
                at: 0,
                failed: std::sync::Arc::clone(&drain_failed),
            };
            let mut drain = ZstdDrain { inner: file, error: None, failed: drain_failed };
            ruzstd::encoding::compress(&mut source, &mut drain, ruzstd::encoding::CompressionLevel::Fastest);
            match drain.error {
                Some(error) => Err(error),
                None => drain.inner.flush().map(|()| drain.inner),
            }
        });
        ZstdWriter {
            pending: Vec::with_capacity(ZSTD_PIECE),
            sender: Some(sender),
            encoder: Some(encoder),
            failed,
        }
    }

    /// What is pending sent; Err once the encoder has stopped taking it.
    fn send(&mut self) -> io::Result<()> {
        if self.failed.load(Ordering::Relaxed) {
            return Err(Error::other(t!("Could not write the archive")));
        }
        let piece = std::mem::replace(&mut self.pending, Vec::with_capacity(ZSTD_PIECE));
        match &self.sender {
            Some(sender) if sender.send(piece).is_ok() => Ok(()),
            _ => Err(Error::other(t!("Could not write the archive"))),
        }
    }

    /// The end of the stream: the encoder's own error, if it met one, or
    /// the file it wrote to.
    fn finish(mut self) -> io::Result<io::BufWriter<File>> {
        let sent = if self.pending.is_empty() { Ok(()) } else { self.send() };
        self.sender = None;
        let encoded = match self.encoder.take().map(std::thread::JoinHandle::join) {
            Some(Ok(encoded)) => encoded,
            _ => Err(Error::other(t!("Could not write the archive"))),
        };
        // The encoder's error says what went wrong; the send's only that it
        // had stopped.
        let file = encoded?;
        sent?;
        Ok(file)
    }
}

impl Write for ZstdWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let room = buf.len().min(ZSTD_PIECE - self.pending.len());
        self.pending.extend_from_slice(&buf[..room]);
        if self.pending.len() == ZSTD_PIECE {
            self.send()?;
        }
        Ok(room)
    }

    /// Nothing to push through: the encoder writes whole blocks as it goes.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Let go of unfinished - cancelled, or failed - the encoder ends its
/// frame on what it had, and is waited for, so the file it holds is shut
/// before the temporary is removed.
impl Drop for ZstdWriter {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(encoder) = self.encoder.take() {
            let _ = encoder.join();
        }
    }
}

/// What the encoder reads: the pieces sent, ending when nothing more is
/// coming or the drain has failed.
struct ZstdSource {
    receiver: std::sync::mpsc::Receiver<Vec<u8>>,
    piece: Vec<u8>,
    at: usize,
    failed: std::sync::Arc<AtomicBool>,
}

impl Read for ZstdSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.piece.len() {
            if self.failed.load(Ordering::Relaxed) {
                return Ok(0);
            }
            match self.receiver.recv() {
                Ok(piece) => (self.piece, self.at) = (piece, 0),
                Err(_) => return Ok(0),
            }
        }
        let room = buf.len().min(self.piece.len() - self.at);
        buf[..room].copy_from_slice(&self.piece[self.at..self.at + room]);
        self.at += room;
        Ok(room)
    }
}

/// Where the encoder writes: never failing, but keeping the first failure
/// and writing nothing after it.
struct ZstdDrain {
    inner: io::BufWriter<File>,
    error: Option<Error>,
    failed: std::sync::Arc<AtomicBool>,
}

impl Write for ZstdDrain {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.error.is_none()
            && let Err(error) = self.inner.write_all(buf)
        {
            self.error = Some(error);
            self.failed.store(true, Ordering::Relaxed);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.error.is_none()
            && let Err(error) = self.inner.flush()
        {
            self.error = Some(error);
            self.failed.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// A tar keeps what `ls -l` shows - mode, owner, time - and names as bytes.
fn write_tar(compressor: Compressor, members: &[Member], report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<Compressor, Error> {
    let mut tar = tar::Builder::new(compressor);
    tar.follow_symlinks(false);
    append_tar(&mut tar, members, report, cancelled)?;
    tar.into_inner()
}

/// The members that go in, onto the end of a tar being written.
pub(super) fn append_tar<W: Write>(tar: &mut tar::Builder<W>, members: &[Member], report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<(), Error> {
    for member in members.iter().filter(|member| member.goes_in()) {
        let mut header = tar::Header::new_gnu();
        header.set_metadata(&member.metadata);
        match &member.kind {
            MemberKind::Dir => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                tar.append_data(&mut header, &member.name, io::empty())?;
            }
            MemberKind::Link(target) => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                tar.append_link(&mut header, &member.name, target)?;
            }
            MemberKind::File => {
                let Some(file) = open_member(member, report, cancelled)? else {
                    continue;
                };
                let len = member.metadata.len();
                header.set_size(len);
                tar.append_data(&mut header, &member.name, Counted::new(file, len, report, cancelled))?;
            }
        }
    }
    Ok(())
}

/// A name for a zip or a 7z: its parts joined with `/`, which they both use
/// on any system. Checked to be UTF-8 on the walk.
pub(super) fn text_name(name: &Path) -> String {
    name.iter().map(|part| part.to_string_lossy()).collect::<Vec<_>>().join("/")
}

#[cfg(unix)]
pub(super) fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.mode())
}

#[cfg(not(unix))]
pub(super) fn unix_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

/// A zip, deflated, with each file's mode and time - to the even second
/// from 1980 on, which is all a zip can hold.
fn write_zip(file: File, members: &[Member], level: PackLevel, password: Option<&str>, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
    append_zip(&mut zip, members, level, password, report, cancelled)?;
    zip.finish().map_err(crate::archive::zip_error)?.into_inner().map_err(io::IntoInnerError::into_error)
}

/// The members that go in, onto the end of a zip being written.
pub(super) fn append_zip<W: Write + io::Seek>(zip: &mut zip::ZipWriter<W>, members: &[Member], level: PackLevel, password: Option<&str>, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<(), Error> {
    use zip::write::SimpleFileOptions;
    let zip_error = crate::archive::zip_error;
    for member in members.iter().filter(|member| member.goes_in()) {
        let mut options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(Some(i64::from(level.preset())));
        // Each file's contents: a zip keeps its names in the clear.
        if let Some(password) = password
            && matches!(member.kind, MemberKind::File)
        {
            options = options.with_aes_encryption(zip::AesMode::Aes256, password);
        }
        if let Some(time) = member.metadata.modified().ok().and_then(zip_time) {
            options = options.last_modified_time(time);
        }
        if let Some(mode) = unix_mode(&member.metadata) {
            options = options.unix_permissions(mode & 0o7777);
        }
        let name = text_name(&member.name);
        match &member.kind {
            MemberKind::Dir => zip.add_directory(name, options).map_err(zip_error)?,
            MemberKind::Link(target) => zip.add_symlink(name, target.to_string_lossy(), options).map_err(zip_error)?,
            MemberKind::File => {
                let Some(file) = open_member(member, report, cancelled)? else {
                    continue;
                };
                let len = member.metadata.len();
                // Past 4 GiB a zip needs its 64-bit fields, said before the
                // bytes are written.
                zip.start_file(name, options.large_file(len >= u64::from(u32::MAX))).map_err(zip_error)?;
                io::copy(&mut Counted::new(file, len, report, cancelled), zip)?;
            }
        }
    }
    Ok(())
}

pub(super) fn zip_time(time: std::time::SystemTime) -> Option<zip::DateTime> {
    use chrono::{Datelike, Timelike};
    let local = chrono::DateTime::<chrono::Local>::from(time);
    zip::DateTime::from_date_and_time(u16::try_from(local.year()).ok()?, local.month() as u8, local.day() as u8, local.hour() as u8, local.minute() as u8, local.second() as u8).ok()
}

/// A 7z, packed solid as 7-Zip packs one: every file in one LZMA2 stream,
/// opened only as the stream reaches it. A mode goes in the top half of the
/// attributes, flagged as 7-Zip flags it on Unix.
fn write_7z(file: File, members: &[Member], level: PackLevel, password: Option<&str>, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    let sevenz_error = crate::archive::sevenz_error;
    let mut writer = sevenz_writer(file, level, password)?;
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    gather_7z(&mut writer, members, report, cancelled, &mut entries, &mut sources)?;
    if !entries.is_empty() {
        writer.push_archive_entries(entries, sources).map_err(sevenz_error)?;
    }
    writer.finish()
}

/// A 7z writer packing at `level`, and with a password encrypting the
/// stream and the list of names both, as `7z -p -mhe=on` does.
pub(super) fn sevenz_writer(file: File, level: PackLevel, password: Option<&str>) -> Result<sevenz_rust2::ArchiveWriter<File>, Error> {
    use sevenz_rust2::encoder_options::{AesEncoderOptions, Lzma2Options};
    let mut writer = sevenz_rust2::ArchiveWriter::new(file).map_err(crate::archive::sevenz_error)?;
    let lzma2 = Lzma2Options::from_level(level.preset()).into();
    match password {
        Some(password) => {
            writer.set_content_methods(vec![AesEncoderOptions::new(sevenz_rust2::Password::from(password)).into(), lzma2]);
            writer.set_encrypt_header(true);
        }
        None => {
            writer.set_content_methods(vec![lzma2]);
        }
    }
    Ok(writer)
}

/// A mode in a 7z entry's attributes, flagged as 7-Zip flags it on Unix.
pub(super) fn sevenz_mode(entry: &mut sevenz_rust2::ArchiveEntry, mode: u32, is_dir: bool) {
    const UNIX_EXTENSION: u32 = 0x8000;
    const DIRECTORY: u32 = 0x10;
    entry.has_windows_attributes = true;
    entry.windows_attributes = UNIX_EXTENSION | (mode << 16) | if is_dir { DIRECTORY } else { 0 };
}

/// The members that go in, for a 7z being written: each directory pushed
/// at once, and the rest gathered for the one solid stream, each read only
/// as the stream reaches it.
pub(super) fn gather_7z<'m, 's, 'r, 'p>(
    writer: &mut sevenz_rust2::ArchiveWriter<File>,
    members: &'m [Member],
    report: &'s Shared<'r, 'p>,
    cancelled: &'s Cell<bool>,
    entries: &mut Vec<sevenz_rust2::ArchiveEntry>,
    sources: &mut Vec<sevenz_rust2::SourceReader<Lazy<'m, 's, 'r, 'p>>>,
) -> Result<(), Error> {
    use sevenz_rust2::{ArchiveEntry, SourceReader};
    let sevenz_error = crate::archive::sevenz_error;
    for member in members.iter().filter(|member| member.goes_in()) {
        let name = text_name(&member.name);
        let is_dir = matches!(member.kind, MemberKind::Dir);
        let mut entry = if is_dir { ArchiveEntry::new_directory(&name) } else { ArchiveEntry::new_file(&name) };
        if let Some(time) = member.metadata.modified().ok().and_then(|time| sevenz_rust2::NtTime::try_from(time).ok()) {
            entry.last_modified_date = time;
            entry.has_last_modified_date = true;
        }
        if let Some(mode) = unix_mode(&member.metadata) {
            sevenz_mode(&mut entry, mode, is_dir);
        }
        match &member.kind {
            MemberKind::Dir => {
                writer.push_archive_entry(entry, None::<&[u8]>).map_err(sevenz_error)?;
            }
            MemberKind::Link(target) => {
                entries.push(entry);
                sources.push(SourceReader::new(Lazy::Bytes(io::Cursor::new(target.as_os_str().as_encoded_bytes().to_vec()))));
            }
            MemberKind::File => {
                entries.push(entry);
                sources.push(SourceReader::new(Lazy::File { member, opened: None, report, cancelled }));
            }
        }
    }
    Ok(())
}

/// What goes into a 7z's stream: a file, opened when the stream reaches it,
/// a link's target, or an entry of the 7z being written again, as it is
/// read out of it.
pub(super) enum Lazy<'m, 's, 'r, 'p> {
    File {
        member: &'m Member,
        opened: Option<Counted<'s, 'r, 'p, File>>,
        report: &'s Shared<'r, 'p>,
        cancelled: &'s Cell<bool>,
    },
    Bytes(io::Cursor<Vec<u8>>),
    Old(super::repack::Fed<'s, 'r, 'p>),
}

impl Read for Lazy<'_, '_, '_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Lazy::Bytes(bytes) => bytes.read(buf),
            Lazy::Old(fed) => fed.read(buf),
            Lazy::File { member, opened, report, cancelled } => {
                if opened.is_none() {
                    // Tried on the walk; failing now is past leaving it out.
                    let file = match open_member(member, report, cancelled)? {
                        Some(file) => file,
                        None => return Err(Error::other(t!("{path} could not be read after all", path = member.path.display()))),
                    };
                    *opened = Some(Counted::new(file, member.metadata.len(), report, cancelled));
                }
                opened.as_mut().expect("opened above").read(buf)
            }
        }
    }
}

#[cfg(test)]
mod tests;
