//! What a file says of itself, for multi-rename's `[Exif:date]`,
//! `[Tag:artist]` and `[Pdf:title]`: a photograph's EXIF, a song's tags, a
//! PDF's title. Read from the headers, as little of the file as will do,
//! and in plain Rust - what a format keeps where is simple enough to find.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// What one file holds, by token: `exif:date`, `tag:artist` and the rest.
pub type Fields = HashMap<&'static str, String>;

/// Every field there is a token for. One not among them is no token, and
/// is left in the pattern as it is written.
pub const KNOWN: [&str; 12] = ["exif:date", "exif:day", "exif:time", "exif:make", "exif:model", "tag:artist", "tag:title", "tag:album", "tag:track", "tag:year", "pdf:title", "pdf:author"];

/// The head of a file read for its metadata: EXIF sits at the start of a
/// JPEG, and an ID3 tag before the music.
const HEAD: u64 = 1024 * 1024;

/// What `path` says of itself, by the format its first bytes say it is.
/// Nothing for a file that cannot be read, or says nothing.
pub fn read(path: &Path) -> Fields {
    let mut fields = Fields::new();
    let Ok(mut file) = File::open(path) else {
        return fields;
    };
    let mut head = Vec::new();
    if (&mut file).take(HEAD).read_to_end(&mut head).is_err() {
        return fields;
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        jpeg(&head, &mut fields);
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(&head, &mut fields);
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        webp(&head, &mut fields);
    } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        tiff(&head, &mut fields);
    } else if head.starts_with(b"fLaC") {
        flac(&head, &mut fields);
    } else if head.starts_with(b"OggS") {
        ogg(&head, &mut fields);
    } else if head.starts_with(b"%PDF") {
        pdf(&mut file, &head, &mut fields);
    } else if head.len() >= 8 && &head[4..8] == b"ftyp" {
        mp4(&mut file, &mut fields);
    } else {
        if head.starts_with(b"ID3") {
            id3v2(&head, &mut fields);
        }
        id3v1(&mut file, &mut fields);
    }
    fields
}

/// A value fit to keep: trimmed, without the NULs formats pad with.
fn put(fields: &mut Fields, key: &'static str, value: &str) {
    let value = value.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if !value.is_empty() {
        fields.entry(key).or_insert_with(|| value.to_string());
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

// Photographs.

/// A JPEG's segments up to the picture itself, for the one holding EXIF.
fn jpeg(bytes: &[u8], fields: &mut Fields) {
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        let Some(length) = be16(bytes, at + 2).map(usize::from) else { return };
        // Start of scan: the picture, and nothing more to read before it.
        if marker == 0xDA {
            return;
        }
        let segment = bytes.get(at + 4..at + 2 + length).unwrap_or_default();
        if marker == 0xE1
            && let Some(exif) = segment.strip_prefix(b"Exif\0\0")
        {
            return tiff(exif, fields);
        }
        at += 2 + length;
    }
}

/// A PNG's chunks, for `eXIf`.
fn png(bytes: &[u8], fields: &mut Fields) {
    let mut at = 8;
    while let (Some(length), Some(kind)) = (be32(bytes, at).map(|length| length as usize), bytes.get(at + 4..at + 8)) {
        if kind == b"eXIf" {
            return tiff(bytes.get(at + 8..at + 8 + length).unwrap_or_default(), fields);
        }
        if kind == b"IEND" {
            return;
        }
        at += 12 + length;
    }
}

/// A WebP's chunks, for `EXIF` - written by some with JPEG's prefix kept.
fn webp(bytes: &[u8], fields: &mut Fields) {
    let mut at = 12;
    while let (Some(kind), Some(length)) = (bytes.get(at..at + 4), le32(bytes, at + 4).map(|length| length as usize)) {
        if kind == b"EXIF" {
            let exif = bytes.get(at + 8..at + 8 + length).unwrap_or_default();
            return tiff(exif.strip_prefix(b"Exif\0\0").unwrap_or(exif), fields);
        }
        at += 8 + length + length % 2;
    }
}

/// EXIF as TIFF lays it out: the camera, and when the picture was taken -
/// when it was made, failing that, or last changed.
fn tiff(bytes: &[u8], fields: &mut Fields) {
    let little = match bytes.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return,
    };
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|two| if little { u16::from_le_bytes([two[0], two[1]]) } else { u16::from_be_bytes([two[0], two[1]]) });
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|four| if little { u32::from_le_bytes(four.try_into().unwrap()) } else { u32::from_be_bytes(four.try_into().unwrap()) });
    // An IFD's entries: each tag, and where its value is.
    let entries = |ifd: usize| -> Vec<(u16, u16, u32, usize)> {
        let count = u16_at(ifd).unwrap_or(0) as usize;
        (0..count.min(512))
            .filter_map(|index| {
                let entry = ifd + 2 + index * 12;
                Some((u16_at(entry)?, u16_at(entry + 2)?, u32_at(entry + 4)?, entry + 8))
            })
            .collect()
    };
    let text = |kind: u16, count: u32, value: usize| -> Option<String> {
        if kind != 2 {
            return None;
        }
        let count = count as usize;
        let at = if count <= 4 { value } else { u32_at(value)? as usize };
        Some(String::from_utf8_lossy(bytes.get(at..at + count)?).into_owned())
    };
    let Some(first) = u32_at(4) else { return };
    let (mut taken, mut made, mut changed) = (None, None, None);
    let mut exif_ifd = None;
    for (tag, kind, count, value) in entries(first as usize) {
        match tag {
            0x010F => put(fields, "exif:make", &text(kind, count, value).unwrap_or_default()),
            0x0110 => put(fields, "exif:model", &text(kind, count, value).unwrap_or_default()),
            0x0132 => changed = text(kind, count, value),
            0x8769 => exif_ifd = u32_at(value),
            _ => {}
        }
    }
    if let Some(ifd) = exif_ifd {
        for (tag, kind, count, value) in entries(ifd as usize) {
            match tag {
                0x9003 => taken = text(kind, count, value),
                0x9004 => made = text(kind, count, value),
                _ => {}
            }
        }
    }
    if let Some((day, time)) = taken.or(made).or(changed).as_deref().and_then(exif_date) {
        put(fields, "exif:date", &format!("{day} {time}"));
        put(fields, "exif:day", &day);
        put(fields, "exif:time", &time);
    }
}

