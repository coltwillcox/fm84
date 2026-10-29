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
