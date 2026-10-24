//! The editor's save: whole or not at all, and keeping what the file was.

use super::*;

#[test]
fn a_save_replaces_the_content_and_leaves_nothing_beside_it() {
    let dir = scratch("save");
    let file = dir.join("notes.txt");
    fs::write(&file, "old and longer than the new").unwrap();

    save_file(&file, b"new").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "new");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "a temporary was left behind");
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_save_keeps_the_mode_and_writes_through_a_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("save-link");
    let file = dir.join("run.sh");
    fs::write(&file, "echo old").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).unwrap();
    std::os::unix::fs::symlink(&file, dir.join("link.sh")).unwrap();

    save_file(&dir.join("link.sh"), b"echo new").unwrap();
    assert!(fs::symlink_metadata(dir.join("link.sh")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&file).unwrap(), "echo new");
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o750);
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_save_keeps_a_hard_link_joined() {
    let dir = scratch("save-hardlink");
    let file = dir.join("a.txt");
    fs::write(&file, "old").unwrap();
    fs::hard_link(&file, dir.join("b.txt")).unwrap();

    save_file(&file, b"new").unwrap();
    assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "new");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_read_only_file_is_not_replaced() {
    let dir = scratch("save-readonly");
    let file = dir.join("locked.txt");
    fs::write(&file, "old").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions.clone()).unwrap();

    // Root may write anything, so there is nothing to refuse.
    if File::options().write(true).open(&file).is_err() {
        assert!(save_file(&file, b"new").is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "old");
    }
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&file, permissions).unwrap();
    fs::remove_dir_all(&dir).unwrap();
}