/// EXIF's `2026:03:14 18:02:11` as a name can hold it: `2026-03-14` and
/// `18.02.11`. None for one not filled in - cameras write zeros.
fn exif_date(value: &str) -> Option<(String, String)> {
    let value = value.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let (day, time) = value.split_once(' ')?;
    let day: Vec<&str> = day.split(':').collect();
    let time: Vec<&str> = time.split(':').collect();
    let digits = |parts: &[&str], widths: [usize; 3]| parts.len() == 3 && parts.iter().zip(widths).all(|(part, width)| part.len() == width && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !digits(&day, [4, 2, 2]) || !digits(&time, [2, 2, 2]) || day[0] == "0000" {
        return None;
    }
    Some((day.join("-"), time.join(".")))
}

// Music.

/// An ID3v2 tag's text frames, in versions 2.2, 2.3 and 2.4.
fn id3v2(bytes: &[u8], fields: &mut Fields) {
    let Some(&[_, _, _, major, _, flags, a, b, c, d]) = bytes.get(..10).and_then(|head| <&[u8; 10]>::try_from(head).ok()) else {
        return;
    };
    let syncsafe = |four: [u8; 4]| four.iter().fold(0usize, |size, &byte| (size << 7) | usize::from(byte & 0x7F));
    let end = (10 + syncsafe([a, b, c, d])).min(bytes.len());
    let mut at = 10;
    // An extended header, which says nothing wanted here.
    if flags & 0x40 != 0 && major >= 3 {
        let Some(four) = bytes.get(at..at + 4) else { return };
        let four: [u8; 4] = four.try_into().unwrap_or_default();
        at += if major == 4 { syncsafe(four) } else { u32::from_be_bytes(four) as usize + 4 };
    }
    let (id_length, header) = if major == 2 { (3, 6) } else { (4, 10) };
    while at + header <= end {
        let id = &bytes[at..at + id_length];
        if id[0] == 0 {
            return;
        }
        let size = match major {
            2 => (usize::from(bytes[at + 3]) << 16) | (usize::from(bytes[at + 4]) << 8) | usize::from(bytes[at + 5]),
            4 => syncsafe(bytes[at + 4..at + 8].try_into().unwrap_or_default()),
            _ => be32(bytes, at + 4).unwrap_or(0) as usize,
        };
        let body = bytes.get(at + header..(at + header + size).min(end)).unwrap_or_default();
        let key = match id {
            b"TIT2" | b"TT2" => Some("tag:title"),
            b"TPE1" | b"TP1" => Some("tag:artist"),
            b"TALB" | b"TAL" => Some("tag:album"),
            b"TRCK" | b"TRK" => Some("tag:track"),
            b"TYER" | b"TDRC" | b"TYE" => Some("tag:year"),
            _ => None,
        };
        if let (Some(key), Some((&encoding, text))) = (key, body.split_first()) {
            let text = id3_text(encoding, text);
            // Several values are separated by NULs: the first is enough.
            put(fields, key, text.split('\0').next().unwrap_or_default());
        }
        at += header + size;
    }
    tidy_tags(fields);
}

/// An ID3 text frame's text, in the encoding its first byte names.
fn id3_text(encoding: u8, bytes: &[u8]) -> String {
    match encoding {
        1 | 2 => {
            let (big, bytes) = match bytes {
                [0xFE, 0xFF, rest @ ..] => (true, rest),
                [0xFF, 0xFE, rest @ ..] => (false, rest),
                rest => (encoding == 2, rest),
            };
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&pair| if big { u16::from_be_bytes(pair) } else { u16::from_le_bytes(pair) }).collect();
            String::from_utf16_lossy(&units)
        }
        3 => String::from_utf8_lossy(bytes).into_owned(),
        _ => bytes.iter().map(|&byte| char::from(byte)).collect(),
    }
}

