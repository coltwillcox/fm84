//! Writing into an archive.

use super::*;
use crate::archive::fixtures::{Put, scratch, sevenz, tar, zip};

/// Answers a name already taken the same way each time, and every failure
/// too; remembers what it was asked about, and can stop after so many
/// steps.
struct Asking {
    clash: Clash,
    reply: Reply,
    asked: Vec<PathBuf>,
    failures: Vec<PathBuf>,
    steps_left: Option<usize>,
}

impl Asking {
    fn new(clash: Clash) -> Self {
        Asking {
            clash,
            reply: Reply::Abort,
            asked: Vec::new(),
            failures: Vec::new(),
            steps_left: None,
        }
    }
}

impl Progress for Asking {
    fn step(&mut self, _step: Step<'_>) -> bool {
        match &mut self.steps_left {
            Some(0) => false,
            Some(left) => {
                *left -= 1;
                true
            }
            None => true,
        }
    }

    fn failed(&mut self, path: &Path, _error: &Error) -> Reply {
        self.failures.push(path.to_path_buf());
        self.reply
    }

    fn exists(&mut self, _source: &Path, dest: &Path) -> Clash {
        self.asked.push(dest.to_path_buf());
        self.clash
    }
}

/// Plan and write, as the job does.
fn write_into(archive: &Path, edits: &Repack, move_after: bool, asking: &mut Asking) -> Result<Transfer, Error> {
    let Some(plan) = plan_repack(archive, edits, asking)? else {
        return Ok(Transfer::Cancelled);
    };
    repack(&plan, move_after, asking)
}

/// Everything in an archive by fm84's own reading of it, sorted: each path
/// with a file's contents, a link's target, or "/" for a directory.
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

fn listed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut listed: Vec<(String, String)> = pairs.iter().map(|(path, held)| (path.to_string(), held.to_string())).collect();
    listed.sort();
    listed
}

/// An archive of each format to write into: a directory with a file, an
/// empty file and one below; and a file beside it.
fn archives(dir: &Path) -> Vec<PathBuf> {
    let made = dir.join("made");
    fs::create_dir_all(made.join("docs/deep")).unwrap();
    fs::write(made.join("docs/notes.txt"), "notes").unwrap();
    fs::write(made.join("docs/empty.txt"), "").unwrap();
    fs::write(made.join("docs/deep/more.txt"), "more!").unwrap();
    fs::write(made.join("top.txt"), "top").unwrap();
    let sources = [(made.join("docs"), true), (made.join("top.txt"), false)];
    let archives = PackFormat::ALL
        .iter()
        .map(|format| {
            let dest = dir.join(format!("box{}", format.extension()));
            let mut ask = |_: Step<'_>| true;
            let plan = plan_pack(&sources, &dest, *format, None, &mut ask).unwrap().unwrap();
            pack(&plan, &dest, *format, PackLevel::Fast, None, false, &mut ask).unwrap();
            dest
        })
        .collect();
    fs::remove_dir_all(&made).unwrap();
    archives
}

/// Into every format: a file in at the top and one into a directory, a
/// directory taken out with what is in it, a directory and an empty file
/// made - and the rest just as it was, with no temporary left behind.
#[test]
fn every_format_takes_what_comes_in_and_lets_go_of_what_goes() {
    let dir = scratch("repack-formats");
    let boxes = archives(&dir);
    fs::write(dir.join("new.txt"), "new").unwrap();
    fs::write(dir.join("inside.txt"), "inside").unwrap();
    for archive in &boxes {
        let edits = Repack {
            remove: vec![PathBuf::from("docs/deep")],
            add: vec![(dir.join("new.txt"), PathBuf::from("new.txt")), (dir.join("inside.txt"), PathBuf::from("docs/inside.txt"))],
            make: vec![(PathBuf::from("docs/made"), true), (PathBuf::from("blank.txt"), false)],
        };
        let done = write_into(archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap_or_else(|e| panic!("{}: {e}", archive.display()));
        assert_eq!(done, Transfer::Done, "{}", archive.display());
        assert_eq!(
            contents(archive),
            listed(&[
                ("blank.txt", ""),
                ("docs", "/"),
                ("docs/empty.txt", ""),
                ("docs/inside.txt", "inside"),
                ("docs/made", "/"),
                ("docs/notes.txt", "notes"),
                ("new.txt", "new"),
                ("top.txt", "top"),
            ]),
            "{}",
            archive.display()
        );
    }
    let left: Vec<String> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| name.contains(".fm84-")).collect();
    assert!(left.is_empty(), "{left:?}");
    fs::remove_dir_all(&dir).unwrap();
}

