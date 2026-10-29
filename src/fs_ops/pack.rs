//! Making an archive of what is chosen - Alt+P - as a copy is made: a job
//! with its bar and Esc, asking about what cannot be read, written beside
//! where it goes and put there whole once it is.

use super::transfer::place;
use super::*;
use std::cell::{Cell, RefCell};

/// What Pack can make: everything fm84 reads but a zstd tar, whose
/// pure-Rust encoder cannot say when a write fails.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackFormat {
    Zip,
    SevenZ,
    Tar,
    TarGz,
    TarXz,
    TarBz2,
}

impl PackFormat {
    /// In the order Tab goes through them in the dialog.
    pub const ALL: [PackFormat; 6] = [PackFormat::Zip, PackFormat::SevenZ, PackFormat::TarGz, PackFormat::TarXz, PackFormat::TarBz2, PackFormat::Tar];

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
            Kind::TarZst => return None,
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
        }
    }

    /// Zip and 7z keep names as text; a tar keeps them as the bytes they are.
    fn needs_utf8(self) -> bool {
        matches!(self, PackFormat::Zip | PackFormat::SevenZ)
    }
}

/// One thing going into the archive: where it is, the name it goes in under,
/// and what it is.
pub struct Member {
    path: PathBuf,
    name: PathBuf,
    kind: MemberKind,
    metadata: fs::Metadata,
}

enum MemberKind {
    Dir,
    File,
    Link(PathBuf),
}

/// Everything that will go in, found before anything is written.
pub struct PackPlan {
    members: Vec<Member>,
    /// The bytes of the files, for the bar.
    pub bytes: u64,
}

/// Walk what is to be packed, links not followed. Each source goes in under
/// its own name, and a directory with everything in it. Done before the
/// archive is begun, so an archive made inside a directory being packed is
/// not found and packed into itself; one already at `dest` is left out too.
/// None when cancelled.
pub fn plan_pack(sources: &[(PathBuf, bool)], dest: &Path, format: PackFormat, report: Report<'_>) -> Result<Option<PackPlan>, Error> {
    let mut members = Vec::new();
    for (source, _) in sources {
        let Some(name) = source.file_name() else {
            continue;
        };
        if !walk(source, PathBuf::from(name), dest, format, report, &mut members)? {
            return Ok(None);
        }
    }
    let bytes = members.iter().filter(|member| matches!(member.kind, MemberKind::File)).map(|member| member.metadata.len()).sum();
    Ok(Some(PackPlan { members, bytes }))
}

/// False when cancelled.
fn walk(path: &Path, name: PathBuf, dest: &Path, format: PackFormat, report: Report<'_>, members: &mut Vec<Member>) -> Result<bool, Error> {
    // Asked between entries, so Esc stops a walk through a large tree.
    if !report.step(Step::Advanced(0)) {
        return Ok(false);
    }
    if path == dest {
        return Ok(true);
    }
    if format.needs_utf8() && name.to_str().is_none() {
        let refused = Error::new(ErrorKind::InvalidData, format!("{} is not a name a {} can hold: it is not UTF-8", name.display(), format.extension()));
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
            });
        }
    } else if file_type.is_dir() {
        members.push(Member {
            path: path.to_path_buf(),
            name: name.clone(),
            kind: MemberKind::Dir,
            metadata,
        });
        let Some(entries) = attempt(path, report, |_| read_dir(path))? else {
            return Ok(true);
        };
        for entry in entries {
            let Some((entry, _)) = next_entry(path, entry, report)? else {
                continue;
            };
            if !walk(&entry.path(), name.join(entry.file_name()), dest, format, report, members)? {
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
        });
    } else {
        // A pipe or a device: nothing a reader of the archive could use.
        attempt(path, report, |_| -> Result<(), Error> { Err(not_regular(path)) })?;
    }
    Ok(true)
}