/// The old ID3v1 tag in the last 128 bytes, for what a newer one did not say.
fn id3v1(file: &mut File, fields: &mut Fields) {
    let mut tag = [0u8; 128];
    if file.seek(SeekFrom::End(-128)).is_err() || file.read_exact(&mut tag).is_err() || &tag[..3] != b"TAG" {
        return;
    }
    let latin1 = |bytes: &[u8]| bytes.iter().map(|&byte| char::from(byte)).collect::<String>();
    put(fields, "tag:title", &latin1(&tag[3..33]));
    put(fields, "tag:artist", &latin1(&tag[33..63]));
    put(fields, "tag:album", &latin1(&tag[63..93]));
    put(fields, "tag:year", &latin1(&tag[93..97]));
    // ID3v1.1 keeps the track in the comment's last byte, after a NUL.
    if tag[125] == 0 && tag[126] != 0 {
        put(fields, "tag:track", &tag[126].to_string());
    }
    tidy_tags(fields);
}

/// FLAC's metadata blocks, for the one of Vorbis comments.
fn flac(bytes: &[u8], fields: &mut Fields) {
    let mut at = 4;
    while let Some(&kind) = bytes.get(at) {
        let length = (usize::from(*bytes.get(at + 1).unwrap_or(&0)) << 16) | (usize::from(*bytes.get(at + 2).unwrap_or(&0)) << 8) | usize::from(*bytes.get(at + 3).unwrap_or(&0));
        if kind & 0x7F == 4 {
            return vorbis_comments(bytes.get(at + 4..at + 4 + length).unwrap_or_default(), fields);
        }
        if kind & 0x80 != 0 {
            return;
        }
        at += 4 + length;
    }
}

/// An Ogg stream's second packet - Vorbis's comment header, or Opus's tags
/// - put back together from the pages it is spread over.
fn ogg(bytes: &[u8], fields: &mut Fields) {
    let mut packets: Vec<Vec<u8>> = vec![Vec::new()];
    let mut at = 0;
    while packets.len() <= 2 && bytes.get(at..at + 4) == Some(b"OggS") {
        let Some(&count) = bytes.get(at + 26) else { return };
        let table = bytes.get(at + 27..at + 27 + usize::from(count)).unwrap_or_default();
        let mut body = at + 27 + usize::from(count);
        for &lacing in table {
            let segment = bytes.get(body..body + usize::from(lacing)).unwrap_or_default();
            packets.last_mut().expect("never empty").extend_from_slice(segment);
            body += usize::from(lacing);
            // A segment shorter than 255 ends its packet.
            if lacing < 255 {
                packets.push(Vec::new());
            }
        }
        at = body;
    }
    let Some(comments) = packets.get(1) else { return };
    if let Some(rest) = comments.strip_prefix(b"\x03vorbis").or_else(|| comments.strip_prefix(b"OpusTags")) {
        vorbis_comments(rest, fields);
    }
}

