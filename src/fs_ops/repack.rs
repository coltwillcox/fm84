//! Writing into an archive - what is copied or moved in, deleted from it or
//! made in it - by writing the archive again beside itself and putting it in
//! its place whole, as Pack does. What stays is carried across as it is: a
//! zip's entries copied still compressed, a tar's with their headers, a 7z's
//! read out of its stream as the new one is written. A cancel, or a failure,
//! leaves the archive as it was.

use super::pack::{Compressor, Counted, Lazy, MemberKind, PackPlan, Shared, append_tar, append_zip, gather_7z, plan_named, sevenz_mode, sevenz_writer, text_name, zip_time};
use super::transfer::place;
use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::mpsc;

/// What is to change in an archive. Every path is from its root.
#[derive(Clone, Debug, Default)]
pub struct Repack {
    /// Taken out, with everything under them.
    pub remove: Vec<PathBuf>,
    /// Put in from the disk: each source, and the name it goes in under.
    pub add: Vec<(PathBuf, PathBuf)>,
    /// Made new and empty: a directory, or a file.
    pub make: Vec<(PathBuf, bool)>,
}

/// The format an archive is written back in, or why it cannot be: a git
/// commit is history, and a `.tar.zst` is read but never written.
pub fn writable_format(archive: &Path) -> Result<PackFormat, String> {
    match crate::archive::kind_of(archive) {
        Some(crate::archive::Kind::TarZst) => Err(t!("A {format} is read but cannot be written", format = ".tar.zst")),
        _ => PackFormat::of(archive).ok_or_else(|| t!("Archives are read-only").to_string()),
    }
}

/// Everything a rewrite needs, worked out before a byte is written.
pub struct RepackPlan {
    /// The archive itself, not a link to it.
    archive: PathBuf,
    format: PackFormat,
    remove: Vec<PathBuf>,
    /// Files in the archive that a file coming in is put in place of.
    replaced: HashSet<PathBuf>,
    added: PackPlan,
    make: Vec<(PathBuf, bool)>,
    /// What the bar goes to: the files kept and the files coming in.
    pub bytes: u64,
}

impl RepackPlan {
    /// Whether the entry at `path` - None for a name fm84 will not take as
    /// a path, kept as it is - is left out of the archive written again.
    fn drops(&self, path: Option<&Path>) -> bool {
        path.is_some_and(|path| !path.as_os_str().is_empty() && (self.replaced.contains(path) || self.remove.iter().any(|removed| path.starts_with(removed))))
    }
}

/// What a rewrite will do. What comes in is walked, as Pack walks it, and
/// each name it would take that the archive has already asked about as a
/// copy asks: a file over a file replaced, kept, or put beside it; a
/// directory over a directory merged. A file and a directory in each
/// other's place is asked about as a failure, and left out. None when
/// cancelled.
pub fn plan_repack(archive: &Path, edits: &Repack, report: Report<'_>) -> Result<Option<RepackPlan>, Error> {
    let format = writable_format(archive).map_err(|message| Error::new(ErrorKind::Unsupported, message))?;
    // Through a link it is the file the link points at that is written.
    let archive = fs::canonicalize(archive)?;
    let index = crate::archive::index(&archive)?;
    let Some(mut added) = plan_named(&edits.add, &archive, format, report)? else {
        return Ok(None);
    };
    let incoming: HashSet<PathBuf> = added.members.iter().map(|member| member.name.clone()).collect();
    let mut replaced = HashSet::new();
    let mut blocked: Vec<PathBuf> = Vec::new();
    for member in &mut added.members {
        if blocked.iter().any(|dir| member.name.starts_with(dir)) {
            member.left_out.set(true);
            continue;
        }
        let Some(at) = index.find(&member.name) else {
            continue;
        };
        let there_dir = index.entry(at).kind == crate::archive::EntryKind::Dir;
        let coming_dir = matches!(member.kind, MemberKind::Dir);
        let dest = archive.join(&member.name);
        match (coming_dir, there_dir) {
            (true, true) => member.in_place.set(true),
            (false, false) => match report.exists(&member.path, &dest) {
                Clash::Replace => {
                    replaced.insert(member.name.clone());
                }
                Clash::Keep => member.left_out.set(true),
                Clash::KeepBoth => member.name = free_name_among(&member.name, |name| index.find(name).is_some() || incoming.contains(name)),
                Clash::Abort => return Ok(None),
            },
            (coming_dir, _) => {
                let message = if coming_dir {
                    t!("Cannot replace file {path} with a directory", path = dest.display())
                } else {
                    t!("Cannot replace directory {path} with a file", path = dest.display())
                };
                attempt(&member.path, report, |_| -> Result<(), Error> { Err(Error::new(ErrorKind::AlreadyExists, message.clone())) })?;
                member.left_out.set(true);
                if coming_dir {
                    blocked.push(member.name.clone());
                }
            }
        }
    }
    for (name, _) in &edits.make {
        if index.find(name).is_some() {
            return Err(Error::new(ErrorKind::AlreadyExists, t!("Already exists: {name}", name = name.display())));
        }
    }
    let mut plan = RepackPlan {
        archive,
        format,
        remove: edits.remove.clone(),
        replaced,
        added,
        make: edits.make.clone(),
        bytes: 0,
    };
    let kept: u64 = index
        .subtree(Path::new(""))
        .into_iter()
        .map(|at| index.entry(at))
        .filter(|entry| entry.kind == crate::archive::EntryKind::File && !plan.drops(Some(&entry.path)))
        .map(|entry| entry.size)
        .sum();
    let coming: u64 = plan.added.members.iter().filter(|member| matches!(member.kind, MemberKind::File) && !member.left_out.get()).map(|member| member.metadata.len()).sum();
    plan.bytes = kept + coming;
    Ok(Some(plan))
}