/// Write the archive `plan` describes to `dest`, written over only when
/// `replacing` - the caller has been told it may.
pub fn pack(plan: &PackPlan, dest: &Path, format: PackFormat, replacing: bool, report: Report<'_>) -> Result<Transfer, Error> {
    let (temp, file) = temp_beside(dest)?;
    let cancelled = Cell::new(false);
    let report = RefCell::new(report);
    let written = match format {
        PackFormat::Zip => write_zip(file, &plan.members, &report, &cancelled),
        PackFormat::SevenZ => write_7z(file, &plan.members, &report, &cancelled),
        _ => Compressor::new(file, format).and_then(|compressor| write_tar(compressor, &plan.members, &report, &cancelled)).and_then(Compressor::finish),
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

type Shared<'r, 'p> = RefCell<&'r mut (dyn Progress + 'p)>;

/// A file opened to go in, announced; None when it was asked about and left
/// out. Cancelled when the report says to stop.
fn open_member(member: &Member, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<Option<File>, Error> {
    let mut report = report.borrow_mut();
    let Some(file) = attempt(&member.path, &mut **report, |_| File::open(&member.path))? else {
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
struct Counted<'s, 'r, 'p, R> {
    inner: R,
    left: u64,
    report: &'s Shared<'r, 'p>,
    cancelled: &'s Cell<bool>,
}

impl<'s, 'r, 'p, R: Read> Counted<'s, 'r, 'p, R> {
    fn new(inner: R, len: u64, report: &'s Shared<'r, 'p>, cancelled: &'s Cell<bool>) -> Self {
        Counted { inner, left: len, report, cancelled }
    }
}

impl<R: Read> Read for Counted<'_, '_, '_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let read = self.inner.read(&mut buf[..room])?;
        if read == 0 && self.left > 0 {
            return Err(Error::new(ErrorKind::UnexpectedEof, "It got shorter while it was being packed"));
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
enum Compressor {
    Plain(io::BufWriter<File>),
    Gz(flate2::write::GzEncoder<io::BufWriter<File>>),
    Xz(Box<lzma_rust2::XzWriter<io::BufWriter<File>>>),
    Bz2(bzip2::write::BzEncoder<io::BufWriter<File>>),
}

impl Compressor {
    fn new(file: File, format: PackFormat) -> Result<Self, Error> {
        let file = io::BufWriter::new(file);
        Ok(match format {
            PackFormat::TarGz => Compressor::Gz(flate2::write::GzEncoder::new(file, flate2::Compression::default())),
            PackFormat::TarXz => Compressor::Xz(Box::new(lzma_rust2::XzWriter::new(file, lzma_rust2::XzOptions::with_preset(6))?)),
            PackFormat::TarBz2 => Compressor::Bz2(bzip2::write::BzEncoder::new(file, bzip2::Compression::default())),
            _ => Compressor::Plain(file),
        })
    }

    /// The end of the stream written, and the file it went to.
    fn finish(self) -> Result<File, Error> {
        let buffered = match self {
            Compressor::Plain(file) => file,
            Compressor::Gz(gz) => gz.finish()?,
            Compressor::Xz(xz) => xz.finish()?,
            Compressor::Bz2(bz) => bz.finish()?,
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
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Compressor::Plain(file) => file.flush(),
            Compressor::Gz(gz) => gz.flush(),
            Compressor::Xz(xz) => xz.flush(),
            Compressor::Bz2(bz) => bz.flush(),
        }
    }
}

/// A tar keeps what `ls -l` shows - mode, owner, time - and names as bytes.
fn write_tar(compressor: Compressor, members: &[Member], report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<Compressor, Error> {
    let mut tar = tar::Builder::new(compressor);
    tar.follow_symlinks(false);
    for member in members {
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
    tar.into_inner()
}

/// A name for a zip or a 7z: its parts joined with `/`, which they both use
/// on any system. Checked to be UTF-8 on the walk.
fn text_name(name: &Path) -> String {
    name.iter().map(|part| part.to_string_lossy()).collect::<Vec<_>>().join("/")
}

#[cfg(unix)]
fn unix_mode(metadata: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.mode())
}

#[cfg(not(unix))]
fn unix_mode(_metadata: &fs::Metadata) -> Option<u32> {
    None
}

/// A zip, deflated, with each file's mode and time - to the even second
/// from 1980 on, which is all a zip can hold.
fn write_zip(file: File, members: &[Member], report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    use zip::write::SimpleFileOptions;
    let zip_error = crate::archive::zip_error;
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
    for member in members {
        let mut options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
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
                io::copy(&mut Counted::new(file, len, report, cancelled), &mut zip)?;
            }
        }
    }
    zip.finish().map_err(zip_error)?.into_inner().map_err(io::IntoInnerError::into_error)
}

fn zip_time(time: std::time::SystemTime) -> Option<zip::DateTime> {
    use chrono::{Datelike, Timelike};
    let local = chrono::DateTime::<chrono::Local>::from(time);
    zip::DateTime::from_date_and_time(u16::try_from(local.year()).ok()?, local.month() as u8, local.day() as u8, local.hour() as u8, local.minute() as u8, local.second() as u8).ok()
}

/// A 7z, packed solid as 7-Zip packs one: every file in one LZMA2 stream,
/// opened only as the stream reaches it. A mode goes in the top half of the
/// attributes, flagged as 7-Zip flags it on Unix.
fn write_7z(file: File, members: &[Member], report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
    let sevenz_error = crate::archive::sevenz_error;
    let mut writer = ArchiveWriter::new(file).map_err(sevenz_error)?;
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    for member in members {
        let name = text_name(&member.name);
        let mut entry = if matches!(member.kind, MemberKind::Dir) { ArchiveEntry::new_directory(&name) } else { ArchiveEntry::new_file(&name) };
        if let Some(time) = member.metadata.modified().ok().and_then(|time| sevenz_rust2::NtTime::try_from(time).ok()) {
            entry.last_modified_date = time;
            entry.has_last_modified_date = true;
        }
        if let Some(mode) = unix_mode(&member.metadata) {
            const UNIX_EXTENSION: u32 = 0x8000;
            const DIRECTORY: u32 = 0x10;
            entry.has_windows_attributes = true;
            entry.windows_attributes = UNIX_EXTENSION | (mode << 16) | if matches!(member.kind, MemberKind::Dir) { DIRECTORY } else { 0 };
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
    if !entries.is_empty() {
        writer.push_archive_entries(entries, sources).map_err(sevenz_error)?;
    }
    writer.finish()
}

/// What goes into a 7z's stream: a file, opened when the stream reaches it,
/// or a link's target.
enum Lazy<'m, 's, 'r, 'p> {
    File {
        member: &'m Member,
        opened: Option<Counted<'s, 'r, 'p, File>>,
        report: &'s Shared<'r, 'p>,
        cancelled: &'s Cell<bool>,
    },
    Bytes(io::Cursor<Vec<u8>>),
}

impl Read for Lazy<'_, '_, '_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Lazy::Bytes(bytes) => bytes.read(buf),
            Lazy::File { member, opened, report, cancelled } => {
                if opened.is_none() {
                    // Tried on the walk; failing now is past leaving it out.
                    let file = match open_member(member, report, cancelled)? {
                        Some(file) => file,
                        None => return Err(Error::other(format!("{} could not be read after all", member.path.display()))),
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
