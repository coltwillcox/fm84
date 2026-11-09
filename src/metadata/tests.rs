//! Each format, made here by hand as small as it can be, read back.

use super::*;

/// The file `bytes` make, read for its metadata.
fn read_bytes(name: &str, bytes: &[u8]) -> Fields {
    let dir = std::env::temp_dir().join(format!("fm84-metadata-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let fields = read(&path);
    std::fs::remove_file(&path).unwrap();
    fields
}

fn field<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields.get(key).map(String::as_str)
}

/// EXIF as a little-endian TIFF: the make out of line, the model inline,
/// and the time taken in an Exif IFD of its own.
fn exif() -> Vec<u8> {
    let mut tiff = b"II*\0".to_vec();
    tiff.extend(8u32.to_le_bytes());
    // IFD0 at 8: three entries, then no next IFD. Its data after it.
    let ifd0_end = 8 + 2 + 3 * 12 + 4;
    let make_at = ifd0_end as u32;
    let exif_ifd = make_at + 6;
    let date_at = exif_ifd + 2 + 12 + 4;
    let entry = |tag: u16, kind: u16, count: u32, value: [u8; 4]| [tag.to_le_bytes().as_slice(), &kind.to_le_bytes(), &count.to_le_bytes(), &value].concat();
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(entry(0x010F, 2, 6, make_at.to_le_bytes()));
    tiff.extend(entry(0x0110, 2, 3, *b"R5\0\0"));
    tiff.extend(entry(0x8769, 4, 1, exif_ifd.to_le_bytes()));
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(b"Canon\0");
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(entry(0x9003, 2, 20, date_at.to_le_bytes()));
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(b"2026:03:14 18:02:11\0");
    tiff
}

#[test]
fn a_photographs_exif_is_read_from_jpeg_png_and_webp() {
    let tiff = exif();
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(&tiff);
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xE1];
    jpeg.extend(((app1.len() + 2) as u16).to_be_bytes());
    jpeg.extend(&app1);
    jpeg.extend([0xFF, 0xDA, 0x00, 0x02]);
    let fields = read_bytes("photo.jpg", &jpeg);
    assert_eq!(field(&fields, "exif:date"), Some("2026-03-14 18.02.11"));
    assert_eq!(field(&fields, "exif:day"), Some("2026-03-14"));
    assert_eq!(field(&fields, "exif:time"), Some("18.02.11"));
    assert_eq!(field(&fields, "exif:make"), Some("Canon"));
    assert_eq!(field(&fields, "exif:model"), Some("R5"));

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend((tiff.len() as u32).to_be_bytes());
    png.extend(b"eXIf");
    png.extend(&tiff);
    png.extend([0; 4]);
    assert_eq!(field(&read_bytes("photo.png", &png), "exif:day"), Some("2026-03-14"));

    let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
    webp.extend(b"EXIF");
    webp.extend((tiff.len() as u32).to_le_bytes());
    webp.extend(&tiff);
    assert_eq!(field(&read_bytes("photo.webp", &webp), "exif:model"), Some("R5"));

    // A camera with its clock never set writes zeros, which is no date.
    assert_eq!(exif_date("0000:00:00 00:00:00"), None);
}

#[test]
fn an_mp3s_id3_tags_are_read() {
    let frame = |id: &[u8], body: &[u8]| [id, &(body.len() as u32).to_be_bytes(), &[0, 0], body].concat();
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("AC/DC".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let frames = [frame(b"TIT2", b"\x03Back in Black"), frame(b"TPE1", &[[1].as_slice(), &utf16].concat()), frame(b"TRCK", b"\x003/12"), frame(b"TYER", b"\x001980")].concat();
    let size = frames.len();
    let syncsafe = [(size >> 21) as u8 & 0x7F, (size >> 14) as u8 & 0x7F, (size >> 7) as u8 & 0x7F, size as u8 & 0x7F];
    let mut mp3 = [b"ID3\x03\x00\x00".as_slice(), &syncsafe, &frames].concat();
    mp3.extend([0xFF, 0xFB, 0x90, 0x00]);
    let fields = read_bytes("song.mp3", &mp3);
    assert_eq!(field(&fields, "tag:title"), Some("Back in Black"));
    assert_eq!(field(&fields, "tag:artist"), Some("AC/DC"));
    assert_eq!(field(&fields, "tag:track"), Some("03"));
    assert_eq!(field(&fields, "tag:year"), Some("1980"));

    // An ID3v1 tag alone, at the end.
    let mut old = vec![0xFF, 0xFB, 0x90, 0x00];
    let mut tag = [0u8; 128];
    tag[..3].copy_from_slice(b"TAG");
    tag[3..8].copy_from_slice(b"Title");
    tag[33..39].copy_from_slice(b"Artist");
    tag[126] = 7;
    old.extend(tag);
    let fields = read_bytes("old.mp3", &old);
    assert_eq!((field(&fields, "tag:artist"), field(&fields, "tag:track")), (Some("Artist"), Some("07")));
}

/// Vorbis comments as both FLAC and Ogg carry them.
fn comments() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(4u32.to_le_bytes());
    out.extend(b"test");
    let lines = ["ARTIST=Daft Punk", "title=Around the World", "TRACKNUMBER=7", "DATE=1997-01-13"];
    out.extend((lines.len() as u32).to_le_bytes());
    for line in lines {
        out.extend((line.len() as u32).to_le_bytes());
        out.extend(line.as_bytes());
    }
    out
}

