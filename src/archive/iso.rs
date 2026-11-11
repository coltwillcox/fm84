//! ISO 9660, the file system of a CD or DVD image: its directories read
//! where the volume descriptor says the root is, with the names Rock Ridge
//! keeps beside the short ones, or Joliet's in a tree of their own - the
//! short ones only when neither is there.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

/// Where the volume descriptors start, in 2048-byte sectors.
const DESCRIPTORS_AT: u64 = 16;
/// Descriptors looked through for the ones wanted, at most.
const DESCRIPTORS: u64 = 64;
/// Entries read, at most: a damaged or hostile image that loops round or
/// claims millions stops there.
const ENTRIES: usize = 1_000_000;
/// The largest directory read: a real one is a few sectors.
const DIRECTORY_MOST: u32 = 16 << 20;

/// What the image holds at one place in it.
#[derive(Debug)]
pub struct IsoEntry {
    /// From the root, its names joined by `/`, each one checked to be a
    /// plain name.
    pub path: Vec<u8>,
    pub kind: IsoKind,
    pub size: u64,
    /// The pieces of the image its bytes are in, as (offset, length).
    pub spans: Vec<(u64, u64)>,
    pub modified: Option<SystemTime>,
    pub mode: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum IsoKind {
    Dir,
    File,
    Link(Vec<u8>),
}

/// One record of a directory, as far as it is needed.
struct Record {
    name: Vec<u8>,
    is_dir: bool,
    /// Kept in more than one piece, this not the last.
    continues: bool,
    extent: u32,
    size: u32,
    modified: Option<SystemTime>,
    rock: Rock,
}

/// What Rock Ridge says of a record.
#[derive(Default)]
struct Rock {
    name: Option<Vec<u8>>,
    mode: Option<u32>,
    link: Option<Vec<u8>>,
    /// A directory moved elsewhere to keep the tree shallow, which is shown
    /// where it was, not here.
    relocated: bool,
    /// Where the directory that was moved from here is.
    child: Option<u32>,
}

/// Everything in the image at `path`.
pub fn read(path: &Path) -> io::Result<Vec<IsoEntry>> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let not_one = || not_an_image(path);
    let (mut primary, mut joliet) = (None, None);
    for number in DESCRIPTORS_AT..DESCRIPTORS_AT + DESCRIPTORS {
        let mut sector = [0u8; 2048];
        if file.seek(SeekFrom::Start(number * 2048)).is_err() || file.read_exact(&mut sector).is_err() {
            break;
        }
        if &sector[1..6] != b"CD001" {
            break;
        }
        let root: [u8; 34] = sector[156..190].try_into().expect("34 bytes");
        let block = u16::from_le_bytes([sector[128], sector[129]]);
        match sector[0] {
            1 if primary.is_none() => primary = Some((root, block)),
            // Joliet's escape sequence names UCS-2 at one of its levels.
            2 if joliet.is_none() && [b"%/@", b"%/C", b"%/E"].iter().any(|escape| sector[88..120].windows(3).any(|seen| seen == *escape)) => joliet = Some((root, block)),
            255 => break,
            _ => {}
        }
    }
    let (root, block) = primary.ok_or_else(not_one)?;
    let block = if block.is_power_of_two() && (512..=2048).contains(&block) { u64::from(block) } else { 2048 };
    let mut reader = Reader { file, length, block };
    let rock = reader.walk(&root, false).map_err(|_| not_one())?;
    // No Rock Ridge name anywhere: Joliet's, if it is there, are longer.
    if !rock.iter().any(|(_, named)| *named)
        && let Some((root, _)) = joliet
        && let Ok(entries) = reader.walk(&root, true)
    {
        return Ok(entries.into_iter().map(|(entry, _)| entry).collect());
    }
    Ok(rock.into_iter().map(|(entry, _)| entry).collect())
}

struct Reader {
    file: File,
    length: u64,
    block: u64,
}

