//! Moves: renames where they can, merges, and what a skip leaves behind.

use super::*;

/// A move merging into a directory goes entry by entry, and one it cannot
/// take from its source stays there, copied but not removed, rather than
/// gone from both.
#[cfg(unix)]
#[test]
fn a_skipped_move_leaves_the_source_of_what_it_skipped() {
    let dir = scratch("skip-move");
    let source = dir.join("a");
    fs::create_dir_all(source.join("locked")).unwrap();
    fs::write(source.join("locked").join("stuck.txt"), "stuck").unwrap();
    fs::write(source.join("free.txt"), "free").unwrap();
    let dest = dir.join("b");
    fs::create_dir_all(dest.join("locked")).unwrap();
    fs::write(dest.join("locked").join("other.txt"), "").unwrap();
    if locked(&source.join("locked")).is_none() {
        return fs::remove_dir_all(&dir).unwrap();
    }

    let mut progress = Answering::new(Reply::Skip);
    assert_eq!(move_path(source.clone(), dest.clone(), &mut progress).unwrap(), Transfer::Done);
    assert_eq!(progress.asked, [source.join("locked").join("stuck.txt")]);
    assert_eq!(fs::read_to_string(dest.join("free.txt")).unwrap(), "free");
    assert!(!path_exists(&source.join("free.txt")));
    assert_eq!(fs::read_to_string(source.join("locked").join("stuck.txt")).unwrap(), "stuck");
    assert_eq!(fs::read_to_string(dest.join("locked").join("stuck.txt")).unwrap(), "stuck");

    unlock(&source.join("locked"));
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn only_a_cross_device_rename_falls_back_to_copying() {
    assert!(is_cross_device(&Error::from_raw_os_error(libc::EXDEV)));
    // 17, which the fallback used to take for Windows' cross-device error.
    assert!(!is_cross_device(&Error::from_raw_os_error(libc::EEXIST)));
    assert!(!is_cross_device(&Error::from_raw_os_error(libc::ENOTEMPTY)));
    assert!(!is_cross_device(&Error::from_raw_os_error(libc::EACCES)));
}

#[test]
fn a_move_onto_a_directory_merges_into_it() {
    let dir = scratch("merge");
    let (source, dest) = (dir.join("src"), dir.join("dst"));
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("clash"), "new").unwrap();
    fs::write(source.join("sub").join("only-in-source"), "s").unwrap();
    fs::create_dir_all(dest.join("sub")).unwrap();
    fs::write(dest.join("clash"), "old").unwrap();
    fs::write(dest.join("only-in-dest"), "d").unwrap();

    // Skipped: the rest merges, and the file in the way stays as it was -
    // as does the one that would have replaced it, with its directory.
    let mut keeping = Clashing::new(Clash::Keep);
    assert_eq!(move_path(source.clone(), dest.clone(), &mut keeping).unwrap(), Transfer::Done);
    assert_eq!(keeping.asked, [(source.join("clash"), dest.join("clash"))]);
    assert_eq!(fs::read_to_string(dest.join("clash")).unwrap(), "old");
    assert_eq!(fs::read_to_string(source.join("clash")).unwrap(), "new");
    assert!(dest.join("sub").join("only-in-source").exists());

    // Written over, which is what a caller with nobody to ask gets.
    let mut report = |_: Step<'_>| true;
    assert_eq!(move_path(source.clone(), dest.clone(), &mut report).unwrap(), Transfer::Done);
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(dest.join("clash")).unwrap(), "new");
    assert_eq!(fs::read_to_string(dest.join("only-in-dest")).unwrap(), "d");
    assert_eq!(fs::read_to_string(dest.join("sub").join("only-in-source")).unwrap(), "s");
    fs::remove_dir_all(&dir).unwrap();
}