#[test]
fn flac_and_ogg_vorbis_comments_are_read() {
    let block = comments();
    let mut flac = b"fLaC".to_vec();
    flac.extend([0x00, 0, 0, 2, 0xAA, 0xBB]);
    flac.extend([0x84, (block.len() >> 16) as u8, (block.len() >> 8) as u8, block.len() as u8]);
    flac.extend(&block);
    let fields = read_bytes("song.flac", &flac);
    assert_eq!(field(&fields, "tag:artist"), Some("Daft Punk"));
    assert_eq!(field(&fields, "tag:title"), Some("Around the World"));
    assert_eq!((field(&fields, "tag:track"), field(&fields, "tag:year")), (Some("07"), Some("1997")));

    // Two pages: the identification packet, then the comments spread over
    // segments of 255 and the rest.
    let page = |packet: &[u8]| {
        let mut lacing: Vec<u8> = vec![255; packet.len() / 255];
        lacing.push((packet.len() % 255) as u8);
        [b"OggS\0\0".as_slice(), &[0; 20], &[lacing.len() as u8], &lacing, packet].concat()
    };
    let mut packet = b"\x03vorbis".to_vec();
    packet.extend(&block);
    packet.extend(vec![b'x'; 300]);
    let ogg = [page(b"\x01vorbis-ident"), page(&packet)].concat();
    assert_eq!(field(&read_bytes("song.ogg", &ogg), "tag:artist"), Some("Daft Punk"));
}

#[test]
fn an_m4as_tags_are_read_with_moov_at_the_end() {
    let atom = |kind: &[u8], body: &[u8]| [&((body.len() + 8) as u32).to_be_bytes(), kind, body].concat();
    let data = |payload: &[u8]| atom(b"data", &[&[0, 0, 0, 1, 0, 0, 0, 0], payload].concat());
    let list = atom(b"ilst", &[atom(b"\xa9ART", &data(b"Moby")), atom(b"trkn", &data(&[0, 0, 0, 4, 0, 12, 0, 0]))].concat());
    let meta = atom(b"meta", &[&[0, 0, 0, 0], list.as_slice()].concat());
    let moov = atom(b"moov", &atom(b"udta", &meta));
    let m4a = [atom(b"ftyp", b"M4A \0\0\0\0"), atom(b"mdat", &[0; 64]), moov].concat();
    let fields = read_bytes("song.m4a", &m4a);
    assert_eq!((field(&fields, "tag:artist"), field(&fields, "tag:track")), (Some("Moby"), Some("04")));
}

#[test]
fn a_pdfs_title_and_author_are_read() {
    let pdf = b"%PDF-1.4\n1 0 obj\n<< /Title (A \\(nested\\) title\\041) /Author <FEFF00C9006D0069006C0065> >>\nendobj\n%%EOF\n";
    let fields = read_bytes("paper.pdf", pdf);
    assert_eq!(field(&fields, "pdf:title"), Some("A (nested) title!"));
    assert_eq!(field(&fields, "pdf:author"), Some("Émile"));

    // Only in the XMP.
    let pdf = b"%PDF-1.7\n<x:xmpmeta><dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">Q&amp;A</rdf:li></rdf:Alt></dc:title></x:xmpmeta>";
    assert_eq!(field(&read_bytes("xmp.pdf", pdf), "pdf:title"), Some("Q&A"));
}
