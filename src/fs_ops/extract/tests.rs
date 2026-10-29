use super::*;
use crate::archive::fixtures::{Put, scratch, sevenz, tar, zip};

/// Answers every file in the way, and every failure, the same way, counts
/// the bytes, and can stop after so many files.
struct Asking {
    clash: Clash,
    reply: Reply,
    clashes: Vec<PathBuf>,
    failures: Vec<PathBuf>,
    bytes: u64,
    files_left: Option<usize>,
}

impl Asking {
    fn new(clash: Clash, reply: Reply) -> Self {
        Asking {
            clash,
            reply,
            clashes: Vec::new(),
            failures: Vec::new(),
            bytes: 0,
            files_left: None,
        }
    }
}

impl Progress for Asking {
    fn step(&mut self, step: Step<'_>) -> bool {
        match step {
            Step::Advanced(amount) => self.bytes += amount,
            Step::Starting(_) => {
                if let Some(left) = &mut self.files_left {
                    if *left == 0 {
                        return false;
                    }
                    *left -= 1;
                }
            }
        }
        true
    }

    fn failed(&mut self, path: &Path, _error: &Error) -> Reply {
        self.failures.push(path.to_path_buf());
        self.reply
    }

    fn exists(&mut self, source: &Path, _dest: &Path) -> Clash {
        self.clashes.push(source.to_path_buf());
        self.clash
    }
}

