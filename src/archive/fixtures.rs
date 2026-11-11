//! Archives made for tests, written with the same crates that read them.

use std::io::Write;
use std::path::{Path, PathBuf};

/// What goes in, by its name as the archive writes it - which a test can make
/// as hostile as it likes.
pub enum Put<'a> {
    Dir,
    File(&'a [u8]),
    Link(&'a str),
}

/// A directory of its own under the system's temporary one, empty.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fm84-archive-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn zip(path: &Path, entries: &[(&str, Put)]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    for (name, put) in entries {
        match put {
            Put::Dir => zip.add_directory(*name, options).unwrap(),
            Put::File(bytes) => {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            Put::Link(target) => zip.add_symlink(*name, *target, options).unwrap(),
        }
    }
    zip.finish().unwrap();
}

/// A tar, compressed as its name says. Names go into the header as bytes,
/// past the checks the crate's own builder makes, so `..` can be written.
pub fn tar(path: &Path, entries: &[(&[u8], Put)]) {
    let mut tar = tar::Builder::new(Vec::new());
    for (name, put) in entries {
        let mut header = tar::Header::new_gnu();
        let field = &mut header.as_gnu_mut().unwrap().name;
        field[..name.len()].copy_from_slice(name);
        header.set_mtime(1_700_000_000);
        let data: &[u8] = match put {
            Put::Dir => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(0o755);
                &[]
            }
            Put::File(bytes) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o640);
                bytes
            }
            Put::Link(target) => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_mode(0o777);
                header.set_link_name(target).unwrap();
                &[]
            }
        };
        header.set_size(data.len() as u64);
        header.set_cksum();
        tar.append(&header, data).unwrap();
    }
    let bytes = tar.into_inner().unwrap();
    std::fs::write(path, compress(path, &bytes)).unwrap();
}

/// `bytes` as the name of `path` says they are compressed.
fn compress(path: &Path, bytes: &[u8]) -> Vec<u8> {
    use super::Kind;
    match super::kind_of(path) {
        Some(Kind::TarGz) => {
            let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            gzip.write_all(bytes).unwrap();
            gzip.finish().unwrap()
        }
        Some(Kind::TarZst) => ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest),
        Some(Kind::TarXz) => {
            let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
            xz.write_all(bytes).unwrap();
            xz.finish().unwrap()
        }
        Some(Kind::TarBz2) => {
            let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
            bz.write_all(bytes).unwrap();
            bz.finish().unwrap()
        }
        _ => bytes.to_vec(),
    }
}

/// A 7z. Solid puts every file in one block, as 7-Zip does by default, so
/// one is only reached by reading through those before it.
pub fn sevenz(path: &Path, entries: &[(&str, Put)], solid: bool) {
    use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
    let mut writer = ArchiveWriter::create(path).unwrap();
    let mut block: (Vec<ArchiveEntry>, Vec<SourceReader<&[u8]>>) = (Vec::new(), Vec::new());
    for (name, put) in entries {
        let (mut entry, data): (ArchiveEntry, &[u8]) = match put {
            Put::Dir => {
                writer.push_archive_entry(ArchiveEntry::new_directory(name), None::<&[u8]>).unwrap();
                continue;
            }
            Put::File(bytes) => (ArchiveEntry::new_file(name), bytes),
            Put::Link(target) => {
                let mut entry = ArchiveEntry::new_file(name);
                entry.has_windows_attributes = true;
                entry.windows_attributes = 0x8000 | (0o120777 << 16);
                (entry, target.as_bytes())
            }
        };
        if solid {
            block.0.push(entry);
            block.1.push(SourceReader::new(data));
        } else {
            entry.has_stream = true;
            writer.push_archive_entry(entry, Some(data)).unwrap();
        }
    }
    if !block.0.is_empty() {
        writer.push_archive_entries(block.0, block.1).unwrap();
    }
    writer.finish().unwrap();
}

