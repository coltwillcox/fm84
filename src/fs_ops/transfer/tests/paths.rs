//! Sizes, names, and where a copy or move is read to go.

use super::*;

#[test]
fn a_size_walk_counts_what_it_can_and_stops_when_asked() {
    let dir = scratch("walk-size");
    fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("b.bin"), vec![0u8; 2500]).unwrap();

    let mut reported = Vec::new();
    let size = walk_dir_size(&dir, &AtomicBool::new(false), &mut |bytes| reported.push(bytes)).unwrap();
    assert_eq!(size, Some(3500));
    reported.sort();
    assert_eq!(reported, [1000, 2500]);

    // Cancelled before it starts, it finds nothing and says so.
    assert_eq!(walk_dir_size(&dir, &AtomicBool::new(true), &mut |_| {}).unwrap(), None);
    // The directory itself has to be there; a vanished one is an error.
    assert!(walk_dir_size(&dir.join("gone"), &AtomicBool::new(false), &mut |_| {}).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

/// A file under two names is shown once in a directory's size, as du shows
/// it - but a copy writes it twice, so measuring for one counts both.
#[cfg(unix)]
#[test]
fn a_hard_linked_file_is_sized_once_and_measured_twice() {
    let dir = scratch("walk-links");
    fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::hard_link(dir.join("a.bin"), dir.join("sub").join("again.bin")).unwrap();

    assert_eq!(walk_dir_size(&dir, &AtomicBool::new(false), &mut |_| {}).unwrap(), Some(1000));
    assert_eq!(calculate_dir_size(&dir).unwrap(), 2000);
    fs::remove_dir_all(&dir).unwrap();
}

/// /proc holds a file the size of all memory, none of it on a disk.
#[cfg(target_os = "linux")]
#[test]
fn a_filesystem_the_kernel_makes_up_has_no_size() {
    assert_eq!(walk_dir_size(Path::new("/proc"), &AtomicBool::new(false), &mut |_| {}).unwrap(), Some(0));
}

#[test]
fn measure_adds_up_files_and_walks_directories() {
    let dir = scratch("measure");
    fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
    fs::create_dir(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("b.bin"), vec![0u8; 2500]).unwrap();

    let items = vec![(dir.join("a.bin"), dir.join("copy-a.bin"), false), (dir.join("sub"), dir.join("copy-sub"), true)];
    assert_eq!(measure(&items), 3500);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_plain_name_stays_where_it_is_put() {
    for name in ["notes.txt", ".hidden", "with space", "ünïcode", "a.b.c"] {
        assert!(is_plain_name(name), "{name}");
    }
    for name in ["", ".", "..", "../x", "sub/x", "x/", "/x", "./x"] {
        assert!(!is_plain_name(name), "{name}");
    }
    #[cfg(windows)]
    for name in ["sub\\x", "C:x", "C:"] {
        assert!(!is_plain_name(name), "{name}");
    }
}

#[cfg(unix)]
#[test]
fn an_entry_is_itself_and_nothing_else() {
    let dir = scratch("same");
    fs::write(dir.join("a"), "a").unwrap();
    fs::write(dir.join("b"), "b").unwrap();
    std::os::unix::fs::symlink(dir.join("a"), dir.join("link")).unwrap();

    assert!(is_same_entry(&dir.join("a"), &dir.join("a")));
    assert!(!is_same_entry(&dir.join("a"), &dir.join("b")));
    // A link to a is an entry of its own; renaming a over it must be refused.
    assert!(!is_same_entry(&dir.join("a"), &dir.join("link")));
    assert!(!is_same_entry(&dir.join("a"), &dir.join("missing")));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_destination_is_read_as_cp_reads_it() {
    let dir = scratch("targets");
    fs::create_dir(dir.join("into")).unwrap();
    let one = vec![(dir.join("a.txt"), false)];
    let two = vec![(dir.join("a.txt"), false), (dir.join("sub"), true)];

    // A directory that is there takes each under its own name, whether or
    // not it was typed with a separator after it.
    for names_dir in [false, true] {
        assert_eq!(
            transfer_targets(&two, &dir.join("into"), names_dir).unwrap(),
            [(dir.join("a.txt"), dir.join("into").join("a.txt"), false), (dir.join("sub"), dir.join("into").join("sub"), true)]
        );
    }
    // Anything else is a new name, for one source only.
    assert_eq!(transfer_targets(&one, &dir.join("b.txt"), false).unwrap(), [(dir.join("a.txt"), dir.join("b.txt"), false)]);
    assert!(transfer_targets(&two, &dir.join("b.txt"), false).unwrap_err().contains("2 items need a directory"));
    // In a directory that has to be there, and a separator means one.
    assert!(transfer_targets(&one, &dir.join("missing").join("b.txt"), false).unwrap_err().starts_with("No such directory"));
    assert!(transfer_targets(&one, &dir.join("missing"), true).unwrap_err().starts_with("No such directory"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn destinations_are_checked_before_anything_is_written() {
    let dir = scratch("check");
    let (from, to) = (dir.join("from"), dir.join("to"));
    fs::create_dir_all(from.join("tree")).unwrap();
    fs::create_dir_all(to.join("tree")).unwrap();
    for name in ["same", "new", "file-vs-dir"] {
        fs::write(from.join(name), name).unwrap();
    }
    fs::write(to.join("same"), "old").unwrap();
    fs::create_dir(to.join("file-vs-dir")).unwrap();
    let item = |name: &str| (from.join(name), to.join(name), from.join(name).is_dir());

    // Nothing in the way, and something in the way: the latter is reported.
    assert_eq!(check_destinations(&[item("new")]).unwrap(), Vec::<PathBuf>::new());
    assert_eq!(check_destinations(&[item("new"), item("same"), item("tree")]).unwrap(), [to.join("same"), to.join("tree")]);
    // What overwriting could never make right.
    assert!(check_destinations(&[item("file-vs-dir")]).unwrap_err().contains("Cannot replace directory"));
    assert!(check_destinations(&[(to.join("tree"), from.join("same"), true)]).unwrap_err().contains("Cannot replace file"));
    assert!(check_destinations(&[(from.join("same"), from.join("same"), false)]).unwrap_err().contains("already there"));
    assert!(check_destinations(&[(from.clone(), from.join("inside"), true)]).unwrap_err().contains("into itself"));
    fs::remove_dir_all(&dir).unwrap();
}