/// Vorbis comments, as FLAC and Ogg keep them: `ARTIST=...` and the like.
fn vorbis_comments(bytes: &[u8], fields: &mut Fields) {
    let Some(vendor) = le32(bytes, 0) else { return };
    let mut at = 4 + vendor as usize;
    let count = le32(bytes, at).unwrap_or(0);
    at += 4;
    for _ in 0..count.min(1024) {
        let Some(length) = le32(bytes, at).map(|length| length as usize) else { return };
        let comment = String::from_utf8_lossy(bytes.get(at + 4..at + 4 + length).unwrap_or_default()).into_owned();
        at += 4 + length;
        let Some((name, value)) = comment.split_once('=') else { continue };
        let key = match name.to_ascii_uppercase().as_str() {
            "TITLE" => "tag:title",
            "ARTIST" => "tag:artist",
            "ALBUM" => "tag:album",
            "TRACKNUMBER" => "tag:track",
            "DATE" | "YEAR" => "tag:year",
            _ => continue,
        };
        put(fields, key, value);
    }
    tidy_tags(fields);
}

/// An MP4's tags - iTunes keeps them in moov/udta/meta/ilst, and moov can be
/// at the end, after the music, so the top level is walked by seeking.
fn mp4(file: &mut File, fields: &mut Fields) {
    let mut at = 0u64;
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    while at + 8 <= length {
        let mut header = [0u8; 16];
        if file.seek(SeekFrom::Start(at)).is_err() || file.read_exact(&mut header[..8]).is_err() {
            return;
        }
        let mut size = u64::from(u32::from_be_bytes(header[..4].try_into().unwrap()));
        let mut skip = 8;
        if size == 1 {
            if file.read_exact(&mut header[8..]).is_err() {
                return;
            }
            size = u64::from_be_bytes(header[8..].try_into().unwrap());
            skip = 16;
        } else if size == 0 {
            size = length - at;
        }
        if size < skip {
            return;
        }
        if &header[4..8] == b"moov" {
            let mut moov = Vec::new();
            if (&mut *file).take((size - skip).min(16 * 1024 * 1024)).read_to_end(&mut moov).is_ok() {
                mp4_tags(&moov, fields);
            }
            return;
        }
        at += size;
    }
}

/// The atoms of `bytes`, each its kind and what is inside it.
fn atoms(bytes: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let size = be32(bytes, at)? as usize;
        let kind = bytes.get(at + 4..at + 8)?;
        let body = bytes.get(at + 8..at + size.max(8))?;
        at += size.max(8);
        Some((kind, body))
    })
}

/// What is inside the first atom of `kind` among `bytes`.
fn find<'a>(bytes: &'a [u8], kind: &[u8]) -> Option<&'a [u8]> {
    atoms(bytes).find(|(found, _)| *found == kind).map(|(_, body)| body)
}

fn mp4_tags(moov: &[u8], fields: &mut Fields) {
    let Some(meta) = find(moov, b"udta").and_then(|udta| find(udta, b"meta")) else { return };
    // A version and flags open meta, except as QuickTime writes it.
    let meta = if meta.get(4..8) == Some(b"hdlr") { meta } else { meta.get(4..).unwrap_or_default() };
    let Some(list) = find(meta, b"ilst") else { return };
    for (kind, body) in atoms(list) {
        let Some(data) = find(body, b"data").and_then(|data| data.get(8..)) else { continue };
        let text = || String::from_utf8_lossy(data).into_owned();
        match kind {
            b"\xa9nam" => put(fields, "tag:title", &text()),
            b"\xa9ART" => put(fields, "tag:artist", &text()),
            b"\xa9alb" => put(fields, "tag:album", &text()),
            b"\xa9day" => put(fields, "tag:year", &text()),
            b"trkn" => {
                if let Some(track) = be16(data, 2).filter(|&track| track > 0) {
                    put(fields, "tag:track", &track.to_string());
                }
            }
            _ => {}
        }
    }
    tidy_tags(fields);
}

