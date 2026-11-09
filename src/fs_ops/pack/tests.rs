use super::*;
use crate::archive::fixtures::scratch;

/// Answers every failure the same way, remembers what it was asked about,
/// and can stop after so many files.
struct Asking {
    reply: Reply,
    failures: Vec<PathBuf>,
    files_left: Option<usize>,
    /// How a name taken twice is answered.
    clash: Clash,
}

impl Asking {
    fn new(reply: Reply) -> Self {
        Asking {
            reply,
            failures: Vec::new(),
            files_left: None,
            clash: Clash::Replace,
        }
    }
}

impl Progress for Asking {
    fn step(&mut self, step: Step<'_>) -> bool {
        if let (Step::Starting(_), Some(left)) = (step, &mut self.files_left) {
            if *left == 0 {
                return false;
            }
            *left -= 1;
        }
        true
    }

    fn failed(&mut self, path: &Path, _error: &Error) -> Reply {
        self.failures.push(path.to_path_buf());
        self.reply
    }

    fn exists(&mut self, _source: &Path, _dest: &Path) -> Clash {
        self.clash
    }
}

/// Plan and pack, as the job does.
fn pack_into(sources: &[(PathBuf, bool)], dest: &Path, asking: &mut Asking) -> Result<Transfer, Error> {
    let format = PackFormat::of(dest).expect("named as an archive");
    let Some(plan) = plan_pack(sources, dest, format, None, asking)? else {
        return Ok(Transfer::Cancelled);
    };
    pack(&plan, dest, format, PackLevel::Normal, None, false, asking)
}

/// Everything in an archive, by fm84's own reading of it: each path with
/// what it holds - a file's contents, a link's target, or "/" for a directory.
fn contents(archive: &Path) -> Vec<(String, String)> {
    use crate::archive::{EntryKind, Next};
    let index = crate::archive::index(archive).unwrap();
    let mut found = Vec::new();
    let mut reading = Vec::new();
    for at in index.subtree(Path::new("")) {
        let entry = index.entry(at);
        let path = entry.path.to_string_lossy().replace('\\', "/");
        match (entry.kind, &entry.link) {
            (EntryKind::Dir, _) => found.push((path, "/".to_string())),
            (EntryKind::Link, Some(target)) => found.push((path, format!("-> {}", target.display()))),
            _ => reading.push((entry.position.unwrap(), path, entry.kind)),
        }
    }
    let positions: Vec<usize> = reading.iter().map(|(position, _, _)| *position).collect();
    crate::archive::read_entries(archive, index.kind, &positions, &mut |position, read| {
        let (_, path, kind) = reading.iter().find(|(at, _, _)| *at == position).unwrap();
        let mut text = String::new();
        read.unwrap().read_to_string(&mut text).unwrap();
        found.push((path.clone(), if *kind == EntryKind::Link { format!("-> {text}") } else { text }));
        Next::Go
    })
    .unwrap();
    found.sort();
    found
}

/// A tree to pack: a directory with a file, an empty file, a directory
/// inside it, and a link; and a file beside it.
fn tree(dir: &Path) -> Vec<(PathBuf, bool)> {
    let docs = dir.join("docs");
    fs::create_dir_all(docs.join("deep")).unwrap();
    fs::write(docs.join("notes.txt"), "notes").unwrap();
    fs::write(docs.join("empty.txt"), "").unwrap();
    fs::write(docs.join("deep/more.txt"), "more!").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("notes.txt", docs.join("link")).unwrap();
    fs::write(dir.join("top.txt"), "top").unwrap();
    vec![(docs, true), (dir.join("top.txt"), false)]
}

