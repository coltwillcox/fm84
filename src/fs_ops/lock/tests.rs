use super::*;
use crate::archive::fixtures::scratch;

/// Says yes to everything, and can stop at the first file.
#[derive(Default)]
struct Going {
    stop: bool,
    started: Vec<PathBuf>,
}

impl Progress for Going {
    fn step(&mut self, step: Step<'_>) -> bool {
        if let Step::Starting(path) = step {
            self.started.push(path.to_path_buf());
        }
        !self.stop
    }
}

fn lock_into(source: &Path, is_dir: bool, dest: &Path, password: &str) -> Result<Transfer, Error> {
    let mut going = Going::default();
    let plan = plan_lock(source, is_dir, dest, &mut going)?.expect("not stopped");
    lock(&plan, dest, password, false, &mut going)
}

fn unlock_into(source: &Path, dest: &Path, password: &str) -> Result<Transfer, Error> {
    let len = fs::metadata(source).unwrap().len();
    unlock(source, len, dest, password, false, &mut Going::default())
}

/// The names a lock gives and takes away: `.age` on a file, `.tar.age` on a
/// directory, and off again in any case - but not off a name that is
/// nothing else.
#[test]
fn locked_names_come_and_go() {
    assert_eq!(locked_name(Path::new("/a/notes.txt"), false), "notes.txt.age");
    assert_eq!(locked_name(Path::new("/a/photos"), true), "photos.tar.age");
    assert!(is_locked(Path::new("/a/notes.txt.age")));
    assert!(is_locked(Path::new("/a/NOTES.AGE")));
    assert!(!is_locked(Path::new("/a/.age")));
    assert!(!is_locked(Path::new("/a/notes.txt")));
    assert_eq!(unlocked_name(Path::new("/a/notes.txt.age")).as_deref(), Some(std::ffi::OsStr::new("notes.txt")));
    assert_eq!(unlocked_name(Path::new("/a/photos.tar.age")).as_deref(), Some(std::ffi::OsStr::new("photos.tar")));
    assert_eq!(unlocked_name(Path::new("/a/.age")), None);
}

/// A file locked is no longer readable as it was, and opens to the same
/// bytes with the same password - only with it.
#[test]
fn a_file_locks_and_opens_again_with_its_password() {
    let dir = scratch("lock-file");
    let source = dir.join("notes.txt");
    let words: Vec<u8> = (0..200_000u32).flat_map(|n| n.to_le_bytes()).collect();
    fs::write(&source, &words).unwrap();
    let locked = dir.join("notes.txt.age");
    assert_eq!(lock_into(&source, false, &locked, "hunter2").unwrap(), Transfer::Done);
    let sealed = fs::read(&locked).unwrap();
    assert!(sealed.starts_with(b"age-encryption.org/v1"), "the age format");
    assert!(!sealed.windows(64).any(|window| window == &words[..64]), "nothing left in the clear");

    let wrong = unlock_into(&locked, &dir.join("wrong.txt"), "hunter3").unwrap_err();
    assert_eq!(wrong.to_string(), "Wrong password");
    assert!(!dir.join("wrong.txt").exists(), "nothing written for a wrong one");

    let opened = dir.join("opened.txt");
    assert_eq!(unlock_into(&locked, &opened, "hunter2").unwrap(), Transfer::Done);
    assert_eq!(fs::read(&opened).unwrap(), words);
    // Nothing of fm84's left beside them.
    let mut left: Vec<String> = fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["notes.txt", "notes.txt.age", "opened.txt"]);
}

/// A directory goes in as a tar of itself, which fm84 reads as any archive
/// once it is opened again.
#[test]
fn a_directory_locks_as_a_tar() {
    let dir = scratch("lock-dir");
    let tree = dir.join("photos");
    fs::create_dir_all(tree.join("june")).unwrap();
    fs::write(tree.join("june/beach.jpg"), b"sand").unwrap();
    fs::write(tree.join("list.txt"), b"one, two").unwrap();
    let locked = dir.join("photos.tar.age");
    assert_eq!(lock_into(&tree, true, &locked, "pw").unwrap(), Transfer::Done);
    let opened = dir.join("photos.tar");
    assert_eq!(unlock_into(&locked, &opened, "pw").unwrap(), Transfer::Done);
    let index = crate::archive::index(&opened).unwrap();
    let mut names: Vec<String> = index.subtree(Path::new("")).into_iter().map(|at| index.entry(at).path.to_string_lossy().replace('\\', "/")).collect();
    names.sort();
    assert_eq!(names, ["photos", "photos/june", "photos/june/beach.jpg", "photos/list.txt"]);
}

/// Stopped, a lock leaves nothing behind - not even half a file.
#[test]
fn a_lock_stopped_leaves_nothing() {
    let dir = scratch("lock-stop");
    let source = dir.join("big.bin");
    fs::write(&source, vec![7u8; 1 << 20]).unwrap();
    let locked = dir.join("big.bin.age");
    let mut going = Going::default();
    let plan = plan_lock(&source, false, &locked, &mut going).unwrap().unwrap();
    going.stop = true;
    assert_eq!(lock(&plan, &locked, "pw", false, &mut going).unwrap(), Transfer::Cancelled);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "only the source");
}

/// A file that only has the ending is said to be no lock at all.
#[test]
fn a_file_not_locked_says_so() {
    let dir = scratch("lock-not");
    let fake = dir.join("fake.age");
    fs::write(&fake, b"just some text, long enough to be read as a header").unwrap();
    let error = unlock_into(&fake, &dir.join("fake"), "pw").unwrap_err();
    assert_eq!(error.to_string(), "Not a file locked with age");
}