/// Merging within one filesystem renames each entry into place rather
/// than copying it and deleting the source. A copy would be a new file
/// with a new inode; a rename keeps the one it had, and takes no time
/// however large the file.
#[cfg(unix)]
#[test]
fn a_merge_on_one_filesystem_renames_rather_than_copies() {
    use std::os::unix::fs::MetadataExt;
    let inode = |path: &Path| fs::symlink_metadata(path).unwrap().ino();

    let dir = scratch("merge-rename");
    let (source, dest) = (dir.join("src"), dir.join("dst"));
    fs::create_dir_all(source.join("shared")).unwrap();
    fs::create_dir_all(source.join("fresh")).unwrap();
    fs::write(source.join("shared").join("big.bin"), vec![0u8; 4096]).unwrap();
    fs::write(source.join("fresh").join("inner.txt"), "i").unwrap();
    fs::write(source.join("clash"), "new").unwrap();
    fs::create_dir_all(dest.join("shared")).unwrap();
    fs::write(dest.join("shared").join("keep.txt"), "k").unwrap();
    fs::write(dest.join("clash"), "old").unwrap();

    let before = [inode(&source.join("shared").join("big.bin")), inode(&source.join("fresh")), inode(&source.join("clash"))];
    let mut report = |_: Step<'_>| true;
    assert_eq!(move_path(source.clone(), dest.clone(), &mut report).unwrap(), Transfer::Done);

    // A file inside a directory both sides have, a directory only the
    // source has (moved whole), and a file replacing one in the way.
    let after = [inode(&dest.join("shared").join("big.bin")), inode(&dest.join("fresh")), inode(&dest.join("clash"))];
    assert_eq!(before, after);
    assert_eq!(fs::read_to_string(dest.join("clash")).unwrap(), "new");
    assert_eq!(fs::read_to_string(dest.join("shared").join("keep.txt")).unwrap(), "k");
    assert!(!source.exists());
    fs::remove_dir_all(&dir).unwrap();
}

/// Merged into a directory holding another name for the same file - a
/// hard link - the move takes the source's name away and is done, rather
/// than leaving it and failing on its directory as not empty.
#[cfg(unix)]
#[test]
fn a_move_onto_its_own_hard_link_takes_the_old_name_away() {
    let dir = scratch("move-hardlink");
    let (source, dest) = (dir.join("src"), dir.join("dst"));
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("shared"), "one file, two names").unwrap();
    fs::hard_link(source.join("shared"), dest.join("shared")).unwrap();
    fs::write(source.join("other"), "o").unwrap();

    // Not asked about: nothing would be written over.
    let mut asking = Clashing::new(Clash::Keep);
    assert_eq!(move_path(source.clone(), dest.clone(), &mut asking).unwrap(), Transfer::Done);
    assert!(asking.asked.is_empty(), "{:?}", asking.asked);
    assert!(!source.exists(), "the source directory is gone, emptied");
    assert_eq!(fs::read_to_string(dest.join("shared")).unwrap(), "one file, two names");
    assert_eq!(fs::read_to_string(dest.join("other")).unwrap(), "o");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_move_onto_a_file_replaces_it() {
    let dir = scratch("replace");
    fs::write(dir.join("a"), "new").unwrap();
    fs::write(dir.join("b"), "old").unwrap();
    let mut report = |_: Step<'_>| true;
    assert_eq!(move_path(dir.join("a"), dir.join("b"), &mut report).unwrap(), Transfer::Done);
    assert_eq!(fs::read_to_string(dir.join("b")).unwrap(), "new");
    assert!(!dir.join("a").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rename_within_one_filesystem_moves_no_bytes() {
    let dir = scratch("rename");
    let source = dir.join("a.bin");
    let dest = dir.join("b.bin");
    fs::write(&source, vec![3u8; 4096]).unwrap();

    assert!(rename_in_place(&source, &dest));
    assert!(!source.exists() && dest.exists());
    // Across filesystems it has to say so rather than pretend.
    assert!(!rename_in_place(&dest, Path::new("/proc/fm84-cannot-go-here")));
    fs::remove_dir_all(&dir).unwrap();
}