/// A track as a number of two digits or more, `3/12` as `03`, and a year as
/// its four digits, `2026-03-14` as `2026`.
fn tidy_tags(fields: &mut Fields) {
    if let Some(track) = fields.get_mut("tag:track") {
        let number: String = track.trim().chars().take_while(char::is_ascii_digit).collect();
        *track = match number.parse::<u32>() {
            Ok(number) => format!("{number:02}"),
            Err(_) => track.clone(),
        };
    }
    if let Some(year) = fields.get_mut("tag:year") {
        let digits: String = year.trim().chars().take_while(char::is_ascii_digit).collect();
        if digits.len() >= 4 {
            *year = digits[..4].to_string();
        }
    }
}

// Documents.

/// A PDF's title and author: from its Info dictionary, which is at the
/// end as often as the start, else from the XMP it may carry.
fn pdf(file: &mut File, head: &[u8], fields: &mut Fields) {
    let mut bytes = head.to_vec();
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    if length > HEAD {
        let mut tail = Vec::new();
        if file.seek(SeekFrom::Start(length.saturating_sub(HEAD).max(HEAD))).is_ok() && file.read_to_end(&mut tail).is_ok() {
            bytes.extend_from_slice(&tail);
        }
    }
    for (name, key) in [(&b"/Title"[..], "pdf:title"), (b"/Author", "pdf:author")] {
        // The last is the newest, an update being written after the rest.
        if let Some(value) = find_all(&bytes, name).filter_map(|at| pdf_string(&bytes[at + name.len()..])).filter(|value| !value.trim().is_empty()).last() {
            put(fields, key, &value);
        }
    }
    for (tag, key) in [(&b"<dc:title>"[..], "pdf:title"), (b"<dc:creator>", "pdf:author")] {
        if let Some(value) = find_all(&bytes, tag).next().and_then(|at| xmp_item(&bytes[at..])) {
            put(fields, key, &value);
        }
    }
}

/// Where each `needle` in `bytes` starts.
fn find_all<'a>(bytes: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    bytes.windows(needle.len()).enumerate().filter(move |(_, window)| *window == needle).map(|(at, _)| at)
}

/// The string a PDF name is given: `(literal)`, with its escapes, or
/// `<hex>`; in UTF-16 with a byte-order mark, or else taken as Latin-1.
fn pdf_string(after: &[u8]) -> Option<String> {
    let start = after.iter().position(|byte| !byte.is_ascii_whitespace())?;
    let after = &after[start..];
    let mut raw = Vec::new();
    match after.first()? {
        b'(' => {
            let (mut depth, mut at) = (1, 1);
            while let Some(&byte) = after.get(at) {
                at += 1;
                match byte {
                    b'\\' => {
                        let Some(&next) = after.get(at) else { break };
                        at += 1;
                        match next {
                            b'n' => raw.push(b'\n'),
                            b'r' => raw.push(b'\r'),
                            b't' => raw.push(b'\t'),
                            b'0'..=b'7' => {
                                let mut value = u32::from(next - b'0');
                                for _ in 0..2 {
                                    match after.get(at) {
                                        Some(&digit @ b'0'..=b'7') => {
                                            value = value * 8 + u32::from(digit - b'0');
                                            at += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                raw.push(value as u8);
                            }
                            b'\r' | b'\n' => {}
                            other => raw.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        raw.push(byte);
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        raw.push(byte);
                    }
                    _ => raw.push(byte),
                }
            }
        }
        b'<' => {
            let end = after.iter().position(|&byte| byte == b'>')?;
            let hex: Vec<u8> = after[1..end].iter().copied().filter(u8::is_ascii_hexdigit).collect();
            raw = hex
                .chunks(2)
                .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok().map(|value| if pair.len() == 1 { value << 4 } else { value }))
                .collect();
        }
        _ => return None,
    }
    Some(match raw.as_slice() {
        [0xFE, 0xFF, rest @ ..] => String::from_utf16_lossy(&rest.as_chunks::<2>().0.iter().map(|&pair| u16::from_be_bytes(pair)).collect::<Vec<_>>()),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        bytes => bytes.iter().map(|&byte| char::from(byte)).collect(),
    })
}

/// The first `<rdf:li>` of an XMP property, its entities undone.
fn xmp_item(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
    let item = text.find("<rdf:li")?;
    let open = item + text[item..].find('>')? + 1;
    let close = open + text[open..].find("</rdf:li>")?;
    Some(text[open..close].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}

#[cfg(test)]
mod tests;