/// Every format packs the same tree, read back the same - links as links -
/// and leaves no temporary behind.
#[test]
fn every_format_packs_what_it_is_given() {
    let dir = scratch("pack-formats");
    let sources = tree(&dir.join("in"));
    for format in PackFormat::ALL {
        let dest = dir.join(format!("out{}", format.extension()));
        let done = pack_into(&sources, &dest, &mut Asking::new(Reply::Abort)).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!(done, Transfer::Done, "{format:?}");
        let mut expected = vec![("docs", "/"), ("docs/deep", "/"), ("docs/deep/more.txt", "more!"), ("docs/empty.txt", ""), ("docs/notes.txt", "notes"), ("top.txt", "top")];
        if cfg!(unix) {
            expected.push(("docs/link", "-> notes.txt"));
        }
        let mut expected: Vec<(String, String)> = expected.into_iter().map(|(path, held)| (path.to_string(), held.to_string())).collect();
        expected.sort();
        assert_eq!(contents(&dest), expected, "{format:?}");
    }
    let left: Vec<String> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| name.contains(".fm84-")).collect();
    assert!(left.is_empty(), "{left:?}");
    fs::remove_dir_all(&dir).unwrap();
}

/// A tar keeps a file's mode and time as they were.
#[cfg(unix)]
#[test]
fn a_tar_keeps_modes_and_times() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("pack-modes");
    let script = dir.join("run.sh");
    fs::write(&script, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o750)).unwrap();
    let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    File::options().write(true).open(&script).unwrap().set_modified(when).unwrap();

    for name in ["out.tar.gz", "out.zip", "out.7z"] {
        let dest = dir.join(name);
        pack_into(&[(script.clone(), false)], &dest, &mut Asking::new(Reply::Abort)).unwrap();
        let index = crate::archive::index(&dest).unwrap();
        let entry = index.entry(index.find(Path::new("run.sh")).unwrap());
        assert_eq!(entry.mode.map(|mode| mode & 0o777), Some(0o750), "{name}");
        // A zip holds local time to the even second; this one is even.
        assert_eq!(entry.modified, Some(when), "{name}");
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// An archive made inside the directory being packed is not packed into
/// itself, nor is one already at its name that is being replaced.
#[test]
fn an_archive_inside_what_is_packed_leaves_itself_out() {
    let dir = scratch("pack-inside");
    fs::write(dir.join("a.txt"), "a").unwrap();
    fs::write(dir.join("self.zip"), "an old one").unwrap();
    let dest = dir.join("self.zip");
    let format = PackFormat::Zip;
    let mut asking = Asking::new(Reply::Abort);
    let plan = plan_pack(&[(dir.clone(), true)], &dest, format, None, &mut asking).unwrap().unwrap();
    assert_eq!(pack(&plan, &dest, format, PackLevel::Normal, None, true, &mut asking).unwrap(), Transfer::Done);
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(contents(&dest), [(name.clone(), "/".to_string()), (format!("{name}/a.txt"), "a".to_string())]);
    fs::remove_dir_all(&dir).unwrap();
}

/// Cancelled partway, nothing is left: no archive, no temporary.
#[test]
fn a_cancel_leaves_nothing() {
    let dir = scratch("pack-cancel");
    let sources = tree(&dir.join("in"));
    for format in PackFormat::ALL {
        let dest = dir.join(format!("out{}", format.extension()));
        let mut asking = Asking::new(Reply::Abort);
        asking.files_left = Some(1);
        assert_eq!(pack_into(&sources, &dest, &mut asking).unwrap(), Transfer::Cancelled, "{format:?}");
        assert!(!dest.exists(), "{format:?}");
    }
    let left: Vec<_> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(left, ["in"]);
    fs::remove_dir_all(&dir).unwrap();
}

/// A file that cannot be read is asked about; skipped, the rest goes in.
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_asked_about_and_can_be_left_out() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("pack-unreadable");
    fs::write(dir.join("fine.txt"), "fine").unwrap();
    fs::write(dir.join("locked.txt"), "locked").unwrap();
    fs::set_permissions(dir.join("locked.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads it anyway, and there is nothing to ask.
    if File::open(dir.join("locked.txt")).is_ok() {
        return fs::remove_dir_all(&dir).unwrap();
    }
    let sources = [(dir.join("fine.txt"), false), (dir.join("locked.txt"), false)];
    for name in ["out.tar.gz", "out.zip", "out.7z"] {
        let dest = dir.join(name);
        let mut asking = Asking::new(Reply::Skip);
        assert_eq!(pack_into(&sources, &dest, &mut asking).unwrap(), Transfer::Done, "{name}");
        assert_eq!(asking.failures, [dir.join("locked.txt")], "{name}");
        assert_eq!(contents(&dest), [("fine.txt".to_string(), "fine".to_string())], "{name}");
    }
    fs::set_permissions(dir.join("locked.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

/// A name that is not UTF-8 goes into a tar as it is; a zip or a 7z cannot
/// hold it, and says so.
#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_goes_only_into_a_tar() {
    use std::os::unix::ffi::OsStrExt;
    let dir = scratch("pack-latin1");
    let odd = dir.join(std::ffi::OsStr::from_bytes(b"caf\xe9.txt"));
    fs::write(&odd, "x").unwrap();
    let mut asking = Asking::new(Reply::Skip);
    pack_into(&[(odd.clone(), false)], &dir.join("out.tar"), &mut asking).unwrap();
    assert!(asking.failures.is_empty());
    let index = crate::archive::index(&dir.join("out.tar")).unwrap();
    assert_eq!(index.children(Path::new("")).next().unwrap().name().as_bytes(), b"caf\xe9.txt");
    for name in ["out.zip", "out.7z"] {
        let mut asking = Asking::new(Reply::Skip);
        pack_into(&[(odd.clone(), false)], &dir.join(name), &mut asking).unwrap();
        assert_eq!(asking.failures, std::slice::from_ref(&odd), "{name}");
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Something already at the name is only written over when that was said
/// to be all right.
#[test]
fn an_archive_already_there_is_kept_unless_replacing() {
    let dir = scratch("pack-taken");
    fs::write(dir.join("a.txt"), "a").unwrap();
    let dest = dir.join("out.zip");
    fs::write(&dest, "somebody's").unwrap();
    let err = pack_into(&[(dir.join("a.txt"), false)], &dest, &mut Asking::new(Reply::Abort)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "somebody's");
    fs::remove_dir_all(&dir).unwrap();
}

/// Every level of every format reads back the same, and makes a different
/// archive from the others; a plain tar has no level, and is the same
/// whichever is asked for.
#[test]
fn every_level_packs_what_it_is_given() {
    let dir = scratch("pack-levels");
    let text: String = (0..20_000).map(|n| format!("line {} of {}\n", n * 7919 % 1000, n % 13)).collect();
    fs::write(dir.join("words.txt"), &text).unwrap();
    let sources = [(dir.join("words.txt"), false)];
    for format in PackFormat::ALL {
        let mut sizes = Vec::new();
        for level in PackLevel::ALL {
            let dest = dir.join(format!("{level:?}{}", format.extension()));
            let mut asking = Asking::new(Reply::Abort);
            let plan = plan_pack(&sources, &dest, format, None, &mut asking).unwrap().unwrap();
            assert_eq!(pack(&plan, &dest, format, level, None, false, &mut asking).unwrap(), Transfer::Done, "{format:?} {level:?}");
            assert_eq!(contents(&dest), [("words.txt".to_string(), text.clone())], "{format:?} {level:?}");
            sizes.push(fs::metadata(&dest).unwrap().len());
        }
        // Which is smaller depends on what is packed - LZMA's fast mode can
        // win on text this regular - but the level has to have reached it.
        if format.has_levels() {
            assert_ne!(sizes[0], sizes[2], "{format:?}: {sizes:?}");
        } else {
            assert!(sizes.iter().all(|size| *size == sizes[0]), "{format:?}: {sizes:?}");
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// The levels go round either way.
#[test]
fn a_level_steps_round() {
    assert_eq!(PackLevel::Normal.step(true), PackLevel::Best);
    assert_eq!(PackLevel::Best.step(true), PackLevel::Fast);
    assert_eq!(PackLevel::Fast.step(false), PackLevel::Best);
    assert_eq!(PackLevel::default(), PackLevel::Normal);
}

/// Moved into the archive: what went in is gone from the disk, directories
/// and links with it; what turned up meanwhile, and what changed since the
/// walk, stay, and so does the directory that still holds them.
#[test]
fn what_went_in_is_removed_and_nothing_else() {
    let dir = scratch("pack-move");
    let sources = tree(&dir.join("in"));
    let dest = dir.join("out.tar.gz");
    let format = PackFormat::TarGz;
    let mut asking = Asking::new(Reply::Abort);
    let plan = plan_pack(&sources, &dest, format, None, &mut asking).unwrap().unwrap();
    assert_eq!(pack(&plan, &dest, format, PackLevel::Normal, None, false, &mut asking).unwrap(), Transfer::Done);
    let docs = dir.join("in/docs");
    // After the walk: one file new, one grown.
    fs::write(docs.join("deep/new.txt"), "new").unwrap();
    fs::write(docs.join("notes.txt"), "notes, and more").unwrap();
    assert_eq!(remove_packed(&plan, &mut asking).unwrap(), Transfer::Done);

    assert!(!dir.join("in/top.txt").exists());
    assert!(!docs.join("empty.txt").exists());
    assert!(!docs.join("deep/more.txt").exists());
    assert!(docs.join("link").symlink_metadata().is_err());
    assert_eq!(fs::read_to_string(docs.join("deep/new.txt")).unwrap(), "new");
    assert_eq!(fs::read_to_string(docs.join("notes.txt")).unwrap(), "notes, and more");
    // And everything still in the archive.
    assert!(contents(&dest).iter().any(|(path, held)| path == "docs/deep/more.txt" && held == "more!"));
    fs::remove_dir_all(&dir).unwrap();
}

/// A whole tree packed and moved leaves nothing of it.
#[test]
fn a_tree_moved_in_whole_is_gone() {
    let dir = scratch("pack-move-whole");
    let sources = tree(&dir.join("in"));
    for format in PackFormat::ALL {
        let sources: Vec<(PathBuf, bool)> = sources.clone();
        let dest = dir.join(format!("out{}", format.extension()));
        let mut asking = Asking::new(Reply::Abort);
        let plan = plan_pack(&sources, &dest, format, None, &mut asking).unwrap().unwrap();
        assert_eq!(pack(&plan, &dest, format, PackLevel::Fast, None, false, &mut asking).unwrap(), Transfer::Done);
        assert_eq!(remove_packed(&plan, &mut asking).unwrap(), Transfer::Done);
        assert!(!dir.join("in/docs").exists() && !dir.join("in/top.txt").exists(), "{format:?}");
        // Put back from what was packed, for the next format.
        tree(&dir.join("in"));
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A file left out when it could not be read stays where it is, and so
/// does the directory it is in.
#[cfg(unix)]
#[test]
fn what_was_left_out_is_not_removed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("pack-move-skipped");
    let box_dir = dir.join("box");
    fs::create_dir_all(&box_dir).unwrap();
    fs::write(box_dir.join("fine.txt"), "fine").unwrap();
    fs::write(box_dir.join("locked.txt"), "locked").unwrap();
    fs::set_permissions(box_dir.join("locked.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    if File::open(box_dir.join("locked.txt")).is_ok() {
        return fs::remove_dir_all(&dir).unwrap();
    }
    for name in ["out.zip", "out.tar.xz", "out.7z"] {
        fs::write(box_dir.join("fine.txt"), "fine").unwrap();
        let dest = dir.join(name);
        let format = PackFormat::of(&dest).unwrap();
        let mut asking = Asking::new(Reply::Skip);
        let plan = plan_pack(&[(box_dir.clone(), true)], &dest, format, None, &mut asking).unwrap().unwrap();
        assert_eq!(pack(&plan, &dest, format, PackLevel::Normal, None, false, &mut asking).unwrap(), Transfer::Done, "{name}");
        assert_eq!(remove_packed(&plan, &mut asking).unwrap(), Transfer::Done, "{name}");
        assert!(!box_dir.join("fine.txt").exists(), "{name}");
        assert!(box_dir.join("locked.txt").symlink_metadata().is_ok(), "{name}");
    }
    fs::set_permissions(box_dir.join("locked.txt"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}

/// A write that fails under the zstd encoder - which would panic on it -
/// comes back as the error it was, whether the writes stop on it or only
/// the end does, and nothing goes down.
#[cfg(target_os = "linux")]
#[test]
fn a_zstd_write_that_fails_says_so() {
    let full = File::options().write(true).open("/dev/full").unwrap();
    let mut zst = Compressor::new(full, PackFormat::TarZst, PackLevel::Normal).unwrap();
    // What does not compress, so the encoder writes as it goes.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let noise: Vec<u8> = (0..64 * 1024)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    let error = match (0..256).try_for_each(|_| zst.write_all(&noise)) {
        // As Pack stops: on the write, the end never reached.
        Err(error) => error,
        Ok(()) => zst.finish().unwrap_err(),
    };
    assert_eq!(error.kind(), ErrorKind::StorageFull, "{error}");
}

/// With a password a zip's files and a 7z's whole are encrypted: without
/// it nothing is read - a 7z not even listed - and with the wrong one,
/// that is said and the password let go of; with the right one, all of it.
#[test]
fn a_password_locks_a_zip_and_a_7z() {
    use crate::archive::{forget_password, locked, set_password};
    let dir = scratch("pack-password");
    let sources = tree(&dir.join("in"));
    for name in ["locked.zip", "locked.7z"] {
        let dest = dir.join(name);
        let format = PackFormat::of(&dest).unwrap();
        let mut asking = Asking::new(Reply::Abort);
        let plan = plan_pack(&sources, &dest, format, None, &mut asking).unwrap().unwrap();
        assert_eq!(pack(&plan, &dest, format, PackLevel::Normal, Some("sesame"), false, &mut asking).unwrap(), Transfer::Done);

        forget_password(&dest);
        assert!(crate::archive::is_encrypted(&dest) || format == PackFormat::SevenZ, "{name}");
        let needs = |dest: &Path| match crate::archive::index(dest) {
            // A 7z keeps its names locked too.
            Err(error) => locked(&error),
            Ok(index) => {
                let at = index.find(Path::new("docs/notes.txt")).unwrap();
                assert!(index.entry(at).encrypted, "{name}");
                assert!(!index.entry(index.find(Path::new("docs")).unwrap()).encrypted, "{name}");
                let mut found = None;
                crate::archive::read_entries(dest, index.kind, &[index.entry(at).position.unwrap()], &mut |_, read| {
                    found = read.err().as_ref().and_then(locked);
                    crate::archive::Next::Go
                })
                .unwrap();
                found
            }
        };
        assert_eq!(needs(&dest).map(|locked| locked.wrong), Some(false), "{name}");
        set_password(&dest, "open up");
        assert_eq!(needs(&dest).map(|locked| locked.wrong), Some(true), "{name}");
        assert_eq!(crate::archive::password(&dest), None, "{name}: a wrong one is forgotten");
        set_password(&dest, "sesame");
        assert!(contents(&dest).contains(&("docs/notes.txt".to_string(), "notes".to_string())), "{name}");
        forget_password(&dest);
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// What is left out is not packed, nor anything under it: a name for a
/// file or a directory alike, one ending in `/` for directories alone, and
/// a path for one place only.
#[test]
fn what_is_left_out_is_not_packed() {
    let dir = scratch("pack-leave-out");
    let sources = tree(&dir.join("in"));
    fs::create_dir_all(dir.join("in/docs/.git")).unwrap();
    fs::write(dir.join("in/docs/.git/HEAD"), "ref").unwrap();
    fs::write(dir.join("in/docs/main.o"), "obj").unwrap();
    fs::create_dir_all(dir.join("in/docs/empty.txt.d")).unwrap();
    let dest = dir.join("out.tar");
    let glob = crate::glob::Glob::exclude(".git;*.o;deep/;*.d/;docs/notes.txt").unwrap();
    let mut asking = Asking::new(Reply::Abort);
    let plan = plan_pack(&sources, &dest, PackFormat::Tar, Some(&glob), &mut asking).unwrap().unwrap();
    pack(&plan, &dest, PackFormat::Tar, PackLevel::Normal, None, false, &mut asking).unwrap();
    let mut expected = vec![("docs", "/"), ("docs/empty.txt", ""), ("top.txt", "top")];
    if cfg!(unix) {
        expected.push(("docs/link", "-> notes.txt"));
    }
    let mut expected: Vec<(String, String)> = expected.into_iter().map(|(path, held)| (path.to_string(), held.to_string())).collect();
    expected.sort();
    assert_eq!(contents(&dest), expected);
    fs::remove_dir_all(&dir).unwrap();
}

/// Two of what is packed with one name - files from the basket, or from
/// Find files, each from a directory of its own - do not go in twice under
/// it: a zip will not hold that, and a tar or a 7z would keep only the
/// last of them when unpacked. The second is asked about as a copy asks,
/// and goes as it says: in place of the first, left out, or beside it,
/// numbered. Two directories of one name are merged; a file and a
/// directory are asked about as a failure, the second left out.
#[test]
fn two_of_one_name_are_asked_about() {
    let dir = scratch("pack-same-name");
    for side in ["one", "two"] {
        fs::create_dir_all(dir.join(side).join("docs")).unwrap();
        fs::write(dir.join(side).join("a.txt"), side).unwrap();
        fs::write(dir.join(side).join("docs").join(format!("{side}.txt")), side).unwrap();
    }
    fs::create_dir_all(dir.join("three/a.txt")).unwrap();
    let sources = [(dir.join("one/a.txt"), false), (dir.join("two/a.txt"), false), (dir.join("one/docs"), true), (dir.join("two/docs"), true)];
    let held = |pairs: &[(&str, &str)]| pairs.iter().map(|(path, held)| (path.to_string(), held.to_string())).collect::<Vec<_>>();
    let docs = [("docs", "/"), ("docs/one.txt", "one"), ("docs/two.txt", "two")];
    for format in PackFormat::ALL {
        for (clash, mut expected) in [(Clash::Replace, held(&[("a.txt", "two")])), (Clash::Keep, held(&[("a.txt", "one")])), (Clash::KeepBoth, held(&[("a (2).txt", "two"), ("a.txt", "one")]))] {
            let dest = dir.join(format!("out-{clash:?}{}", format.extension()));
            let mut asking = Asking::new(Reply::Abort);
            asking.clash = clash;
            let done = pack_into(&sources, &dest, &mut asking).unwrap_or_else(|e| panic!("{format:?} {clash:?}: {e}"));
            assert_eq!(done, Transfer::Done, "{format:?} {clash:?}");
            expected.extend(held(&docs));
            expected.sort();
            assert_eq!(contents(&dest), expected, "{format:?} {clash:?}");
        }
        // A directory where a file is: a failure, and the directory left out.
        let dest = dir.join(format!("out-mixed{}", format.extension()));
        let mut asking = Asking::new(Reply::Skip);
        assert_eq!(pack_into(&[(dir.join("one/a.txt"), false), (dir.join("three/a.txt"), true)], &dest, &mut asking).unwrap(), Transfer::Done);
        assert_eq!(asking.failures, [dir.join("three/a.txt")], "{format:?}");
        assert_eq!(contents(&dest), held(&[("a.txt", "one")]), "{format:?}");
        // Cancelled when the answer is to stop.
        let mut asking = Asking::new(Reply::Abort);
        asking.clash = Clash::Abort;
        assert_eq!(pack_into(&sources, &dir.join(format!("out-abort{}", format.extension())), &mut asking).unwrap(), Transfer::Cancelled, "{format:?}");
    }
    fs::remove_dir_all(&dir).unwrap();
}