impl Reader {
    /// The tree under the root record `root`, each entry with whether
    /// Rock Ridge named it.
    fn walk(&mut self, root: &[u8], joliet: bool) -> io::Result<Vec<(IsoEntry, bool)>> {
        let root = parse(root, joliet).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let mut found = Vec::new();
        // A directory listed twice - or inside itself, or one above it - is
        // shown once, where it was met first.
        let mut seen = HashSet::from([root.extent]);
        let mut stack = vec![(Vec::new(), root.extent, Some(root.size))];
        while let Some((dir, extent, size)) = stack.pop() {
            let records = self.directory(extent, size, joliet)?;
            let mut pieces: Vec<(u64, u64)> = Vec::new();
            for record in records {
                if found.len() >= ENTRIES {
                    return Ok(found);
                }
                if record.rock.relocated {
                    continue;
                }
                let named = record.rock.name.is_some();
                let name = match &record.rock.name {
                    Some(name) => name.clone(),
                    None => short_name(&record.name, joliet),
                };
                let at = u64::from(record.extent) * self.block;
                // Every piece but the last says more is to come.
                if !record.is_dir {
                    pieces.push((at.min(self.length), u64::from(record.size).min(self.length.saturating_sub(at))));
                    if record.continues {
                        continue;
                    }
                }
                let spans = std::mem::take(&mut pieces);
                if !plain(&name) {
                    continue;
                }
                let mut path = dir.clone();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(&name);
                let kind = match (&record.rock.link, record.is_dir || record.rock.child.is_some()) {
                    (_, true) => IsoKind::Dir,
                    (Some(link), false) => IsoKind::Link(link.clone()),
                    (None, false) => IsoKind::File,
                };
                if kind == IsoKind::Dir {
                    let (extent, size) = match record.rock.child {
                        Some(child) => (child, None),
                        None => (record.extent, Some(record.size)),
                    };
                    if !seen.insert(extent) {
                        continue;
                    }
                    stack.push((path.clone(), extent, size));
                }
                let size = if kind == IsoKind::File { spans.iter().map(|&(_, length)| length).sum() } else { 0 };
                found.push((
                    IsoEntry {
                        path,
                        spans: if kind == IsoKind::File { spans } else { Vec::new() },
                        kind,
                        size,
                        modified: record.modified,
                        mode: record.rock.mode.map(|mode| mode & 0o7777),
                    },
                    named,
                ));
            }
        }
        Ok(found)
    }

    /// The records of the directory at `extent`, `size` bytes long - or as
    /// long as its own first record says, when that is not known.
    fn directory(&mut self, extent: u32, size: Option<u32>, joliet: bool) -> io::Result<Vec<Record>> {
        let at = u64::from(extent) * self.block;
        let size = match size {
            Some(size) => size,
            None => {
                let first = self.bytes(at, 34)?;
                parse(&first, joliet).map_or(0, |record| record.size)
            }
        };
        let data = self.bytes(at, size.min(DIRECTORY_MOST))?;
        let mut records = Vec::new();
        let mut position = 0;
        let block = self.block as usize;
        while position < data.len() {
            let length = usize::from(data[position]);
            // The rest of this sector is padding: records do not cross one.
            if length == 0 {
                position = (position / block + 1) * block;
                continue;
            }
            let Some(raw) = data.get(position..position + length) else { break };
            position += length;
            let Some(record) = parse(raw, joliet) else { continue };
            // Itself and its parent.
            if record.name == [0] || record.name == [1] {
                continue;
            }
            records.push(record);
        }
        Ok(records)
    }

