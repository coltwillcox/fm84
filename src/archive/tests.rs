use super::fixtures::{Put, scratch, tar, zip};
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