/// Write the archive again as `plan` says and put it in its place - and,
/// for a move, take what went into it off the disk once it is there.
pub fn repack(plan: &RepackPlan, move_after: bool, report: Report<'_>) -> Result<Transfer, Error> {
    let archive = &plan.archive;
    let (temp, file) = temp_beside(archive)?;
    // Its permissions, so a shared archive stays shared.
    if let Ok(metadata) = fs::metadata(archive) {
        let _ = fs::set_permissions(&temp, metadata.permissions());
    }
    let cancelled = Cell::new(false);
    let shared = RefCell::new(report);
    let level = PackLevel::Normal;
    let written = match plan.format {
        PackFormat::Zip => rezip(plan, file, level, &shared, &cancelled),
        PackFormat::SevenZ => re7z(plan, file, level, &shared, &cancelled),
        format => Compressor::new(file, format, level).and_then(|compressor| retar(plan, compressor, &shared, &cancelled)).and_then(Compressor::finish),
    };
    let report = shared.into_inner();
    match written.and_then(|file| file.sync_all()) {
        Ok(()) => {
            place(&temp, archive, true)?;
            if move_after { remove_packed(&plan.added, report) } else { Ok(Transfer::Done) }
        }
        Err(error) => {
            let _ = remove_file(&temp);
            if cancelled.get() { Ok(Transfer::Cancelled) } else { Err(error) }
        }
    }
}

fn now() -> std::time::SystemTime {
    std::time::SystemTime::now()
}

/// The mode a directory or a file made in an archive is given.
fn made_mode(is_dir: bool) -> u32 {
    if is_dir { 0o755 } else { 0o644 }
}

/// Bytes counted onto the bar; false when it says to stop, which is then
/// remembered.
fn advance(report: &Shared<'_, '_>, cancelled: &Cell<bool>, bytes: u64) -> Result<(), Error> {
    if report.borrow_mut().step(Step::Advanced(bytes)) {
        Ok(())
    } else {
        cancelled.set(true);
        Err(Error::other("Cancelled"))
    }
}

/// A tar written again: each entry kept with its header - owner, mode,
/// time, links of both kinds - and then what comes in.
fn retar(plan: &RepackPlan, compressor: Compressor, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<Compressor, Error> {
    let mut tar = tar::Builder::new(compressor);
    tar.follow_symlinks(false);
    let mut old = tar::Archive::new(crate::archive::tar_reader(&plan.archive, plan.format.kind())?);
    for entry in old.entries()? {
        let mut entry = entry?;
        let raw = entry.path_bytes().into_owned();
        if plan.drops(crate::archive::inner_path(&raw).as_deref()) {
            continue;
        }
        let mut header = entry.header().clone();
        // Named whole in its own header - nearly always - it goes back in
        // byte for byte, a name fm84 would not take as a path with it.
        let link = entry.link_name_bytes().map(|bytes| bytes.into_owned());
        let fits = *header.path_bytes() == *raw && link.as_deref() == header.link_name_bytes().as_deref();
        if fits && header.entry_type() != tar::EntryType::GNUSparse {
            let size = header.entry_size()?;
            tar.append(&header, Counted::new(&mut entry, size, report, cancelled))?;
            continue;
        }
        let path = crate::archive::link_target(&raw);
        match header.entry_type() {
            tar::EntryType::Symlink | tar::EntryType::Link => {
                let target = entry.link_name_bytes().map(|bytes| crate::archive::link_target(&bytes)).unwrap_or_default();
                tar.append_link(&mut header, &path, &target)?;
            }
            kind => {
                // A sparse file is read out whole, and goes back in as one.
                if kind == tar::EntryType::GNUSparse {
                    header.set_entry_type(tar::EntryType::Regular);
                }
                let size = entry.size();
                header.set_size(size);
                tar.append_data(&mut header, &path, Counted::new(&mut entry, size, report, cancelled))?;
            }
        }
    }
    append_tar(&mut tar, &plan.added.members, report, cancelled)?;
    for (name, is_dir) in &plan.make {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(if *is_dir { tar::EntryType::Directory } else { tar::EntryType::Regular });
        header.set_mode(made_mode(*is_dir));
        header.set_mtime(now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs()));
        header.set_size(0);
        // Whoever makes it, as for what comes in from the disk.
        #[cfg(unix)]
        {
            // SAFETY: getuid and getgid cannot fail, and touch nothing.
            let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
            header.set_uid(u64::from(uid));
            header.set_gid(u64::from(gid));
        }
        tar.append_data(&mut header, name, io::empty())?;
    }
    tar.into_inner()
}