/// A directory comes out whole, in both kinds, with its files' contents and
/// times, and the bar counts every byte.
#[test]
fn a_directory_comes_out_whole() {
    let dir = scratch("extract-tree");
    for name in ["a.zip", "a.tar.gz", "a.tar.zst", "a.tar.xz", "a.tar.bz2", "a.7z"] {
        let archive = dir.join(name);
        if name.ends_with("zip") {
            zip(
                &archive,
                &[("docs/notes.txt", Put::File(b"notes")), ("docs/deep/more.txt", Put::File(b"more!")), ("docs/empty/", Put::Dir), ("other.txt", Put::File(b"no"))],
            );
        } else if name.ends_with("7z") {
            sevenz(
                &archive,
                &[("docs/empty", Put::Dir), ("docs/notes.txt", Put::File(b"notes")), ("docs/deep/more.txt", Put::File(b"more!")), ("other.txt", Put::File(b"no"))],
                true,
            );
        } else {
            tar(
                &archive,
                &[(b"docs/notes.txt", Put::File(b"notes")), (b"docs/deep/more.txt", Put::File(b"more!")), (b"docs/empty/", Put::Dir), (b"other.txt", Put::File(b"no"))],
            );
        }
        let out = dir.join(format!("out-{name}"));
        fs::create_dir_all(&out).unwrap();

        let mut asking = Asking::new(Clash::Replace, Reply::Abort);
        let done = extract(&archive, &[(PathBuf::from("docs"), out.join("docs"))], &mut asking).unwrap();
        assert_eq!(done, Transfer::Done, "{name}");
        assert_eq!(fs::read_to_string(out.join("docs/notes.txt")).unwrap(), "notes");
        assert_eq!(fs::read_to_string(out.join("docs/deep/more.txt")).unwrap(), "more!");
        assert!(out.join("docs/empty").is_dir());
        assert!(!out.join("other.txt").exists());
        assert_eq!(asking.bytes, 10);
        assert_eq!(asking.bytes, measure_extract(&archive, &[PathBuf::from("docs")]));
        if name.starts_with("a.tar") {
            let modified = fs::metadata(out.join("docs/notes.txt")).unwrap().modified().unwrap();
            assert_eq!(modified, std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// A file in the way is asked about: kept, it stays as it was; replaced, it
/// takes the archive's.
#[test]
fn a_file_in_the_way_is_asked_about() {
    let dir = scratch("extract-clash");
    let archive = dir.join("a.tar");
    tar(&archive, &[(b"note.txt", Put::File(b"from the archive"))]);
    let dest = dir.join("note.txt");

    fs::write(&dest, "mine").unwrap();
    let mut keeping = Asking::new(Clash::Keep, Reply::Abort);
    extract(&archive, &[(PathBuf::from("note.txt"), dest.clone())], &mut keeping).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "mine");
    assert_eq!(keeping.clashes, [archive.join("note.txt")]);
    // Still counted, so the bar reaches its end.
    assert_eq!(keeping.bytes, 16);

    let mut replacing = Asking::new(Clash::Replace, Reply::Abort);
    extract(&archive, &[(PathBuf::from("note.txt"), dest.clone())], &mut replacing).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "from the archive");
    fs::remove_dir_all(&dir).unwrap();
}

/// A link comes out as a link, in both kinds, pointing where it pointed -
/// never followed, nothing written through it.
#[cfg(unix)]
#[test]
fn a_link_comes_out_as_a_link() {
    let dir = scratch("extract-link");
    for name in ["a.zip", "a.tar", "a.7z"] {
        let archive = dir.join(name);
        if name.ends_with("zip") {
            zip(&archive, &[("target.txt", Put::File(b"t")), ("link", Put::Link("target.txt"))]);
        } else if name.ends_with("7z") {
            sevenz(&archive, &[("target.txt", Put::File(b"t")), ("link", Put::Link("target.txt"))], true);
        } else {
            tar(&archive, &[(b"target.txt", Put::File(b"t")), (b"link", Put::Link("target.txt"))]);
        }
        let out = dir.join(format!("out-{name}"));
        fs::create_dir_all(&out).unwrap();
        let mut asking = Asking::new(Clash::Replace, Reply::Abort);
        extract(&archive, &[(PathBuf::from("link"), out.join("link")), (PathBuf::from("target.txt"), out.join("target.txt"))], &mut asking).unwrap();
        assert!(out.join("link").symlink_metadata().unwrap().file_type().is_symlink(), "{name}");
        assert_eq!(fs::read_link(out.join("link")).unwrap(), Path::new("target.txt"), "{name}");
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Cancelled partway, what was out stays and nothing half-written is left.
#[test]
fn a_cancel_stops_between_files() {
    let dir = scratch("extract-cancel");
    let archive = dir.join("a.zip");
    zip(&archive, &[("one", Put::File(b"1")), ("two", Put::File(b"2")), ("three", Put::File(b"3"))]);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut asking = Asking::new(Clash::Replace, Reply::Abort);
    asking.files_left = Some(1);
    let items: Vec<(PathBuf, PathBuf)> = ["one", "two", "three"].iter().map(|name| (PathBuf::from(name), out.join(name))).collect();
    assert_eq!(extract(&archive, &items, &mut asking).unwrap(), Transfer::Cancelled);
    let left: Vec<_> = fs::read_dir(&out).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(left, ["one"]);
    fs::remove_dir_all(&dir).unwrap();
}

/// A file that cannot be written is asked about; skipped, the rest still
/// comes out.
#[test]
fn a_failure_is_asked_about_and_can_be_skipped() {
    let dir = scratch("extract-fail");
    let archive = dir.join("a.tar.gz");
    tar(&archive, &[(b"blocked/inner.txt", Put::File(b"x")), (b"fine.txt", Put::File(b"ok"))]);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    // A file where the directory has to go.
    fs::write(out.join("blocked"), "in the way").unwrap();
    let mut asking = Asking::new(Clash::Replace, Reply::Skip);
    let items = [(PathBuf::from("blocked"), out.join("blocked")), (PathBuf::from("fine.txt"), out.join("fine.txt"))];
    assert_eq!(extract(&archive, &items, &mut asking).unwrap(), Transfer::Done);
    assert_eq!(asking.failures, [archive.join("blocked")]);
    assert_eq!(fs::read_to_string(out.join("fine.txt")).unwrap(), "ok");
    assert_eq!(fs::read_to_string(out.join("blocked")).unwrap(), "in the way");
    fs::remove_dir_all(&dir).unwrap();
}
