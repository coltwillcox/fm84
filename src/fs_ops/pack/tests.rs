use super::*;
use crate::archive::fixtures::scratch;

/// Answers every failure the same way, remembers what it was asked about,
/// and can stop after so many files.
struct Asking {
    reply: Reply,
    failures: Vec<PathBuf>,
    files_left: Option<usize>,
}

impl Asking {
    fn new(reply: Reply) -> Self {
        Asking { reply, failures: Vec::new(), files_left: None }
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
}

/// Plan and pack, as the job does.
fn pack_into(sources: &[(PathBuf, bool)], dest: &Path, asking: &mut Asking) -> Result<Transfer, Error> {
    let format = PackFormat::of(dest).expect("named as an archive");
    let Some(plan) = plan_pack(sources, dest, format, asking)? else {
        return Ok(Transfer::Cancelled);
    };
    pack(&plan, dest, format, false, asking)
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
    let plan = plan_pack(&[(dir.clone(), true)], &dest, format, &mut asking).unwrap().unwrap();
    assert_eq!(pack(&plan, &dest, format, true, &mut asking).unwrap(), Transfer::Done);
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