/// A zip written again: each entry kept copied as it is stored, still
/// compressed - or encrypted - and then what comes in.
fn rezip(plan: &RepackPlan, file: File, level: PackLevel, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    use zip::write::SimpleFileOptions;
    let zip_error = crate::archive::zip_error;
    let mut zip = zip::ZipWriter::new(io::BufWriter::new(file));
    let mut old = zip::ZipArchive::new(io::BufReader::new(File::open(&plan.archive)?)).map_err(zip_error)?;
    for position in 0..old.len() {
        let entry = old.by_index_raw(position).map_err(zip_error)?;
        if plan.drops(crate::archive::inner_path(entry.name_raw()).as_deref()) {
            continue;
        }
        let size = if entry.is_file() { entry.size() } else { 0 };
        zip.raw_copy_file(entry).map_err(zip_error)?;
        advance(report, cancelled, size)?;
    }
    append_zip(&mut zip, &plan.added.members, level, report, cancelled)?;
    for (name, is_dir) in &plan.make {
        let mut options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(made_mode(*is_dir));
        if let Some(time) = zip_time(now()) {
            options = options.last_modified_time(time);
        }
        if *is_dir {
            zip.add_directory(text_name(name), options).map_err(zip_error)?;
        } else {
            zip.start_file(text_name(name), options).map_err(zip_error)?;
        }
    }
    zip.finish().map_err(zip_error)?.into_inner().map_err(io::IntoInnerError::into_error)
}

/// A 7z written again, solid as Pack makes one: what it kept read out of
/// the old one on a thread of its own, into the new one's stream as it
/// reaches each, and then what comes in.
fn re7z(plan: &RepackPlan, file: File, level: PackLevel, report: &Shared<'_, '_>, cancelled: &Cell<bool>) -> Result<File, Error> {
    use sevenz_rust2::{ArchiveEntry, SourceReader};
    let sevenz_error = crate::archive::sevenz_error;
    let reader = sevenz_rust2::ArchiveReader::new(io::BufReader::new(File::open(&plan.archive)?), sevenz_rust2::Password::empty()).map_err(sevenz_error)?;
    let old: Vec<ArchiveEntry> = reader.archive().files.clone();
    drop(reader);

    let mut writer = sevenz_writer(file, level)?;
    let (sender, receiver) = mpsc::sync_channel(8);
    let feed = Feed { receiver, chunk: RefCell::new((Vec::new(), 0)) };
    let mut entries = Vec::new();
    let mut sources: Vec<SourceReader<Lazy<'_, '_, '_, '_>>> = Vec::new();
    let mut streamed = Vec::new();
    for (position, was) in old.iter().enumerate() {
        // What an earlier update deleted: gone with this one.
        if was.is_anti_item || plan.drops(crate::archive::inner_path(was.name.replace('\\', "/").as_bytes()).as_deref()) {
            continue;
        }
        let mut entry = if was.is_directory { ArchiveEntry::new_directory(&was.name) } else { ArchiveEntry::new_file(&was.name) };
        entry.has_last_modified_date = was.has_last_modified_date;
        entry.last_modified_date = was.last_modified_date;
        entry.has_creation_date = was.has_creation_date;
        entry.creation_date = was.creation_date;
        entry.has_access_date = was.has_access_date;
        entry.access_date = was.access_date;
        entry.has_windows_attributes = was.has_windows_attributes;
        entry.windows_attributes = was.windows_attributes;
        if was.is_directory {
            writer.push_archive_entry(entry, None::<&[u8]>).map_err(sevenz_error)?;
        } else if was.has_stream && was.size > 0 {
            streamed.push(position);
            entries.push(entry);
            sources.push(SourceReader::new(Lazy::Old(Fed {
                feed: &feed,
                position,
                left: was.size,
                report,
                cancelled,
            })));
        } else {
            entries.push(entry);
            sources.push(SourceReader::new(Lazy::Bytes(io::Cursor::new(Vec::new()))));
        }
    }
    gather_7z(&mut writer, &plan.added.members, report, cancelled, &mut entries, &mut sources)?;
    for (name, is_dir) in &plan.make {
        let name = text_name(name);
        let mut entry = if *is_dir { ArchiveEntry::new_directory(&name) } else { ArchiveEntry::new_file(&name) };
        if let Ok(time) = sevenz_rust2::NtTime::try_from(now()) {
            entry.last_modified_date = time;
            entry.has_last_modified_date = true;
        }
        if cfg!(unix) {
            sevenz_mode(&mut entry, made_mode(*is_dir), *is_dir);
        }
        if *is_dir {
            writer.push_archive_entry(entry, None::<&[u8]>).map_err(sevenz_error)?;
        } else {
            entries.push(entry);
            sources.push(SourceReader::new(Lazy::Bytes(io::Cursor::new(Vec::new()))));
        }
    }

    let archive = plan.archive.clone();
    let reading = std::thread::spawn(move || feed_7z(&archive, &streamed, &sender));
    let pushed = if entries.is_empty() { Ok(()) } else { writer.push_archive_entries(entries, sources).map(|_| ()).map_err(sevenz_error) };
    // Done with, the feed is let go of, and the thread stops at its next send.
    drop(feed);
    let _ = reading.join();
    pushed?;
    writer.finish()
}