/// A Debian package, or anything else in an ar: each member by its name
/// and its bytes.
pub fn ar(path: &Path, members: &[(&str, &[u8])]) {
    let mut bytes = b"!<arch>\n".to_vec();
    for (name, data) in members {
        bytes.extend_from_slice(format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", format!("{name}/"), 1_700_000_000, 0, 0, "100644", data.len()).as_bytes());
        bytes.extend_from_slice(data);
        if data.len() % 2 == 1 {
            bytes.push(b'\n');
        }
    }
    std::fs::write(path, bytes).unwrap();
}

/// A disc image made a sector at a time, for what a tool would not write:
/// names of any kind, directories that loop, files in several pieces.
/// Sectors 16 to 18 are kept for the descriptors `finish` writes.
pub struct Iso {
    sectors: Vec<[u8; 2048]>,
}

impl Iso {
    pub fn new() -> Self {
        Iso { sectors: vec![[0; 2048]; 19] }
    }

    /// Where the next thing put goes.
    pub fn next(&self) -> u32 {
        self.sectors.len() as u32
    }

    /// `bytes` in sectors of their own, and the first of them.
    pub fn put(&mut self, bytes: &[u8]) -> u32 {
        let first = self.next();
        for chunk in bytes.chunks(2048) {
            let mut sector = [0; 2048];
            sector[..chunk.len()].copy_from_slice(chunk);
            self.sectors.push(sector);
        }
        if bytes.is_empty() {
            self.sectors.push([0; 2048]);
        }
        first
    }

    /// A directory record: its name as the image has it, where its bytes
    /// are, how many, 2 in `flags` for a directory and 0x80 for a piece
    /// with more to come, and Rock Ridge's entries after it.
    pub fn record(name: &[u8], extent: u32, size: u32, flags: u8, system_use: &[u8]) -> Vec<u8> {
        let mut record = vec![0u8; 33];
        record[2..6].copy_from_slice(&extent.to_le_bytes());
        record[6..10].copy_from_slice(&extent.to_be_bytes());
        record[10..14].copy_from_slice(&size.to_le_bytes());
        record[14..18].copy_from_slice(&size.to_be_bytes());
        // 2023-11-14 22:13:20 UTC.
        record[18..25].copy_from_slice(&[123, 11, 14, 22, 13, 20, 0]);
        record[25] = flags;
        record[28] = 1;
        record[32] = name.len() as u8;
        record.extend_from_slice(name);
        if name.len().is_multiple_of(2) {
            record.push(0);
        }
        record.extend_from_slice(system_use);
        record[0] = record.len() as u8;
        record
    }

    /// A directory of `records`, with its own two before them, and the
    /// record that names it: `name`, with `system_use`.
    pub fn dir(&mut self, name: &[u8], system_use: &[u8], records: &[Vec<u8>]) -> Vec<u8> {
        let at = self.next();
        let mut bytes = Vec::new();
        let mut own: Vec<Vec<u8>> = vec![Self::record(&[0], at, 0, 2, b"SP\x07\x01\xbe\xef\x00"), Self::record(&[1], at, 0, 2, &[])];
        own.extend(records.iter().cloned());
        let size = 2048 * own.iter().map(Vec::len).sum::<usize>().div_ceil(2048).max(1) as u32;
        own[0] = Self::record(&[0], at, size, 2, b"SP\x07\x01\xbe\xef\x00");
        own[1] = Self::record(&[1], at, size, 2, &[]);
        for record in own {
            // A record never crosses into the next sector.
            if bytes.len() / 2048 != (bytes.len() + record.len() - 1) / 2048 {
                bytes.resize(bytes.len().div_ceil(2048) * 2048, 0);
            }
            bytes.extend_from_slice(&record);
        }
        self.put(&bytes);
        Self::record(name, at, size, 2, system_use)
    }

    /// Rock Ridge's name for a record.
    pub fn nm(name: &str) -> Vec<u8> {
        let mut entry = vec![b'N', b'M', (5 + name.len()) as u8, 1, 0];
        entry.extend_from_slice(name.as_bytes());
        entry
    }

    /// Rock Ridge's mode for a record.
    pub fn px(mode: u32) -> Vec<u8> {
        let mut entry = vec![b'P', b'X', 36, 1];
        entry.extend_from_slice(&mode.to_le_bytes());
        entry.extend_from_slice(&mode.to_be_bytes());
        entry.extend_from_slice(&[0; 24]);
        entry
    }

    /// Rock Ridge's link to `target`, a part at a time.
    pub fn sl(target: &str) -> Vec<u8> {
        let mut components = Vec::new();
        if target.starts_with('/') {
            components.extend_from_slice(&[0x08, 0]);
        }
        for part in target.split('/').filter(|part| !part.is_empty()) {
            match part {
                ".." => components.extend_from_slice(&[0x04, 0]),
                "." => components.extend_from_slice(&[0x02, 0]),
                _ => {
                    components.extend_from_slice(&[0, part.len() as u8]);
                    components.extend_from_slice(part.as_bytes());
                }
            }
        }
        let mut entry = vec![b'S', b'L', (5 + components.len()) as u8, 1, 0];
        entry.extend_from_slice(&components);
        entry
    }

    /// The image, its tree from `root` - and Joliet's from `joliet` - as
    /// the record `dir` gave for each.
    pub fn finish(mut self, path: &Path, root: &[u8], joliet: Option<&[u8]>) {
        let descriptor = |kind: u8, root: &[u8]| {
            let mut sector = [0u8; 2048];
            sector[0] = kind;
            sector[1..6].copy_from_slice(b"CD001");
            sector[6] = 1;
            sector[128..130].copy_from_slice(&2048u16.to_le_bytes());
            sector[130..132].copy_from_slice(&2048u16.to_be_bytes());
            sector[156..156 + root.len().min(34)].copy_from_slice(&root[..root.len().min(34)]);
            if kind == 2 {
                sector[88..91].copy_from_slice(b"%/E");
            }
            sector
        };
        self.sectors[16] = descriptor(1, root);
        let mut end = 17;
        if let Some(joliet) = joliet {
            self.sectors[17] = descriptor(2, joliet);
            end = 18;
        }
        self.sectors[end][0] = 255;
        self.sectors[end][1..6].copy_from_slice(b"CD001");
        std::fs::write(path, self.sectors.concat()).unwrap();
    }
}

/// A name as Joliet writes one: UCS-2, high byte first.
pub fn joliet(name: &str) -> Vec<u8> {
    name.encode_utf16().flat_map(u16::to_be_bytes).collect()
}