    /// `length` bytes of the image from `at`, or as many as there are.
    fn bytes(&mut self, at: u64, length: u32) -> io::Result<Vec<u8>> {
        let length = u64::from(length).min(self.length.saturating_sub(at));
        let mut bytes = Vec::new();
        self.file.seek(SeekFrom::Start(at))?;
        (&mut self.file).take(length).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// A directory record, or None for one too short to be one.
fn parse(raw: &[u8], joliet: bool) -> Option<Record> {
    if raw.len() < 34 {
        return None;
    }
    let name_length = usize::from(raw[32]);
    let name = raw.get(33..33 + name_length)?.to_vec();
    // A name of even length is padded to keep what follows on an even byte.
    let system_use = raw.get(33 + name_length + usize::from(name_length % 2 == 0)..).unwrap_or(&[]);
    let le = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
    Some(Record {
        name,
        is_dir: raw[25] & 0x02 != 0,
        continues: raw[25] & 0x80 != 0,
        extent: le(2),
        size: le(10),
        modified: when(&raw[18..25]),
        rock: if joliet { Rock::default() } else { rock(system_use) },
    })
}

/// Rock Ridge's entries in a record's system use area. What is in a
/// continuation area elsewhere is not read: a long name or link there is
/// rare, and the short name stands in.
fn rock(mut area: &[u8]) -> Rock {
    let mut rock = Rock::default();
    let mut link: Option<Vec<u8>> = None;
    // A link's last component was cut short, to go on in the next entry.
    let mut carried = false;
    while area.len() >= 4 {
        let length = usize::from(area[2]);
        if length < 4 || length > area.len() {
            break;
        }
        let (entry, rest) = area.split_at(length);
        let data = &entry[4..];
        match &entry[..2] {
            b"NM" if !data.is_empty() && data[0] & 0x06 == 0 => rock.name.get_or_insert_with(Vec::new).extend_from_slice(&data[1..]),
            b"PX" if data.len() >= 4 => rock.mode = Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            b"RE" => rock.relocated = true,
            b"CL" if data.len() >= 4 => rock.child = Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
            b"SL" if !data.is_empty() => {
                let target = link.get_or_insert_with(Vec::new);
                let mut components = &data[1..];
                while components.len() >= 2 {
                    let (flags, size) = (components[0], usize::from(components[1]));
                    let Some(content) = components.get(2..2 + size) else { break };
                    if !carried && !(target.is_empty() || target == b"/") {
                        target.push(b'/');
                    }
                    match flags & 0x0E {
                        0x02 => target.push(b'.'),
                        0x04 => target.extend_from_slice(b".."),
                        0x08 => target.push(b'/'),
                        _ => target.extend_from_slice(content),
                    }
                    carried = flags & 0x01 != 0;
                    components = &components[2 + size..];
                }
            }
            b"ST" => break,
            _ => {}
        }
        area = rest;
    }
    rock.link = link;
    rock
}

/// The name a record has without Rock Ridge: Joliet's UCS-2, or the short
/// upper-case one, as a system mounting the image shows them - the version
/// after `;` gone, and the dot of a name with no ending, and the case.
fn short_name(raw: &[u8], joliet: bool) -> Vec<u8> {
    let name = if joliet {
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|&pair| u16::from_be_bytes(pair)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).to_ascii_lowercase()
    };
    let name = name.split_once(';').map_or(name.as_str(), |(name, _)| name);
    let name = if joliet { name } else { name.strip_suffix('.').unwrap_or(name) };
    name.as_bytes().to_vec()
}

/// A name that can stand as one part of a path: not empty, not `.` or
/// `..`, with no separator in it.
fn plain(name: &[u8]) -> bool {
    !name.is_empty() && name != b"." && name != b".." && !name.contains(&b'/') && !name.contains(&0)
}

fn not_an_image(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, t!("Not an archive: {path}", path = path.display()))
}

/// A record's date: years since 1900, month, day, hour, minute, second,
/// and the zone in quarter hours from UTC.
fn when(raw: &[u8]) -> Option<SystemTime> {
    use chrono::TimeZone;
    let date = chrono::NaiveDate::from_ymd_opt(1900 + i32::from(raw[0]), raw[1].into(), raw[2].into())?;
    let at = date.and_hms_opt(raw[3].into(), raw[4].into(), raw[5].into())?;
    let zone = chrono::FixedOffset::east_opt(i32::from(raw[6] as i8) * 15 * 60)?;
    zone.from_local_datetime(&at).single().map(SystemTime::from)
}