/// A piece of an entry being carried across, by its position; an empty
/// piece ends it.
type Piece = io::Result<(usize, Vec<u8>)>;

/// The thread side of re7z: the entries at `positions` read in the old
/// archive's own order - the order the new one takes them in - and sent in
/// pieces, each ended with an empty one. Stops when nobody takes them.
fn feed_7z(archive: &Path, positions: &[usize], sender: &mpsc::SyncSender<Piece>) {
    let mut gone = false;
    let read = crate::archive::read_entries(archive, crate::archive::Kind::SevenZ, positions, &mut |position, contents| {
        let sent = match contents {
            Ok(contents) => loop {
                let mut buffer = vec![0; 64 * 1024];
                match contents.read(&mut buffer) {
                    Ok(0) => break sender.send(Ok((position, Vec::new()))),
                    Ok(read) => {
                        buffer.truncate(read);
                        if sender.send(Ok((position, buffer))).is_err() {
                            break Err(mpsc::SendError(Ok((position, Vec::new()))));
                        }
                    }
                    Err(error) => break sender.send(Err(error)),
                }
            },
            Err(error) => sender.send(Err(error)),
        };
        if sent.is_err() {
            gone = true;
            return crate::archive::Next::Stop;
        }
        crate::archive::Next::Go
    });
    if let Err(error) = read
        && !gone
    {
        let _ = sender.send(Err(error));
    }
}

/// Where re7z's entries come in from the thread reading them.
pub(super) struct Feed {
    receiver: mpsc::Receiver<Piece>,
    /// The piece being read from, and how far into it.
    chunk: RefCell<(Vec<u8>, usize)>,
}

/// One entry of the old 7z, as the new one's stream reads it: exactly as
/// long as the old one said, counted onto the bar.
pub(super) struct Fed<'s, 'r, 'p> {
    feed: &'s Feed,
    position: usize,
    left: u64,
    report: &'s Shared<'r, 'p>,
    cancelled: &'s Cell<bool>,
}

impl Read for Fed<'_, '_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 || buf.is_empty() {
            return Ok(0);
        }
        let mut chunk = self.feed.chunk.borrow_mut();
        if chunk.1 >= chunk.0.len() {
            let (position, piece) = match self.feed.receiver.recv() {
                Ok(piece) => piece?,
                Err(_) => return Err(Error::new(ErrorKind::UnexpectedEof, t!("The archive ends before it"))),
            };
            if position != self.position || piece.is_empty() {
                return Err(Error::new(ErrorKind::InvalidData, t!("The archive ends before it")));
            }
            *chunk = (piece, 0);
        }
        let (piece, at) = &mut *chunk;
        let room = buf.len().min(piece.len() - *at).min(usize::try_from(self.left).unwrap_or(usize::MAX));
        buf[..room].copy_from_slice(&piece[*at..*at + room]);
        *at += room;
        self.left -= room as u64;
        // The empty piece that ends it, taken now so the next entry starts
        // on its own first.
        if self.left == 0 && *at >= piece.len() {
            match self.feed.receiver.recv() {
                Ok(Ok((position, end))) if position == self.position && end.is_empty() => {}
                Ok(Err(error)) => return Err(error),
                _ => return Err(Error::new(ErrorKind::InvalidData, t!("The archive ends before it"))),
            }
        }
        drop(chunk);
        advance(self.report, self.cancelled, room as u64)?;
        Ok(room)
    }
}

#[cfg(test)]
mod tests;
