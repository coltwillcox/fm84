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

/// A tar, gzipped when the name says so. Names go into the header as bytes,
/// past the checks the crate's own builder makes, so `..` can be written.
pub fn tar(path: &Path, entries: &[(&[u8], Put)]) {
    let file = std::fs::File::create(path).unwrap();
    let gzip = path.to_string_lossy().ends_with("gz");
    let writer: Box<dyn Write> = if gzip { Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::fast())) } else { Box::new(file) };
    let mut tar = tar::Builder::new(writer);
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
    tar.into_inner().unwrap().flush().unwrap();
}