/// A file onto one the archive has is asked about: replaced, kept, or put
/// in beside it; a directory onto a directory is merged, and not asked.
#[test]
fn a_name_taken_is_asked_about() {
    let dir = scratch("repack-clash");
    let boxes = archives(&dir);
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(dir.join("docs/notes.txt"), "new notes").unwrap();
    fs::write(dir.join("docs/extra.txt"), "extra").unwrap();
    let edits = Repack {
        add: vec![(dir.join("docs"), PathBuf::from("docs"))],
        ..Repack::default()
    };
    for archive in &boxes {
        let read = |name: &str| contents(archive).into_iter().find(|(path, _)| path == name).map(|(_, held)| held);
        let mut asking = Asking::new(Clash::Keep);
        assert_eq!(write_into(archive, &edits, false, &mut asking).unwrap(), Transfer::Done);
        assert_eq!(asking.asked, [fs::canonicalize(archive).unwrap().join("docs/notes.txt")], "{}", archive.display());
        assert_eq!(read("docs/notes.txt").as_deref(), Some("notes"));
        assert_eq!(read("docs/extra.txt").as_deref(), Some("extra"));

        assert_eq!(write_into(archive, &edits, false, &mut Asking::new(Clash::KeepBoth)).unwrap(), Transfer::Done);
        assert_eq!(read("docs/notes (2).txt").as_deref(), Some("new notes"), "{}", archive.display());
        assert_eq!(read("docs/notes.txt").as_deref(), Some("notes"));

        assert_eq!(write_into(archive, &edits, false, &mut Asking::new(Clash::Replace)).unwrap(), Transfer::Done);
        assert_eq!(read("docs/notes.txt").as_deref(), Some("new notes"), "{}", archive.display());
        // Once each, not twice over.
        assert_eq!(contents(archive).iter().filter(|(path, _)| path == "docs/notes.txt").count(), 1);
        assert_eq!(contents(archive).iter().filter(|(path, _)| path == "docs").count(), 1);

        let before = fs::read(archive).unwrap();
        assert_eq!(write_into(archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Cancelled);
        assert_eq!(fs::read(archive).unwrap(), before, "{}", archive.display());
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A file onto a directory, or a directory onto a file, is a failure asked
/// about; skipped, it is left out with what is in it, and the rest goes in.
#[test]
fn a_file_and_a_directory_in_each_others_place_are_left_out() {
    let dir = scratch("repack-kinds");
    let boxes = archives(&dir);
    fs::create_dir_all(dir.join("in/top.txt")).unwrap();
    fs::write(dir.join("in/top.txt/under.txt"), "under").unwrap();
    fs::write(dir.join("in/fine.txt"), "fine").unwrap();
    let edits = Repack {
        add: vec![(dir.join("in/top.txt"), PathBuf::from("top.txt")), (dir.join("in/fine.txt"), PathBuf::from("fine.txt"))],
        ..Repack::default()
    };
    for archive in &boxes {
        let mut asking = Asking::new(Clash::Replace);
        asking.reply = Reply::Skip;
        assert_eq!(write_into(archive, &edits, false, &mut asking).unwrap(), Transfer::Done);
        assert_eq!(asking.failures, [dir.join("in/top.txt")], "{}", archive.display());
        let held = contents(archive);
        assert!(held.contains(&("top.txt".to_string(), "top".to_string())), "{held:?}");
        assert!(held.contains(&("fine.txt".to_string(), "fine".to_string())), "{held:?}");
        assert!(!held.iter().any(|(path, _)| path.starts_with("top.txt/")), "{held:?}");
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Cancelled partway, the archive is as it was, byte for byte, and nothing
/// is left beside it.
#[test]
fn a_cancel_leaves_the_archive_as_it_was() {
    let dir = scratch("repack-cancel");
    let boxes = archives(&dir);
    fs::write(dir.join("big.txt"), "x".repeat(100_000)).unwrap();
    for archive in &boxes {
        let before = fs::read(archive).unwrap();
        let mut asking = Asking::new(Clash::Abort);
        asking.steps_left = Some(3);
        let edits = Repack {
            add: vec![(dir.join("big.txt"), PathBuf::from("big.txt"))],
            remove: vec![PathBuf::from("top.txt")],
            ..Repack::default()
        };
        assert_eq!(write_into(archive, &edits, true, &mut asking).unwrap(), Transfer::Cancelled, "{}", archive.display());
        assert_eq!(fs::read(archive).unwrap(), before, "{}", archive.display());
        assert!(dir.join("big.txt").exists());
    }
    let left: Vec<String> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).filter(|name| name.contains(".fm84-")).collect();
    assert!(left.is_empty(), "{left:?}");
    fs::remove_dir_all(&dir).unwrap();
}

/// Moved in, what went in is off the disk, and what was kept out of it -
/// a file already there, answered Keep - is not.
#[test]
fn a_move_in_takes_off_the_disk_only_what_went_in() {
    let dir = scratch("repack-move");
    let boxes = archives(&dir);
    for archive in &boxes {
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/notes.txt"), "mine").unwrap();
        fs::write(dir.join("docs/fresh.txt"), "fresh").unwrap();
        let edits = Repack {
            add: vec![(dir.join("docs"), PathBuf::from("docs"))],
            ..Repack::default()
        };
        assert_eq!(write_into(archive, &edits, true, &mut Asking::new(Clash::Keep)).unwrap(), Transfer::Done);
        assert!(!dir.join("docs/fresh.txt").exists(), "{}", archive.display());
        assert_eq!(fs::read_to_string(dir.join("docs/notes.txt")).unwrap(), "mine");
        assert!(contents(archive).contains(&("docs/fresh.txt".to_string(), "fresh".to_string())));
        fs::remove_dir_all(dir.join("docs")).unwrap();
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// What fm84 will not list - a name reaching outside the archive - is not
/// lost by writing into it: it goes back in as it was.
#[test]
fn entries_left_out_of_the_listing_are_kept() {
    let dir = scratch("repack-hostile");
    let archive = dir.join("odd.tar");
    tar(&archive, &[(b"../outside.txt", Put::File(b"out")), (b"fine.txt", Put::File(b"fine")), (b"link", Put::Link("fine.txt"))]);
    fs::write(dir.join("new.txt"), "new").unwrap();
    let edits = Repack {
        add: vec![(dir.join("new.txt"), PathBuf::from("new.txt"))],
        ..Repack::default()
    };
    assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done);
    let mut names = Vec::new();
    let mut read = tar::Archive::new(File::open(&archive).unwrap());
    for entry in read.entries().unwrap() {
        let entry = entry.unwrap();
        names.push((String::from_utf8_lossy(&entry.path_bytes()).into_owned(), entry.header().mode().unwrap(), entry.header().mtime().unwrap()));
    }
    assert_eq!(names[0], ("../outside.txt".to_string(), 0o640, 1_700_000_000));
    assert_eq!(names.iter().map(|(name, _, _)| name.as_str()).collect::<Vec<_>>(), ["../outside.txt", "fine.txt", "link", "new.txt"]);
    assert_eq!(crate::archive::index(&archive).unwrap().refused, 1);
    assert!(contents(&archive).contains(&("link".to_string(), "-> fine.txt".to_string())));
    fs::remove_dir_all(&dir).unwrap();
}

/// A zip's entries are carried across as stored: one kept uncompressed
/// stays so, its time with it.
#[test]
fn a_zip_keeps_its_entries_as_stored() {
    let dir = scratch("repack-zip-stored");
    let archive = dir.join("stored.zip");
    zip(&archive, &[("a.txt", Put::File(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")), ("d", Put::Dir)]);
    let method = |archive: &Path| zip::ZipArchive::new(File::open(archive).unwrap()).unwrap().by_name("a.txt").unwrap().compression();
    let was = method(&archive);
    fs::write(dir.join("b.txt"), "b").unwrap();
    let edits = Repack {
        add: vec![(dir.join("b.txt"), PathBuf::from("d/b.txt"))],
        ..Repack::default()
    };
    assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done);
    assert_eq!(method(&archive), was);
    assert_eq!(contents(&archive), listed(&[("a.txt", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), ("d", "/"), ("d/b.txt", "b")]));
    fs::remove_dir_all(&dir).unwrap();
}

/// A 7z of many blocks, as one not packed solid is, is read out of each in
/// turn, and written solid; one solid has every file before another read.
#[test]
fn a_7z_is_carried_across_block_by_block() {
    let dir = scratch("repack-7z");
    fs::write(dir.join("new.txt"), "new").unwrap();
    for solid in [false, true] {
        let archive = dir.join(format!("many-{solid}.7z"));
        sevenz(
            &archive,
            &[
                ("d", Put::Dir),
                ("d/one.txt", Put::File(b"one")),
                ("empty.txt", Put::File(b"")),
                ("two.txt", Put::File(b"two")),
                ("three.txt", Put::File(b"three")),
                ("link", Put::Link("two.txt")),
            ],
            solid,
        );
        let edits = Repack {
            add: vec![(dir.join("new.txt"), PathBuf::from("d/new.txt"))],
            remove: vec![PathBuf::from("two.txt")],
            ..Repack::default()
        };
        assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done, "solid {solid}");
        assert_eq!(
            contents(&archive),
            listed(&[("d", "/"), ("d/new.txt", "new"), ("d/one.txt", "one"), ("empty.txt", ""), ("link", "-> two.txt"), ("three.txt", "three")]),
            "solid {solid}"
        );
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A link to an archive writes the archive it points at, and stays a link.
#[cfg(unix)]
#[test]
fn through_a_link_the_archive_itself_is_written() {
    let dir = scratch("repack-link");
    let boxes = archives(&dir);
    let link = dir.join("link.zip");
    std::os::unix::fs::symlink(&boxes[0], &link).unwrap();
    fs::write(dir.join("new.txt"), "new").unwrap();
    let edits = Repack {
        add: vec![(dir.join("new.txt"), PathBuf::from("new.txt"))],
        ..Repack::default()
    };
    assert_eq!(write_into(&link, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done);
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(contents(&boxes[0]).contains(&("new.txt".to_string(), "new".to_string())));
    fs::remove_dir_all(&dir).unwrap();
}

/// A name made that is taken already is refused before anything is
/// written; a `.tar.zst` made by something else is written into as any is.
#[test]
fn a_name_made_that_is_taken_is_refused() {
    let dir = scratch("repack-refused");
    let boxes = archives(&dir);
    let edits = Repack {
        make: vec![(PathBuf::from("docs"), true)],
        ..Repack::default()
    };
    let error = write_into(&boxes[0], &edits, false, &mut Asking::new(Clash::Abort)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    let zst = dir.join("theirs.tar.zst");
    tar(&zst, &[(b"a.txt", Put::File(b"a"))]);
    let edits = Repack {
        make: vec![(PathBuf::from("b"), true)],
        ..Repack::default()
    };
    assert_eq!(write_into(&zst, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done);
    assert_eq!(contents(&zst), listed(&[("a.txt", "a"), ("b", "/")]));
    assert_eq!(writable_format(&zst), Ok(PackFormat::TarZst));
    assert_eq!(writable_format(&boxes[0]), Ok(PackFormat::Zip));
    fs::remove_dir_all(&dir).unwrap();
}

/// Written into, an encrypted archive stays so: what is kept is read with
/// its password, and what comes in is encrypted with it.
#[test]
fn an_encrypted_archive_is_written_into_with_its_password() {
    use crate::archive::{forget_password, set_password};
    let dir = scratch("repack-password");
    fs::create_dir_all(dir.join("in")).unwrap();
    fs::write(dir.join("in/old.txt"), "old").unwrap();
    fs::write(dir.join("new.txt"), "new").unwrap();
    for name in ["locked.zip", "locked.7z"] {
        let archive = dir.join(name);
        let format = PackFormat::of(&archive).unwrap();
        let mut ask = |_: Step<'_>| true;
        let plan = plan_pack(&[(dir.join("in/old.txt"), false)], &archive, format, None, &mut ask).unwrap().unwrap();
        pack(&plan, &archive, format, PackLevel::Fast, Some("sesame"), false, &mut ask).unwrap();
        set_password(&archive, "sesame");
        let edits = Repack {
            add: vec![(dir.join("new.txt"), PathBuf::from("new.txt"))],
            ..Repack::default()
        };
        assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done, "{name}");
        assert_eq!(contents(&archive), listed(&[("new.txt", "new"), ("old.txt", "old")]), "{name}");
        let index = crate::archive::index(&archive).unwrap();
        assert!(index.entry(index.find(Path::new("new.txt")).unwrap()).encrypted, "{name}");
        forget_password(&archive);
        // A 7z's names are locked again with it - asked of the file, past
        // the index kept from reading it with the password.
        if format == PackFormat::SevenZ {
            assert!(sevenz_rust2::ArchiveReader::new(File::open(&archive).unwrap(), sevenz_rust2::Password::empty()).is_err());
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A tar that keeps a file's size in a pax record, as bsdtar does past
/// 8 GiB, keeps the whole file when written into: the size in its header
/// is not the one that counts.
#[test]
fn a_pax_size_is_kept() {
    let dir = scratch("repack-pax");
    let archive = dir.join("pax.tar");
    let mut builder = tar::Builder::new(Vec::new());
    builder.append_pax_extensions([("size", b"11".as_slice())]).unwrap();
    let mut header = tar::Header::new_ustar();
    header.set_path("big.txt").unwrap();
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    // What a writer leaves in the header when the size will not fit there.
    header.set_size(0);
    header.set_cksum();
    builder.append(&header, b"eleven byte".as_slice()).unwrap();
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();
    // Read as tar reads it - not through the index, which would be kept.
    let mut read = tar::Archive::new(File::open(&archive).unwrap());
    let mut entry = read.entries().unwrap().next().unwrap().unwrap();
    assert_eq!((entry.size(), entry.header().entry_size().unwrap()), (11, 0));
    let mut held = String::new();
    entry.read_to_string(&mut held).unwrap();
    assert_eq!(held, "eleven byte");

    fs::write(dir.join("new.txt"), "new").unwrap();
    let edits = Repack {
        add: vec![(dir.join("new.txt"), PathBuf::from("new.txt"))],
        ..Repack::default()
    };
    assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap(), Transfer::Done);
    assert_eq!(contents(&archive), listed(&[("big.txt", "eleven byte"), ("new.txt", "new")]));
    fs::remove_dir_all(&dir).unwrap();
}

/// Written into with the wrong password, an encrypted zip - whose entries
/// are carried across without being opened - is left as it was, rather
/// than taking new entries under a second password; the wrong one is let
/// go of.
#[test]
fn a_wrong_password_writes_nothing_into_a_zip() {
    use crate::archive::{forget_password, locked, password, set_password};
    let dir = scratch("repack-wrong-password");
    fs::write(dir.join("old.txt"), "old").unwrap();
    fs::write(dir.join("new.txt"), "new").unwrap();
    let archive = dir.join("locked.zip");
    let mut ask = |_: Step<'_>| true;
    let plan = plan_pack(&[(dir.join("old.txt"), false)], &archive, PackFormat::Zip, None, &mut ask).unwrap().unwrap();
    pack(&plan, &archive, PackFormat::Zip, PackLevel::Fast, Some("sesame"), false, &mut ask).unwrap();
    let before = fs::read(&archive).unwrap();
    set_password(&archive, "sesmae");
    let edits = Repack {
        add: vec![(dir.join("new.txt"), PathBuf::from("new.txt"))],
        ..Repack::default()
    };
    let error = write_into(&archive, &edits, false, &mut Asking::new(Clash::Abort)).unwrap_err();
    assert_eq!(locked(&error).map(|locked| locked.wrong), Some(true), "{error}");
    assert_eq!(fs::read(&archive).unwrap(), before);
    assert_eq!(password(&archive), None);
    forget_password(&archive);
    fs::remove_dir_all(&dir).unwrap();
}

/// A tar's hard link names the entry before it that holds its contents.
/// That entry deleted, or replaced, the contents stay with the link - the
/// first of them takes them, and the rest name it instead - rather than
/// going with the name.
#[test]
fn a_hard_link_keeps_its_contents_when_its_target_goes() {
    let dir = scratch("repack-hard-link");
    fs::write(dir.join("a.txt"), "replaced").unwrap();
    for replacing in [false, true] {
        let archive = dir.join(format!("links-{replacing}.tar"));
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(5);
        builder.append_data(&mut header, "d/a.txt", b"first".as_slice()).unwrap();
        for name in ["b.txt", "c.txt"] {
            let mut link = tar::Header::new_gnu();
            link.set_entry_type(tar::EntryType::Link);
            link.set_size(0);
            builder.append_link(&mut link, name, "d/a.txt").unwrap();
        }
        fs::write(&archive, builder.into_inner().unwrap()).unwrap();

        let edits = if replacing {
            Repack {
                add: vec![(dir.join("a.txt"), PathBuf::from("d/a.txt"))],
                ..Repack::default()
            }
        } else {
            Repack {
                remove: vec![PathBuf::from("d")],
                ..Repack::default()
            }
        };
        assert_eq!(write_into(&archive, &edits, false, &mut Asking::new(Clash::Replace)).unwrap(), Transfer::Done);
        // Read as tar reads it, each link through to what it names.
        let out = dir.join(format!("out-{replacing}"));
        fs::create_dir_all(&out).unwrap();
        tar::Archive::new(File::open(&archive).unwrap()).unpack(&out).unwrap();
        assert_eq!(fs::read_to_string(out.join("b.txt")).unwrap(), "first", "replacing {replacing}");
        assert_eq!(fs::read_to_string(out.join("c.txt")).unwrap(), "first", "replacing {replacing}");
        assert_eq!(fs::read_to_string(out.join("d/a.txt")).ok().as_deref(), replacing.then_some("replaced"), "replacing {replacing}");
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Two of one name copied in at once - from the basket, say - go in once
/// each: the second asked about, and here put beside the first.
#[test]
fn two_of_one_name_copied_in_go_in_once_each() {
    let dir = scratch("repack-same-name");
    let boxes = archives(&dir);
    for side in ["one", "two"] {
        fs::create_dir_all(dir.join(side)).unwrap();
        fs::write(dir.join(side).join("a.txt"), side).unwrap();
    }
    let edits = Repack {
        add: vec![(dir.join("one/a.txt"), PathBuf::from("a.txt")), (dir.join("two/a.txt"), PathBuf::from("a.txt"))],
        ..Repack::default()
    };
    for archive in &boxes {
        let mut asking = Asking::new(Clash::KeepBoth);
        assert_eq!(write_into(archive, &edits, false, &mut asking).unwrap(), Transfer::Done, "{}", archive.display());
        let held = contents(archive);
        assert!(held.contains(&("a.txt".to_string(), "one".to_string())), "{}: {held:?}", archive.display());
        assert!(held.contains(&("a (2).txt".to_string(), "two".to_string())), "{}: {held:?}", archive.display());
        assert_eq!(asking.asked.len(), 1, "{}", archive.display());
    }
    fs::remove_dir_all(&dir).unwrap();
}
