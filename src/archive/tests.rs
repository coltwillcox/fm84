use super::fixtures::{Put, scratch, sevenz, tar, zip};
use super::*;

fn names(index: &Index, inner: &str) -> Vec<String> {
    let mut names: Vec<String> = index.children(Path::new(inner)).map(|entry| entry.name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A zip that never names its directories still has them, and what each holds
/// is listed a directory at a time, sized without reading a byte.
#[test]
fn a_zip_lists_as_a_tree_with_its_implied_directories() {
    let dir = scratch("zip-tree");
    let path = dir.join("photos.zip");
    zip(&path, &[("2024/june/beach.jpg", Put::File(b"sand")), ("2024/june/sea.jpg", Put::File(b"waves!")), ("readme.txt", Put::File(b"hi")), ("empty/", Put::Dir)]);

    let index = super::index(&path).unwrap();
    assert_eq!(index.kind, Kind::Zip);
    assert_eq!(names(&index, ""), ["2024", "empty", "readme.txt"]);
    assert_eq!(names(&index, "2024/june"), ["beach.jpg", "sea.jpg"]);
    assert!(index.is_dir(Path::new("2024")) && index.is_dir(Path::new("")) && !index.is_dir(Path::new("readme.txt")));
    assert_eq!(index.size_of(Path::new("2024")), 10);
    assert_eq!(index.size_of(Path::new("")), 12);
    let implied = index.entry(index.find(Path::new("2024")).unwrap());
    assert_eq!(implied.position, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tar keeps owner, mode and time; a gzipped one reads the same; a later
/// entry of the same name replaces the earlier, as extracting it would.
#[test]
fn a_tar_gz_lists_with_what_tar_keeps() {
    let dir = scratch("tar-tree");
    let path = dir.join("src.tar.gz");
    tar(&path, &[(b"src/", Put::Dir), (b"src/main.rs", Put::File(b"old")), (b"src/link", Put::Link("main.rs")), (b"src/main.rs", Put::File(b"newer"))]);

    let index = super::index(&path).unwrap();
    assert_eq!(index.kind, Kind::TarGz);
    assert_eq!(names(&index, "src"), ["link", "main.rs"]);
    let main = index.entry(index.find(Path::new("src/main.rs")).unwrap());
    assert_eq!((main.size, main.position, main.mode.map(|mode| mode & 0o777)), (5, Some(3), Some(0o640)));
    assert_eq!(main.modified, Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)));
    let link = index.entry(index.find(Path::new("src/link")).unwrap());
    assert_eq!((link.kind, link.link.as_deref()), (EntryKind::Link, Some(Path::new("main.rs"))));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What would land outside the directory it is extracted to never enters the
/// tree, nor does anything that would be written through a link. A leading
/// slash is only dropped, as tar drops it.
#[test]
fn names_that_reach_outside_are_refused() {
    let dir = scratch("refused");
    let path = dir.join("evil.tar");
    tar(
        &path,
        &[
            (b"../escape", Put::File(b"x")),
            (b"a/../../escape", Put::File(b"x")),
            (b"/etc/motd", Put::File(b"rooted")),
            (b"out", Put::Link("/tmp")),
            (b"out/through", Put::File(b"x")),
            (b"fine.txt", Put::File(b"ok")),
        ],
    );
    let index = super::index(&path).unwrap();
    assert_eq!(names(&index, ""), ["etc", "fine.txt", "out"]);
    assert_eq!(names(&index, "etc"), ["motd"]);
    assert_eq!(index.refused, 3);

    let path = dir.join("evil.zip");
    zip(&path, &[("../escape", Put::File(b"x")), ("fine.txt", Put::File(b"ok"))]);
    let index = super::index(&path).unwrap();
    assert_eq!((names(&index, ""), index.refused), (vec!["fine.txt".to_string()], 1));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tar name need not be UTF-8, and comes through as the bytes it is.
#[cfg(unix)]
#[test]
fn a_tar_name_need_not_be_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let dir = scratch("latin1");
    let path = dir.join("old.tar");
    tar(&path, &[(b"caf\xe9.txt", Put::File(b"x"))]);
    let index = super::index(&path).unwrap();
    let entry = index.children(Path::new("")).next().unwrap();
    assert_eq!(entry.name().as_bytes(), b"caf\xe9.txt");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A path through an archive is found from the archive up; anything really
/// on the disk is not in one.
#[test]
fn a_path_through_an_archive_is_located() {
    let dir = scratch("locate");
    let path = dir.join("a.zip");
    zip(&path, &[("x/y.txt", Put::File(b"y"))]);

    let located = |path: &Path| locate(path).map(|found| (found.archive, found.inner));
    assert_eq!(located(&path.join("x").join("y.txt")), Some((path.clone(), PathBuf::from("x/y.txt"))));
    assert_eq!(located(&path.join("not/there")), Some((path.clone(), PathBuf::from("not/there"))));
    assert_eq!(located(&path), Some((path.clone(), PathBuf::new())));
    assert_eq!(located(&dir), None);
    assert_eq!(located(&dir.join("gone").join("deeper")), None);
    // A file that is not an archive has nothing inside it.
    std::fs::write(dir.join("plain.txt"), "").unwrap();
    assert_eq!(located(&dir.join("plain.txt").join("x")), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The index is read once, and again when the archive has been written.
#[test]
fn an_index_is_kept_until_its_archive_changes() {
    let dir = scratch("cache");
    let path = dir.join("a.tar");
    tar(&path, &[(b"one", Put::File(b"1"))]);
    let first = super::index(&path).unwrap();
    assert!(Arc::ptr_eq(&first, &super::index(&path).unwrap()));

    tar(&path, &[(b"one", Put::File(b"1")), (b"two", Put::File(b"22"))]);
    let second = super::index(&path).unwrap();
    assert_eq!(names(&second, ""), ["one", "two"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Entries are read out as asked: a zip in the order given, a tar in its
/// own, and Again reads one from its start once more.
#[test]
fn entries_are_read_and_read_again() {
    let dir = scratch("read");
    for name in ["a.zip", "a.tar.gz"] {
        let path = dir.join(name);
        if name.ends_with("zip") {
            zip(&path, &[("first", Put::File(b"1st")), ("second", Put::File(b"2nd"))]);
        } else {
            tar(&path, &[(b"first", Put::File(b"1st")), (b"second", Put::File(b"2nd"))]);
        }
        let index = super::index(&path).unwrap();
        let positions: Vec<usize> = ["second", "first"].iter().map(|inner| index.entry(index.find(Path::new(inner)).unwrap()).position.unwrap()).collect();

        let mut read = Vec::new();
        let mut asked_again = false;
        read_entries(&path, index.kind, &positions, &mut |_, contents| {
            let mut text = String::new();
            contents.unwrap().read_to_string(&mut text).unwrap();
            read.push(text.clone());
            if text == "1st" && !asked_again {
                asked_again = true;
                return Next::Again;
            }
            Next::Go
        })
        .unwrap();
        read.sort();
        assert_eq!(read, ["1st", "1st", "2nd"], "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What is not an archive, though named like one, says so.
#[test]
fn a_file_named_like_an_archive_that_is_not_one_fails_to_read() {
    let dir = scratch("fake");
    for name in ["fake.zip", "fake.tar.gz"] {
        std::fs::write(dir.join(name), "plain text, not an archive").unwrap();
        assert!(super::index(&dir.join(name)).is_err(), "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// One archive made in every format fm84 reads: a 7z both solid, every file
/// in one block, and not.
fn every_format(dir: &Path) -> Vec<PathBuf> {
    let mut made = Vec::new();
    for name in ["a.zip", "a.tar", "a.tar.gz", "a.tar.zst", "a.tar.xz", "a.tar.bz2"] {
        let path = dir.join(name);
        if name.ends_with("zip") {
            zip(&path, &[("docs/notes.txt", Put::File(b"notes")), ("docs/deep/more.txt", Put::File(b"more!")), ("top.txt", Put::File(b"top"))]);
        } else {
            tar(&path, &[(b"docs/notes.txt", Put::File(b"notes")), (b"docs/deep/more.txt", Put::File(b"more!")), (b"top.txt", Put::File(b"top"))]);
        }
        made.push(path);
    }
    for (name, solid) in [("solid.7z", true), ("loose.7z", false)] {
        let path = dir.join(name);
        sevenz(&path, &[("docs/notes.txt", Put::File(b"notes")), ("docs/deep/more.txt", Put::File(b"more!")), ("top.txt", Put::File(b"top"))], solid);
        made.push(path);
    }
    made
}

/// Every format lists the same tree and reads the same contents - asked for
/// last first, which in a solid 7z means reading through what comes before.
#[test]
fn every_format_lists_and_reads_alike() {
    let dir = scratch("formats");
    for path in every_format(&dir) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let index = super::index(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(names(&index, ""), ["docs", "top.txt"], "{name}");
        assert_eq!(names(&index, "docs"), ["deep", "notes.txt"], "{name}");
        assert_eq!(index.size_of(Path::new("")), 13, "{name}");

        let wanted = ["top.txt", "docs/deep/more.txt", "docs/notes.txt"];
        let positions: Vec<usize> = wanted.iter().map(|inner| index.entry(index.find(Path::new(inner)).unwrap()).position.unwrap()).collect();
        let mut read = Vec::new();
        read_entries(&path, index.kind, &positions, &mut |position, contents| {
            let mut text = String::new();
            contents.unwrap().read_to_string(&mut text).unwrap();
            read.push((position, text));
            Next::Go
        })
        .unwrap();
        read.sort();
        let mut expected: Vec<(usize, String)> = positions.iter().zip(["top", "more!", "notes"]).map(|(&position, text)| (position, text.to_string())).collect();
        expected.sort();
        assert_eq!(read, expected, "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A wanted entry that is only partly read, or not at all, does not throw the
/// next one in its block off: what was left of it is read past.
#[test]
fn a_solid_7z_entry_left_half_read_does_not_spoil_the_next() {
    let dir = scratch("7z-drain");
    let path = dir.join("a.7z");
    sevenz(&path, &[("first", Put::File(b"0123456789")), ("second", Put::File(b"second"))], true);
    let index = super::index(&path).unwrap();
    let positions: Vec<usize> = ["first", "second"].iter().map(|inner| index.entry(index.find(Path::new(inner)).unwrap()).position.unwrap()).collect();
    let mut second = String::new();
    read_entries(&path, index.kind, &positions, &mut |position, contents| {
        let contents = contents.unwrap();
        if position == positions[0] {
            let mut half = [0; 3];
            contents.read_exact(&mut half).unwrap();
        } else {
            contents.read_to_string(&mut second).unwrap();
        }
        Next::Go
    })
    .unwrap();
    assert_eq!(second, "second");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// zstd written a frame at a time - by pzstd, or two files joined - is one
/// archive, read on past the end of the first frame.
#[test]
fn a_tar_zst_of_several_frames_reads_whole() {
    let dir = scratch("zst-frames");
    let plain = dir.join("a.tar");
    tar(&plain, &[(b"one", Put::File(b"first")), (b"two", Put::File(b"second"))]);
    let bytes = std::fs::read(&plain).unwrap();
    let (front, back) = bytes.split_at(700);
    let mut joined = ruzstd::encoding::compress_to_vec(front, ruzstd::encoding::CompressionLevel::Fastest);
    joined.extend(ruzstd::encoding::compress_to_vec(back, ruzstd::encoding::CompressionLevel::Fastest));
    let path = dir.join("a.tar.zst");
    std::fs::write(&path, joined).unwrap();
    assert_eq!(names(&super::index(&path).unwrap(), ""), ["one", "two"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A 7z made on Windows names its directories with backslashes; one that
/// would reach outside is refused as in any other format; a link written by
/// Unix is a link.
#[test]
fn a_7z_reads_either_separator_and_refuses_what_reaches_out() {
    let dir = scratch("7z-names");
    let path = dir.join("a.7z");
    sevenz(&path, &[("win\\dir\\file.txt", Put::File(b"w")), ("..\\escape", Put::File(b"x")), ("link", Put::Link("win/dir/file.txt"))], false);
    let index = super::index(&path).unwrap();
    assert_eq!(names(&index, ""), ["link", "win"]);
    assert_eq!(names(&index, "win/dir"), ["file.txt"]);
    assert_eq!(index.refused, 1);
    assert_eq!(index.entry(index.find(Path::new("link")).unwrap()).kind, EntryKind::Link);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Archives 7-Zip itself made: `7z a -mx=9` of a directory and a file, both
/// dated 2024-01-02 03:04:05; and one file encrypted with `-p`, its names
/// with `-mhe=on` too.
fn seven_zip_made(dir: &Path, name: &str) -> PathBuf {
    let bytes: &[u8] = match name {
        "plain.7z" => include_bytes!("testdata/plain.7z"),
        "secret.7z" => include_bytes!("testdata/secret.7z"),
        _ => include_bytes!("testdata/hidden.7z"),
    };
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// What 7-Zip writes - solid, LZMA2, a mode in the top of the attributes -
/// lists and reads as written.
#[test]
fn a_7z_from_7_zip_reads_as_it_was_made() {
    use chrono::TimeZone;
    let dir = scratch("7-zip");
    let path = seven_zip_made(&dir, "plain.7z");
    let index = super::index(&path).unwrap();
    assert_eq!(names(&index, ""), ["b.txt", "dir"]);
    assert_eq!(names(&index, "dir"), ["a.txt"]);
    let b = index.entry(index.find(Path::new("b.txt")).unwrap());
    assert_eq!(b.mode.map(|mode| mode & 0o777), Some(0o644));
    let made = chrono::Local.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    assert_eq!(b.modified, Some(SystemTime::from(made)));

    let positions: Vec<usize> = ["dir/a.txt", "b.txt"].iter().map(|inner| index.entry(index.find(Path::new(inner)).unwrap()).position.unwrap()).collect();
    let mut read = Vec::new();
    read_entries(&path, index.kind, &positions, &mut |_, contents| {
        let mut text = String::new();
        contents.unwrap().read_to_string(&mut text).unwrap();
        read.push(text);
        Next::Go
    })
    .unwrap();
    read.sort();
    assert_eq!(read, ["alpha", "bravo"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An encrypted 7z says so: its files when read, if its names can be listed,
/// and the archive itself when they are encrypted too - not that it is packed
/// by a method fm84 does not know.
#[test]
fn an_encrypted_7z_says_it_needs_the_password() {
    let dir = scratch("7z-secret");
    let path = seven_zip_made(&dir, "secret.7z");
    let index = super::index(&path).unwrap();
    assert_eq!(names(&index, ""), ["b.txt"]);
    let position = index.entry(index.find(Path::new("b.txt")).unwrap()).position.unwrap();
    let mut said = None;
    read_entries(&path, index.kind, &[position], &mut |_, contents| {
        said = Some(contents.err().map(|e| e.to_string()));
        Next::Go
    })
    .unwrap();
    let said = said.flatten().unwrap_or_default();
    assert!(said.starts_with("Encrypted"), "{said}");

    let hidden = seven_zip_made(&dir, "hidden.7z");
    let said = super::index(&hidden).unwrap_err().to_string();
    assert!(said.starts_with("Encrypted"), "{said}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tar's times are what its headers say, and a header can say anything:
/// one past what the clock can hold is no time, not a crash.
#[test]
fn a_tar_time_past_the_clock_is_no_time() {
    let dir = scratch("tar-time");
    let path = dir.join("far.tar");
    let mut builder = tar::Builder::new(Vec::new());
    for (name, mtime) in [("far.txt", u64::MAX), ("near.txt", 1_700_000_000)] {
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_mtime(mtime);
        header.set_cksum();
        builder.append_data(&mut header, name, &b"x"[..]).unwrap();
    }
    std::fs::write(&path, builder.into_inner().unwrap()).unwrap();

    let index = crate::archive::index(&path).unwrap();
    let modified = |name: &str| index.children(Path::new("")).find(|entry| entry.path == Path::new(name)).unwrap().modified;
    assert_eq!(modified("far.txt"), None);
    assert_eq!(modified("near.txt"), Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A tar is never encrypted, and asking whether one needs a password does
/// not read it - which for a large one would hold the UI still.
#[test]
fn a_tar_is_never_asked_for_a_password() {
    let dir = scratch("never-locked");
    let archive = dir.join("big.tar.gz");
    tar(&archive, &[(b"a.txt", Put::File(b"a"))]);
    assert!(!super::needs_password(&archive, &[PathBuf::new()]));
    assert!(!super::is_encrypted(&archive));
    let cached = super::CACHE.lock().unwrap().iter().any(|((path, ..), _)| *path == archive);
    assert!(!cached);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An archive written over by a rename - as fm84 writes one - is read
/// again, even the same size and in the same tick of the clock as the one
/// before it, which is all the modified time can tell apart.
#[test]
fn an_archive_written_over_is_read_again_however_soon() {
    let dir = scratch("written-over");
    let archive = dir.join("box.tar");
    tar(&archive, &[(b"a.txt", Put::File(b"a"))]);
    let before = index(&archive).unwrap();
    assert!(before.find(Path::new("a.txt")).is_some());
    let next = dir.join("next.tar");
    tar(&next, &[(b"b.txt", Put::File(b"b"))]);
    assert_eq!(std::fs::metadata(&next).unwrap().len(), std::fs::metadata(&archive).unwrap().len());
    let when = std::fs::metadata(&archive).unwrap().modified().unwrap();
    std::fs::File::options().write(true).open(&next).unwrap().set_modified(when).unwrap();
    std::fs::rename(&next, &archive).unwrap();
    let after = index(&archive).unwrap();
    assert!(after.find(Path::new("b.txt")).is_some(), "the archive as it was before");

    // And when told, whatever the disk says.
    forget_index(&archive);
    assert!(!CACHE.lock().unwrap().iter().any(|((path, ..), _)| *path == archive));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A password given for an archive not made yet - as Pack gives it, before
/// the job writes the file - is found by the archive once it is there,
/// whichever way it is named, through a link to its directory or not.
#[cfg(unix)]
#[test]
fn a_password_given_before_the_archive_is_made_is_found_after() {
    let dir = scratch("password-ahead");
    let real = dir.join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, dir.join("link")).unwrap();
    set_password(&dir.join("link/new.7z"), "sesame");
    std::fs::write(real.join("new.7z"), "made").unwrap();
    assert_eq!(password(&real.join("new.7z")).as_deref(), Some("sesame"));
    assert_eq!(password(&dir.join("link/new.7z")).as_deref(), Some("sesame"));
    forget_password(&real.join("new.7z"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A path into an archive typed with a separator on the end - `box.zip/`,
/// as a directory is typed, and as F5 opens on one - is still into it.
#[test]
fn a_path_ending_in_a_separator_is_into_the_archive() {
    let dir = scratch("trailing-separator");
    let archive = dir.join("box.zip");
    zip(&archive, &[("sub/a.txt", Put::File(b"a"))]);
    let typed = |path: &Path| PathBuf::from(format!("{}{}", path.display(), std::path::MAIN_SEPARATOR));
    assert_eq!(locate(&typed(&archive)), Some(Located { archive: archive.clone(), inner: PathBuf::new() }));
    assert_eq!(
        locate(&typed(&archive.join("sub"))),
        Some(Located {
            archive: archive.clone(),
            inner: PathBuf::from("sub")
        })
    );
    assert_eq!(locate(&typed(&dir)), None);
    std::fs::remove_dir_all(&dir).unwrap();
}
